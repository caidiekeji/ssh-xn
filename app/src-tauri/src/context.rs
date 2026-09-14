// 上下文引擎（模块C）：会话上下文采集（session_context 落库）、脱敏、报错检测（F4 规则层）
use std::sync::Mutex;

use rusqlite::params;
use tauri::Emitter;

use ai_ssh_core::desensitize::{extract_error_context, redact, tail_lines, RedactOptions};
use ai_ssh_core::Result;

use crate::state::AppState;

/// F4.1：记录用户命令 / 终端输出到 session_context（带时间戳，截断超长内容）。
pub fn record_context(conn: &rusqlite::Connection, session_id: &str, role: &str, content: &str) -> Result<()> {
    let content = tail_lines(content, 200);
    conn.execute(
        "INSERT INTO session_context (session_id, seq, role, content, created_at)
         VALUES (?1, (SELECT COALESCE(MAX(seq),0)+1 FROM session_context WHERE session_id=?1), ?2, ?3, datetime('now'))",
        params![session_id, role, content],
    )?;
    // 实现注意事项 #5：超过 30 条时截断最早一半
    conn.execute(
        "DELETE FROM session_context WHERE session_id=?1 AND seq IN (
            SELECT seq FROM session_context WHERE session_id=?1 ORDER BY seq LIMIT MAX(0, (SELECT COUNT(*) FROM session_context WHERE session_id=?1) - 30)
         )",
        params![session_id],
    )?;
    Ok(())
}

/// 取最近 N 条会话上下文（用于 Prompt 组装）。
pub fn recent_context(conn: &rusqlite::Connection, session_id: &str, limit: usize) -> Vec<String> {
    let mut stmt = match conn.prepare(
        "SELECT role, content FROM session_context WHERE session_id=?1 ORDER BY seq DESC LIMIT ?2",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let rows = match stmt.query_map(params![session_id, limit as i64], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    let mut out: Vec<String> = Vec::new();
    for r in rows {
        if let Ok((role, content)) = r {
            let prefix = match role.as_str() {
                "user" => "$",
                "assistant" => "#",
                _ => ">",
            };
            out.push(format!("{prefix} {content}"));
        }
    }
    out.reverse();
    out
}

/// 双层报错检测的规则层（F4.2）：命中即返回提取的报错上下文。
const ERROR_PATTERNS: &[&str] = &[
    "error",
    "failed",
    "fatal",
    "panic",
    "exception",
    "denied",
    "refused",
    "traceback",
    "segmentation fault",
    "not found",
    "no such file",
    "command not found",
    "permission denied",
    "connection refused",
    "disk full",
    "out of memory",
];

/// 对一段新增输出做报错检测；命中返回上下文片段（向上 50 行）。
pub fn detect_error(output: &str) -> Option<String> {
    let lower = output.to_lowercase();
    let mut hit_line = None;
    for (i, line) in lower.lines().enumerate() {
        if ERROR_PATTERNS.iter().any(|p| line.contains(p)) {
            hit_line = Some(i);
            break;
        }
    }
    hit_line.map(|l| extract_error_context(output, l, 50))
}

/// 在数据回调中调用：脱敏记录 + 报错检测 + 事件推送（防抖 5s）。
pub fn on_terminal_output(state: &AppState, app: &tauri::AppHandle, session_id: &str, raw: &str) {
    // 记录上下文（F4.1）
    if let Ok(conn) = state.conn() {
        let _ = record_context(&conn, session_id, "terminal", raw);
    }

    // 报错检测（规则层）→ 提示条事件；防抖避免刷屏
    if let Some(snippet) = detect_error(raw) {
        let now = chrono::Utc::now().timestamp();
        let mut map = state.last_error_emit.lock().unwrap();
        let last = map.get(session_id).copied().unwrap_or(0);
        if now - last >= 5 {
            map.insert(session_id.to_string(), now);
            let _ = app.emit("error_detected", serde_json::json!({ "session_id": session_id, "snippet": snippet }));
        }
    }
}

/// 发送 LLM 前统一脱敏（F6.3）：云端 provider 必脱敏；本地 provider 默认不脱敏。
pub fn redact_for_provider(text: &str, is_local: bool, mask_ip: bool) -> String {
    redact(
        text,
        &RedactOptions {
            mask_ip,
            // 本地 trusted 由 is_local 决定；is_local=true 且不强制时由调用方决定
            local_trusted: is_local,
        },
    )
}

/// 会话上下文清理：删除 7 天前上下文。
pub fn cleanup_old_context(conn: &rusqlite::Connection) -> Result<()> {
    conn.execute("DELETE FROM session_context WHERE created_at < datetime('now','-7 days')", [])?;
    Ok(())
}
