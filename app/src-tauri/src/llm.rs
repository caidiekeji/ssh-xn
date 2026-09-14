// LLM 命令层（模块B + F3/F4/F8.6）：上下文采集 → 脱敏 → 历史检索注入 →
// Prompt 组装 → SSE 流式 → 结构化校验 → 事件推送（chunk/done/error）
use std::sync::Arc;

use futures_util::StreamExt;
use rusqlite::params;
use tauri::{AppHandle, Emitter};

use ai_ssh_core::llm::{resolve_routing, stream_chat, LlmChunk};
use ai_ssh_core::memory::{self, CaseFilter};
use ai_ssh_core::prompt::{self, AlertAnalysisCtx, CommandEnvCtx};
use ai_ssh_core::schema::CommandResult;
use ai_ssh_core::{Error, Result};

use crate::context;
use crate::monitor as met;
use crate::state::AppState;
use crate::ssh;

/// 把 Provider 列表按场景路由解析成可调用的 provider 序列。
pub async fn resolve_for_scene(state: &Arc<AppState>, scene: &str) -> Result<Vec<ai_ssh_core::schema::ProviderConfig>> {
    let conn = state.conn()?;
    resolve_routing(&conn, scene)
}

/// 统一流式入口：按 provider 顺序尝试（场景主 + fallback + enabled 兜底），
/// 全部失败抛错误（AC-6 降级提示由前端呈现）。
async fn stream_scene(
    state: &Arc<AppState>,
    scene: &str,
    messages: Vec<ai_ssh_core::prompt::ChatMessage>,
    on_chunk: impl Fn(String),
) -> Result<String> {
    let providers = resolve_for_scene(state, scene).await?;
    if providers.is_empty() {
        return Err(Error::Llm("未配置可用的 LLM Provider".into()));
    }
    let mut last_err = String::from("所有 Provider 均失败");
    for provider in providers {
        let provider_id = provider.id;
        match stream_chat(&provider, messages.clone()).await {
            Ok(stream) => {
                let mut buf = String::new();
                let mut ok = true;
                let mut stream = stream;
                while let Some(item) = stream.next().await {
                    match item {
                        LlmChunk::Ok(delta) => {
                            buf.push_str(&delta);
                            on_chunk(delta);
                        }
                        LlmChunk::Err(e) => {
                            last_err = e.to_string();
                            ok = false;
                            break;
                        }
                    }
                }
                if ok {
                    // Token 用量记录（F2.3）
                    if let Ok(conn) = state.conn() {
                        let prompt_txt: String = messages
                            .iter()
                            .map(|m| format!("{}: {}", m.role, m.content))
                            .collect::<Vec<_>>()
                            .join("\n");
                        let _ = ai_ssh_core::llm::record_token_usage(
                            &conn,
                            provider_id,
                            scene,
                            ai_ssh_core::llm::estimate_tokens(&prompt_txt),
                            ai_ssh_core::llm::estimate_tokens(&buf),
                        );
                    }
                    return Ok(buf);
                }
            }
            Err(e) => last_err = e.to_string(),
        }
    }
    Err(Error::Llm(last_err))
}

