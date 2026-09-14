// 安全与审计命令（F6）：safety_check（危险命令检测）、audit_export_csv
use std::sync::Arc;

use ai_ssh_core::audit;
use ai_ssh_core::safety::{self, SafetyVerdict};
use ai_ssh_core::schema::RiskLevel;
use ai_ssh_core::Result;

use crate::state::AppState;

/// safety_check：执行前强制检测（F6.1 / 实现注意事项 #1）。
pub fn safety_check(command: &str) -> SafetyVerdict {
    safety::safety_check(command, None)
}

/// 风险等级字符串。
pub fn risk_str(r: RiskLevel) -> &'static str {
    match r {
        RiskLevel::High => "high",
        RiskLevel::Medium => "medium",
        RiskLevel::Low => "low",
    }
}

/// 导出审计日志 CSV（F6.2）。
pub fn audit_export_csv(state: &Arc<AppState>) -> Result<String> {
    let conn = state.conn()?;
    audit::export_csv(&conn)
}
