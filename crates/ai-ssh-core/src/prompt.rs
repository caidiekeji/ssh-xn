//! Prompt 拼装与 LLM 输出 Schema 校验（PRD 第 6 节）。
//! 6.1 命令生成 Schema / 6.2 错误诊断 Schema / 6.3-6.4 Prompt 模板。

use crate::error::{Error, Result};
use crate::schema::{CommandResult, DiagnosisResult};

/// 对话消息（LLM 侧统一结构）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self { role: "system".into(), content: content.into() }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self { role: "user".into(), content: content.into() }
    }
}

/// 命令生成场景的环境上下文（F3.2 第 2 步）。
#[derive(Debug, Clone, Default)]
pub struct CommandEnvCtx {
    pub host: String,
    pub os_info: String,
    pub cwd: String,
    pub cpu_pct: Option<f64>,
    pub mem_used: Option<u64>,
    pub mem_total: Option<u64>,
    pub mem_pct: Option<f64>,
    pub loadavg: Option<String>,
    pub disk_max_pct: Option<f64>,
    pub recent_commands: Vec<String>,
    /// 已格式化的历史记录（含 id/问题描述/根因/解决方案）
    pub memory_hits: Vec<String>,
    pub user_input: String,
}

impl CommandEnvCtx {
    pub fn resource_line(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(c) = self.cpu_pct {
            parts.push(format!("CPU: {c:.1}%"));
        }
        if let (Some(u), Some(t), Some(p)) = (self.mem_used, self.mem_total, self.mem_pct) {
            parts.push(format!("内存: {u}MB/{t}MB({p:.1}%)"));
        }
        if let Some(l) = &self.loadavg {
            parts.push(format!("负载: {l}"));
        }
        if let Some(d) = self.disk_max_pct {
            parts.push(format!("磁盘最高使用率: {d:.1}%"));
        }
        if parts.is_empty() {
            "（监控未开启或暂不可用）".to_string()
        } else {
            parts.join("，")
        }
    }
}

/// 6.3 命令生成 Prompt 模板。
pub fn build_command_gen_prompt(ctx: &CommandEnvCtx, extra_system: &str) -> Vec<ChatMessage> {
    let system = format!(
        r#"你是资深 Linux 运维专家，运行在 SSH 终端工具中。
【当前环境】主机: {host}，系统: {os_info}，当前路径: {cwd}
【实时资源】{resource}
【最近命令】
{recent}
【历史相似问题】（来自本地记录，供参考）：
{memory}
{extra}
要求：
1. 仅输出符合以下 JSON Schema 的 JSON，无其他任何文本
2. 命令必须适配当前系统
3. 若历史记录中有适用方案，优先复用并在对应字段注明
4. 诚实评估风险等级
Schema:
{{"commands": ["string[]", "按顺序执行的命令列表"], "explanation": ["string[]", "与 commands 一一对应的逐条解释，中文"], "risk": "low | medium | high", "risk_reason": "string，risk 非 low 时必填", "notes": "string，可选，执行前提醒"}}"#,
        host = ctx.host,
        os_info = ctx.os_info,
        cwd = ctx.cwd,
        resource = ctx.resource_line(),
        recent = if ctx.recent_commands.is_empty() {
            "（暂无）".to_string()
        } else {
            ctx.recent_commands.join("\n")
        },
        memory = if ctx.memory_hits.is_empty() {
            "（无）".to_string()
        } else {
            ctx.memory_hits.join("\n---\n")
        },
        extra = extra_system,
    );
    vec![
        ChatMessage::system(system),
        ChatMessage::user(format!("【用户请求】{}", ctx.user_input)),
    ]
}

/// 6.4 资源异常分析 Prompt 模板（告警联动 AI 分析，F8.6）。
#[derive(Debug, Clone)]
pub struct AlertAnalysisCtx {
    pub alert_type: String,
    pub value: f64,
    pub threshold: f64,
    pub duration: String,
    pub overview: String,
    pub process_top: String,
    pub trend_30min: String,
    pub memory_hits: Vec<String>,
}