/// 组装命令生成所需环境上下文（F3.2）：OS/路径/最近命令/资源快照/历史 Top3。
async fn build_command_env(state: &Arc<AppState>, session_id: &str, host_id: i64, user_input: &str) -> CommandEnvCtx {
    let mut os_info = String::from("未知");
    let mut cwd = String::from("/");
    let mut recent: Vec<String> = Vec::new();
    let mut cpu = None;
    let mut mem_used = None;
    let mut mem_total = None;
    let mut mem_pct = None;
    let mut loadavg = None;
    let mut disk_max = None;
    let mut memory_hits: Vec<String> = Vec::new();

    // 远端环境探测（一次 exec 拿全）
    if let Ok((out, _)) = ssh::exec(state, session_id, "uname -sr; pwd; history 2>/dev/null | tail -10 || true").await {
        let mut lines = out.lines();
        os_info = lines.next().unwrap_or("未知").trim().to_string();
        cwd = lines.next().unwrap_or("/").trim().to_string();
        recent = lines.map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
    }

    // 实时资源快照（若监控开启）
    if let Ok(snap) = met::get_snapshot(state, host_id) {
        cpu = Some(snap.cpu.total_pct);
        mem_used = Some(snap.memory.used_mb);
        mem_total = Some(snap.memory.total_mb);
        mem_pct = Some(snap.memory.pct);
        loadavg = Some(format!("{:.2}/{:.2}/{:.2}", snap.load.load1, snap.load.load5, snap.load.load15));
        disk_max = snap.disks.iter().map(|d| d.pct).fold(0.0_f64, f64::max).into();
    }

    // 同主机历史问题 Top3（F5.3）
    if let Ok(conn) = state.conn() {
        if let Ok(hits) = memory::search_cases(&conn, host_id, user_input, 3) {
            memory_hits = hits
                .iter()
                .map(|c| {
                    format!(
                        "[id={}] 问题: {} | 根因: {} | 方案: {}",
                        c.id.unwrap_or(0),
                        c.description,
                        c.root_cause.clone().unwrap_or_default(),
                        c.solution_cmd.clone().unwrap_or_default()
                    )
                })
                .collect();
        }
    }

    CommandEnvCtx {
        host: format!("host#{}", host_id),
        os_info,
        cwd,
        cpu_pct: cpu,
        mem_used,
        mem_total,
        mem_pct,
        loadavg,
        disk_max_pct: disk_max,
        recent_commands: recent,
        memory_hits,
        user_input: user_input.to_string(),
    }
}

/// llm_generate_command：F3 命令生成（流式，最终事件带 CommandResult）。
pub async fn generate_command(state: &Arc<AppState>, app: &AppHandle, session_id: &str, user_input: &str) -> Result<()> {
    let event = "llm_generate_command";
    let s = state
        .sessions
        .lock()
        .unwrap()
        .get(session_id)
        .cloned()
        .ok_or_else(|| Error::NotFound(format!("会话 {session_id}")))?;
    let host_id = s.host_id;

    // 1. 环境上下文 + 历史注入
    let ctx = build_command_env(state, session_id, host_id, user_input).await;
    // 2. 脱敏（云端 provider 由 provider.is_local 决定；此处统一先脱敏 IP）
    let user_input_safe = context::redact_for_provider(user_input, false, true);
    let messages = prompt::build_command_gen_prompt(&ctx, "");

    let state2 = state.clone();
    let app2 = app.clone();
    let session_id2 = session_id.to_string();
    let user_input2 = user_input.to_string();

    tokio::spawn(async move {
        let result = stream_scene(&state2, "command_gen", messages, |delta| {
            let _ = app2.emit(
                format!("{event}_chunk"),
                serde_json::json!({ "chunk": delta }),
            );
        })
        .await;

        match result {
            Ok(raw) => {
                // 3. Schema 校验（F3.4：解析失败重试 1 次 + 纠错指令）
                let mut parsed = prompt::parse_command_result(&raw);
                if parsed.is_err() {
                    let retry_msgs = vec![
                        ai_ssh_core::prompt::ChatMessage::user(format!(
                            "上一条输出不是合法 JSON。请严格只输出符合 Schema 的 JSON。原始输出：\n{raw}"
                        )),
                    ];
                    if let Ok(retried) = stream_scene(&state2, "command_gen", retry_msgs, |_| {}).await {
                        parsed = prompt::parse_command_result(&retried);
                    }
                }
                match parsed {
                    Ok(cmd_result) => {
                        // 4. 服务端安全标注（AI 标注与规则冲突取更高等级，F6.1）
                        let verdict = ai_ssh_core::safety::safety_check(&cmd_result.commands.join("\n"), Some(cmd_result.risk));
                        let final_result = CommandResult {
                            risk: verdict.level,
                            risk_reason: if verdict.reasons.is_empty() { cmd_result.risk_reason.clone() } else { format!("{}（规则: {}）", cmd_result.risk_reason, verdict.reasons.join("; ")) },
                            ..cmd_result
                        };
                        let _ = app2.emit(
                            format!("{event}_done"),
                            serde_json::json!({ "result": serde_json::to_string(&final_result).unwrap_or_default() }),
                        );
                        // 审计：记录生成（未执行前）
                        if let Ok(conn) = state2.conn() {
                            let _ = conn.execute(
                                "INSERT INTO audit_log (created_at, host_id, user_input, generated_cmd, risk_level, executed)
                                 VALUES (datetime('now'), ?1, ?2, ?3, ?4, 0)",
                                params![host_id, user_input2, serde_json::to_string(&final_result.commands).unwrap_or_default(), serde_json::to_string(&final_result.risk).unwrap_or_default()],
                            );
                        }
                    }
                    Err(e) => {
                        let _ = app2.emit(format!("{event}_error"), serde_json::json!({ "message": e.to_string() }));
                    }
                }
            }
            Err(e) => {
                let _ = app2.emit(format!("{event}_error"), serde_json::json!({ "message": e.to_string() }));
            }
        }
    });

    Ok(())
}

