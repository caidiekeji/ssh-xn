// F5.4 自动沉淀：AI 修复命令执行后跑验证命令，exit 0 → 写入 memory_cases（verified=1）
use std::sync::Arc;

use serde::Serialize;

use ai_ssh_core::memory;
use ai_ssh_core::schema::{CaseSource, MemoryCase};
use ai_ssh_core::Result;

use crate::state::AppState;
use crate::ssh;

#[derive(serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AutoSaveInput {
    pub description: String,
    pub error_snippet: Option<String>,
    pub solution_cmd: String,
    pub verify_cmd: Option<String>,
    pub rollback_cmd: Option<String>,
    pub root_cause: Option<String>,
    pub keywords: Option<String>,
}

#[derive(Serialize)]
pub struct AutoSaveResult {
    pub verified: bool,
    pub saved: bool,
    pub id: Option<i64>,
}

/// 执行验证命令；exit 0 则保存记录（F5.4 触发条件第 1 行）。
pub async fn auto_save(
    state: &Arc<AppState>,
    session_id: &str,
    host_id: i64,
    input: AutoSaveInput,
) -> Result<AutoSaveResult> {
    let s = state
        .sessions
        .lock()
        .unwrap()
        .get(session_id)
        .cloned()
        .ok_or_else(|| ai_ssh_core::Error::NotFound(format!("会话 {session_id}")))?;

    let mut verified = false;
    if let Some(vcmd) = input.verify_cmd.as_deref().filter(|v| !v.trim().is_empty()) {
        if let Ok((_, exit)) = ssh::exec(state, session_id, vcmd).await {
            verified = exit == Some(0);
        }
    }

    if verified || !input.solution_cmd.trim().is_empty() {
        let conn = state.conn()?;
        let os_info = ssh::exec(state, session_id, "uname -sr 2>/dev/null || echo unknown")
            .await
            .map(|(o, _)| o.trim().to_string())
            .unwrap_or_default();

        let case = MemoryCase {
            id: None,
            created_at: None,
            updated_at: None,
            host_id: Some(host_id),
            os_info: Some(os_info),
            problem_type: None,
            error_snippet: input.error_snippet.clone(),
            description: input.description,
            keywords: input.keywords,
            root_cause: input.root_cause,
            solution_cmd: Some(input.solution_cmd),
            solution_text: None,
            verify_cmd: input.verify_cmd,
            rollback_cmd: input.rollback_cmd,
            hit_count: 0,
            verified,
            source: CaseSource::AiResolved,
            failed_for: None,
        };
        let id = memory::save_case(&conn, &case)?;
        Ok(AutoSaveResult { verified, saved: true, id: Some(id) })
    } else {
        Ok(AutoSaveResult { verified: false, saved: false, id: None })
    }
}