pub fn build_alert_analysis_prompt(ctx: &AlertAnalysisCtx, extra_system: &str) -> Vec<ChatMessage> {
    let system = format!(
        r#"你是资深 Linux 性能诊断专家。
【告警】类型: {alert_type}，当前值: {value}，阈值: {threshold}，持续: {duration}
【实时数据】{overview}
【进程 Top】
{process_top}
【30分钟趋势】{trend_30min}
【历史相似问题】
{memory}
{extra}
要求：
1. 仅输出符合以下 JSON Schema 的 JSON，无其他任何文本
2. 先定位具体进程/服务，再给根因
3. fix_steps 优先给只读排查命令，确认根因后再给修复命令
Schema:
{{"diagnosis": "string，通俗解释发生了什么", "root_cause": "string，技术根因", "fix_steps": [{{"cmd": "string", "desc": "string，这步做什么"}}], "verify_cmd": "string，验证修复是否生效的命令", "rollback_cmd": "string，回滚命令，无法回滚则为空字符串", "ref_memory": ["string[]，引用的历史记录 ID，未引用则空数组"]}}"#,
        alert_type = ctx.alert_type,
        value = ctx.value,
        threshold = ctx.threshold,
        duration = ctx.duration,
        overview = ctx.overview,
        process_top = ctx.process_top,
        trend_30min = ctx.trend_30min,
        memory = if ctx.memory_hits.is_empty() {
            "（无）".to_string()
        } else {
            ctx.memory_hits.join("\n---\n")
        },
        extra = extra_system,
    );
    vec![
        ChatMessage::system(system),
        ChatMessage::user("【请求】请分析当前异常并给出诊断与处理步骤。"),
    ]
}

/// 错误诊断 Prompt（F4.3）：报错片段 + 历史 Top3 + 资源快照。
pub fn build_error_diagnosis_prompt(
    error_snippet: &str,
    os_info: &str,
    resource_summary: &str,
    memory_hits: Vec<String>,
    user_question: &str,
    extra_system: &str,
) -> Vec<ChatMessage> {
    let system = format!(
        r#"你是资深 Linux 运维专家，运行在 SSH 终端工具中。
【系统】{os_info}
【实时资源】{resource}
【历史相似问题】（来自本地记录，供参考）：
{memory}
{extra}
要求：
1. 仅输出符合以下 JSON Schema 的 JSON，无其他任何文本
2. 基于下方终端输出片段分析报错
3. 若引用了历史记录，必须在 ref_memory 中返回记录 ID
Schema:
{{"diagnosis": "string，通俗解释发生了什么", "root_cause": "string，技术根因", "fix_steps": [{{"cmd": "string", "desc": "string，这步做什么"}}], "verify_cmd": "string，验证修复是否生效的命令", "rollback_cmd": "string，回滚命令，无法回滚则为空字符串", "ref_memory": ["string[]，引用的历史记录 ID，未引用则空数组"]}}"#,
        os_info = os_info,
        resource = if resource_summary.is_empty() { "（未开启）" } else { resource_summary },
        memory = if memory_hits.is_empty() {
            "（无）".to_string()
        } else {
            memory_hits.join("\n---\n")
        },
        extra = extra_system,
    );
    let user = format!(
        "【终端输出片段】\n```\n{error_snippet}\n```\n【用户补充】{user_question}",
        error_snippet = error_snippet,
        user_question = user_question,
    );
    vec![ChatMessage::system(system), ChatMessage::user(user)]
}

// ============ Schema 校验 ============

/// 校验 6.1 Schema：LLM 输出必须合法 JSON 且结构完整。
/// 解析失败走重试（F3.4），禁止把原始文本当命令执行（实现注意事项 #2）。
pub fn parse_command_result(raw: &str) -> Result<CommandResult> {
    let v: serde_json::Value = serde_json::from_str(raw.trim())
        .map_err(|e| Error::Parse(format!("命令生成 JSON 解析失败: {e}")))?;
    let obj = v
        .as_object()
        .ok_or_else(|| Error::Parse("命令生成结果不是 JSON 对象".into()))?;

    let commands: Vec<String> = extract_string_array(obj, "commands")?;
    let explanation: Vec<String> = extract_string_array(obj, "explanation")?;
    if commands.is_empty() {
        return Err(Error::Parse("commands 为空".into()));
    }
    if explanation.len() != commands.len() {
        return Err(Error::Parse(format!(
            "explanation 数量({})与 commands 数量({})不一致",
            explanation.len(),
            commands.len()
        )));
    }
    let risk: crate::schema::RiskLevel = match obj.get("risk").and_then(|v| v.as_str()) {
        Some("low") => crate::schema::RiskLevel::Low,
        Some("medium") => crate::schema::RiskLevel::Medium,
        Some("high") => crate::schema::RiskLevel::High,
        other => {
            return Err(Error::Parse(format!("risk 字段非法: {other:?}")));
        }
    };
    if risk != crate::schema::RiskLevel::Low
        && obj.get("risk_reason").and_then(|v| v.as_str()).map(|s| s.trim().is_empty()).unwrap_or(true)
    {
        return Err(Error::Parse("risk 非 low 时 risk_reason 必填".into()));
    }
    Ok(CommandResult {
        commands,
        explanation,
        risk,
        risk_reason: obj.get("risk_reason").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        notes: obj.get("notes").and_then(|v| v.as_str()).unwrap_or("").to_string(),
    })
}