/// llm_analyze_error：F4.3 报错诊断（报错片段 + 历史 Top3 + 资源快照）。
pub async fn analyze_error(state: &Arc<AppState>, app: &AppHandle, session_id: &str, error_text: &str) -> Result<()> {
    let event = "llm_analyze_error";
    let s = state
        .sessions
        .lock()
        .unwrap()
        .get(session_id)
        .cloned()
        .ok_or_else(|| Error::NotFound(format!("会话 {session_id}")))?;
    let host_id = s.host_id;

    let os_info = ssh::exec(state, session_id, "uname -sr 2>/dev/null || echo unknown").await.map(|(o, _)| o.trim().to_string()).unwrap_or_else(|_| "unknown".into());
    let resource_summary = met::resource_summary(state, host_id).await;

    let mut memory_hits: Vec<String> = Vec::new();
    if let Ok(conn) = state.conn() {
        if let Ok(hits) = memory::search_cases(&conn, host_id, error_text, 3) {
            memory_hits = hits
                .iter()
                .map(|c| format!("[id={}] {} | 根因: {} | 方案: {}", c.id.unwrap_or(0), c.description, c.root_cause.clone().unwrap_or_default(), c.solution_cmd.clone().unwrap_or_default()))
                .collect();
        }
    }

    let error_safe = context::redact_for_provider(error_text, false, true);
    let messages = prompt::build_error_diagnosis_prompt(&error_safe, &os_info, &resource_summary, memory_hits, "请分析该报错", "");

    spawn_diagnosis_stream(state.clone(), app.clone(), event.to_string(), host_id, messages, session_id.to_string());
    Ok(())
}

/// llm_analyze_alert：F8.6 告警联动 AI 分析（附带进程 Top10 与 30 分钟趋势）。
pub async fn analyze_alert(state: &Arc<AppState>, app: &AppHandle, host_id: i64, alert_id: i64) -> Result<()> {
    let event = "llm_analyze_alert";
    let conn = state.conn()?;

    // 告警信息
    let (alert_type, value, threshold, triggered_at) = conn
        .query_row(
            "SELECT alert_type, value, threshold, triggered_at FROM metrics_alerts WHERE id=?1",
            params![alert_id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?, r.get::<_, f64>(2)?, r.get::<_, String>(3)?)),
        )
        .map_err(|_| Error::NotFound(format!("告警 {alert_id}")))?;

    let snap = met::get_snapshot(state, host_id).unwrap_or_default();
    let overview = format!(
        "CPU {:.1}% | 内存 {:.1}% ({}MB/{}MB) | Swap {:.1}% | 负载 {:.2}",
        snap.cpu.total_pct,
        snap.memory.pct,
        snap.memory.used_mb,
        snap.memory.total_mb,
        snap.memory.swap_pct,
        snap.load.load1
    );
    let process_top = snap
        .processes
        .by_cpu
        .iter()
        .take(10)
        .map(|p| format!("PID {} {} CPU {:.1}% MEM {:.1}% {}", p.pid, p.user, p.cpu_pct, p.mem_pct, p.command))
        .collect::<Vec<_>>()
        .join("\n");

    let trend_30min = met::trend_summary(state, host_id).await;

    let mut memory_hits: Vec<String> = Vec::new();
    if let Ok(c) = state.conn() {
        if let Ok(hits) = memory::search_cases(&c, host_id, &format!("{alert_type} 高"), 3) {
            memory_hits = hits
                .iter()
                .map(|x| format!("[id={}] {}", x.id.unwrap_or(0), x.description))
                .collect();
        }
    }

    let ctx = AlertAnalysisCtx {
        alert_type: alert_type.clone(),
        value,
        threshold,
        duration: format!("自 {triggered_at}"),
        overview,
        process_top,
        trend_30min,
        memory_hits,
    };
    let messages = prompt::build_alert_analysis_prompt(&ctx, "");

    spawn_diagnosis_stream(state.clone(), app.clone(), event.to_string(), host_id, messages, format!("alert{host_id}"));
    Ok(())
}

fn spawn_diagnosis_stream(
    state: Arc<AppState>,
    app: AppHandle,
    event: String,
    host_id: i64,
    messages: Vec<ai_ssh_core::prompt::ChatMessage>,
    _session_id: String,
) {
    tokio::spawn(async move {
        let result = stream_scene(&state, "error_analysis", messages, |delta| {
            let _ = app.emit(format!("{event}_chunk"), serde_json::json!({ "chunk": delta }));
        })
        .await;
        match result {
            Ok(raw) => {
                let mut parsed = prompt::parse_diagnosis_result(&raw);
                if parsed.is_err() {
                    if let Ok(retried) = stream_scene(
                        &state,
                        "error_analysis",
                        vec![ai_ssh_core::prompt::ChatMessage::user(format!(
                            "上一条输出不是合法 JSON。请严格只输出符合 Schema 的 JSON。原始输出：\n{raw}"
                        ))],
                        |_| {},
                    )
                    .await
                    {
                        parsed = prompt::parse_diagnosis_result(&retried);
                    }
                }
                match parsed {
                    Ok(d) => {
                        let _ = app.emit(format!("{event}_done"), serde_json::json!({ "result": serde_json::to_string(&d).unwrap_or_default() }));
                    }
                    Err(e) => {
                        let _ = app.emit(format!("{event}_error"), serde_json::json!({ "message": e.to_string() }));
                    }
                }
            }
            Err(e) => {
                let _ = app.emit(format!("{event}_error"), serde_json::json!({ "message": e.to_string() }));
            }
        }
    });
}

/// llm_test_provider：F2.1 连通性测试。
pub async fn test_provider(state: &Arc<AppState>, provider_id: i64) -> Result<(bool, u128, String)> {
    let conn = state.conn()?;
    let provider = ai_ssh_core::llm::get_provider(&conn, provider_id)?;
    match ai_ssh_core::llm::test_provider(&provider).await {
        Ok((ms, reply)) => Ok((true, ms, reply)),
        Err(e) => Ok((false, 0, e.to_string())),
    }
}

/// llm_list_providers：返回全部 Provider（api_key 脱敏）。
pub fn list_providers(state: &Arc<AppState>) -> Result<Vec<serde_json::Value>> {
    let conn = state.conn()?;
    let mut stmt = conn.prepare(
        "SELECT id, name, protocol, base_url, api_key_encrypted, model_name, is_local, temperature, max_tokens, extra_system_prompt, enabled
         FROM llm_providers ORDER BY id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, Option<Vec<u8>>>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, i64>(6)?,
            r.get::<_, f64>(7)?,
            r.get::<_, i64>(8)?,
            r.get::<_, String>(9)?,
            r.get::<_, i64>(10)?,
        ))
    })?;
    let mut out = Vec::new();
    for r in rows {
        let (id, name, protocol, base_url, secret, model_name, is_local, temperature, max_tokens, extra, enabled) = r?;
        out.push(serde_json::json!({
            "id": id, "name": name, "protocol": protocol, "base_url": base_url,
            "api_key": if secret.is_some() { "••••" } else { "" },
            "model_name": model_name, "is_local": is_local != 0,
            "temperature": temperature, "max_tokens": max_tokens,
            "extra_system_prompt": extra, "enabled": enabled != 0,
        }));
    }
    Ok(out)
}