/// 校验 6.2 Schema。
pub fn parse_diagnosis_result(raw: &str) -> Result<DiagnosisResult> {
    let v: serde_json::Value = serde_json::from_str(raw.trim())
        .map_err(|e| Error::Parse(format!("诊断 JSON 解析失败: {e}")))?;
    let obj = v
        .as_object()
        .ok_or_else(|| Error::Parse("诊断结果不是 JSON 对象".into()))?;

    let fix_steps = match obj.get("fix_steps") {
        None => Vec::new(),
        Some(serde_json::Value::Array(arr)) => {
            let mut out = Vec::new();
            for item in arr {
                let o = item
                    .as_object()
                    .ok_or_else(|| Error::Parse("fix_steps 元素不是对象".into()))?;
                let cmd = o
                    .get("cmd")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::Parse("fix_steps.cmd 缺失".into()))?;
                let desc = o
                    .get("desc")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::Parse("fix_steps.desc 缺失".into()))?;
                out.push(crate::schema::FixStep { cmd: cmd.to_string(), desc: desc.to_string() });
            }
            out
        }
        Some(_) => return Err(Error::Parse("fix_steps 不是数组".into())),
    };

    Ok(DiagnosisResult {
        diagnosis: get_str(obj, "diagnosis")?.to_string(),
        root_cause: get_str(obj, "root_cause")?.to_string(),
        fix_steps,
        verify_cmd: obj.get("verify_cmd").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        rollback_cmd: obj.get("rollback_cmd").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        ref_memory: obj
            .get("ref_memory")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default(),
    })
}

fn extract_string_array(obj: &serde_json::Map<String, serde_json::Value>, key: &str) -> Result<Vec<String>> {
    let arr = obj
        .get(key)
        .and_then(|v| v.as_array())
        .ok_or_else(|| Error::Parse(format!("缺少 {key} 数组")))?;
    let mut out = Vec::new();
    for item in arr {
        let s = item
            .as_str()
            .ok_or_else(|| Error::Parse(format!("{key} 元素不是字符串")))?;
        out.push(s.to_string());
    }
    Ok(out)
}

fn get_str<'a>(obj: &'a serde_json::Map<String, serde_json::Value>, key: &str) -> Result<&'a str> {
    obj.get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::Parse(format!("缺少 {key} 字符串字段")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_command_json() {
        let raw = r#"{"commands":["ss -ltnp | grep :80"],"explanation":["查看 80 端口监听"],"risk":"low","notes":"无需 sudo"}"#;
        let r = parse_command_result(raw).unwrap();
        assert_eq!(r.commands.len(), 1);
        assert_eq!(r.risk, crate::schema::RiskLevel::Low);
    }

    #[test]
    fn parse_command_missing_explanation_fails() {
        let raw = r#"{"commands":["ls"],"explanation":[],"risk":"low"}"#;
        assert!(parse_command_result(raw).is_err());
    }

    #[test]
    fn parse_command_high_requires_reason() {
        let raw = r#"{"commands":["rm -rf /"],"explanation":["x"],"risk":"high"}"#;
        assert!(parse_command_result(raw).is_err());
        let raw2 = r#"{"commands":["rm -rf /"],"explanation":["x"],"risk":"high","risk_reason":"危险"}"#;
        assert!(parse_command_result(raw2).is_ok());
    }

    #[test]
    fn parse_invalid_json_fails() {
        assert!(parse_command_result("不是 JSON").is_err());
        assert!(parse_command_result("```json\n{\"a\":1}\n```").is_err());
    }

    #[test]
    fn parse_diagnosis() {
        let raw = r#"{"diagnosis":"连接被拒绝","root_cause":"服务未启动","fix_steps":[{"cmd":"systemctl status nginx","desc":"查看状态"}],"verify_cmd":"curl -I localhost","rollback_cmd":"","ref_memory":["3"]}"#;
        let d = parse_diagnosis_result(raw).unwrap();
        assert_eq!(d.fix_steps.len(), 1);
        assert_eq!(d.ref_memory, vec!["3".to_string()]);
    }

    #[test]
    fn prompt_contains_required_sections() {
        let mut ctx = CommandEnvCtx::default();
        ctx.host = "web-01".into();
        ctx.os_info = "Ubuntu 22.04".into();
        ctx.cwd = "/srv/app".into();
        ctx.user_input = "查看 80 端口占用".into();
        ctx.memory_hits.push("id=1 | 问题: 端口占用 | 解决: ss -ltnp".into());
        let msgs = build_command_gen_prompt(&ctx, "");
        assert!(msgs[0].content.contains("web-01"));
        assert!(msgs[0].content.contains("历史相似问题"));
        assert!(msgs[1].content.contains("查看 80 端口占用"));
    }
}