/// llm_save_provider：新增/更新 Provider（api_key 加密存储）。
pub fn save_provider(state: &Arc<AppState>, provider: ai_ssh_core::schema::ProviderConfig, id: Option<i64>) -> Result<i64> {
    let conn = state.conn()?;
    let api_key_enc = provider
        .api_key
        .as_deref()
        .filter(|k| !k.is_empty() && *k != "••••")
        .map(|k| {
            let key = ai_ssh_core::crypto::derive_key(&ai_ssh_core::crypto::device_fingerprint());
            ai_ssh_core::crypto::encrypt(k.as_bytes(), &key).unwrap_or_default()
        });

    let protocol = match provider.protocol {
        ai_ssh_core::schema::LlmProtocol::OpenaiCompatible => "openai_compatible",
        ai_ssh_core::schema::LlmProtocol::Anthropic => "anthropic",
        ai_ssh_core::schema::LlmProtocol::Ollama => "ollama",
    };

    if let Some(id) = id {
        // 更新；api_key 留空保持原值
        let cur: Option<Vec<u8>> = if api_key_enc.is_none() {
            conn.query_row("SELECT api_key_encrypted FROM llm_providers WHERE id=?1", params![id], |r| r.get(0))
                .unwrap_or(None)
        } else {
            api_key_enc
        };
        conn.execute(
            "UPDATE llm_providers SET name=?1, protocol=?2, base_url=?3, api_key_encrypted=?4, model_name=?5,
                    is_local=?6, temperature=?7, max_tokens=?8, extra_system_prompt=?9, enabled=?10
             WHERE id=?11",
            params![
                provider.name, protocol, provider.base_url, cur, provider.model_name,
                provider.is_local as i64, provider.temperature, provider.max_tokens as i64,
                provider.extra_system_prompt, provider.enabled as i64, id
            ],
        )?;
        Ok(id)
    } else {
        conn.execute(
            "INSERT INTO llm_providers (name, protocol, base_url, api_key_encrypted, model_name, is_local, temperature, max_tokens, extra_system_prompt, enabled)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                provider.name, protocol, provider.base_url, api_key_enc, provider.model_name,
                provider.is_local as i64, provider.temperature, provider.max_tokens as i64,
                provider.extra_system_prompt, provider.enabled as i64
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }
}

// ===== 记忆检索（F5） =====

pub fn memory_search(state: &Arc<AppState>, host_id: Option<i64>, query: &str) -> Result<Vec<ai_ssh_core::schema::MemoryCase>> {
    let conn = state.conn()?;
    match host_id {
        Some(h) => memory::search_cases(&conn, h, query, 3),
        None => {
            let f = CaseFilter { search: Some(query.to_string()), limit: 10, ..Default::default() };
            memory::list_cases(&conn, &f)
        }
    }
}

pub fn memory_save(state: &Arc<AppState>, case: ai_ssh_core::schema::MemoryCase) -> Result<i64> {
    let conn = state.conn()?;
    memory::save_case(&conn, &case)
}

pub fn memory_list(state: &Arc<AppState>, f: CaseFilter) -> Result<Vec<ai_ssh_core::schema::MemoryCase>> {
    let conn = state.conn()?;
    memory::list_cases(&conn, &f)
}

pub fn memory_update(state: &Arc<AppState>, id: i64, patch: ai_ssh_core::schema::MemoryCase) -> Result<()> {
    let conn = state.conn()?;
    let mut c = memory::get_case(&conn, id)?;
    // 仅覆盖传入的非空字段（前端整对象回传）
    c.description = patch.description;
    c.host_id = patch.host_id;
    c.problem_type = patch.problem_type;
    c.os_info = patch.os_info;
    c.error_snippet = patch.error_snippet;
    c.keywords = patch.keywords;
    c.root_cause = patch.root_cause;
    c.solution_cmd = patch.solution_cmd;
    c.solution_text = patch.solution_text;
    c.verify_cmd = patch.verify_cmd;
    c.rollback_cmd = patch.rollback_cmd;
    c.verified = patch.verified;
    c.failed_for = patch.failed_for;
    memory::save_case(&conn, &c)?;
    Ok(())
}

pub fn memory_delete(state: &Arc<AppState>, id: i64) -> Result<()> {
    let conn = state.conn()?;
    memory::delete_case(&conn, id)
}

pub fn memory_feedback(state: &Arc<AppState>, id: i64, up: bool) -> Result<()> {
    let conn = state.conn()?;
    memory::feedback(&conn, id, up)
}

pub fn memory_import_markdown(state: &Arc<AppState>, text: &str) -> Result<i64> {
    let conn = state.conn()?;
    memory::import_markdown(&conn, text)
}

pub fn memory_export_markdown(state: &Arc<AppState>, id: i64) -> Result<String> {
    let conn = state.conn()?;
    let c = memory::get_case(&conn, id)?;
    Ok(memory::export_markdown(&c))
}
