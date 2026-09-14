//! 与 PRD 对齐的数据契约：LLM 输出 Schema（6.1/6.2）、监控快照（第 8 节）、
//! 历史记录条目（F5）、Provider 配置（F2）。

use serde::{Deserialize, Serialize};

// ============ 风险等级 ============

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RiskLevel {
    #[default]
    Low,
    Medium,
    High,
}

impl RiskLevel {
    /// AI 标注与规则引擎冲突时取更高等级（F6.1）。
    pub fn max(self, other: RiskLevel) -> RiskLevel {
        match (self as u8, other as u8) {
            (a, b) if a >= b => self,
            _ => other,
        }
    }
}

// ============ 6.1 命令生成 Schema ============

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandResult {
    /// 按顺序执行的命令列表
    pub commands: Vec<String>,
    /// 与 commands 一一对应的逐条解释，中文
    pub explanation: Vec<String>,
    /// 风险等级
    #[serde(default)]
    pub risk: RiskLevel,
    /// risk 非 low 时必填
    #[serde(default)]
    pub risk_reason: String,
    /// 可选，执行前提醒
    #[serde(default)]
    pub notes: String,
}

// ============ 6.2 错误诊断 Schema ============

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixStep {
    pub cmd: String,
    /// 这步做什么
    pub desc: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosisResult {
    /// 通俗解释发生了什么
    pub diagnosis: String,
    /// 技术根因
    pub root_cause: String,
    #[serde(default)]
    pub fix_steps: Vec<FixStep>,
    /// 验证修复是否生效的命令
    #[serde(default)]
    pub verify_cmd: String,
    /// 回滚命令，无法回滚则为空字符串
    #[serde(default)]
    pub rollback_cmd: String,
    /// 引用的历史记录 ID，未引用则空数组
    #[serde(default)]
    pub ref_memory: Vec<String>,
}

// ============ F5 历史记录 ============

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaseSource {
    AiResolved,
    UserManual,
    Imported,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryCase {
    pub id: Option<i64>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub host_id: Option<i64>,
    pub os_info: Option<String>,
    pub problem_type: Option<String>,
    pub error_snippet: Option<String>,
    pub description: String,
    pub keywords: Option<String>,
    pub root_cause: Option<String>,
    pub solution_cmd: Option<String>,
    pub solution_text: Option<String>,
    pub verify_cmd: Option<String>,
    pub rollback_cmd: Option<String>,
    pub hit_count: i64,
    pub verified: bool,
    pub source: CaseSource,
    pub failed_for: Option<String>,
}

impl Default for MemoryCase {
    fn default() -> Self {
        Self {
            id: None,
            created_at: None,
            updated_at: None,
            host_id: None,
            os_info: None,
            problem_type: None,
            error_snippet: None,
            description: String::new(),
            keywords: None,
            root_cause: None,
            solution_cmd: None,
            solution_text: None,
            verify_cmd: None,
            rollback_cmd: None,
            hit_count: 0,
            verified: false,
            source: CaseSource::UserManual,
            failed_for: None,
        }
    }
}

// ============ F2 Provider 配置 ============

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LlmProtocol {
    /// OpenAI 兼容 /v1/chat/completions（含 DeepSeek、GLM、Ollama）
    OpenaiCompatible,
    /// Anthropic /v1/messages
    Anthropic,
    /// Ollama /api/chat
    Ollama,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub id: Option<i64>,
    pub name: String,
    pub protocol: LlmProtocol,
    pub base_url: String,
    pub api_key: Option<String>,
    pub model_name: String,
    pub is_local: bool,
    pub temperature: f64,
    pub max_tokens: u32,
    pub extra_system_prompt: String,
    pub enabled: bool,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            id: None,
            name: String::new(),
            protocol: LlmProtocol::OpenaiCompatible,
            base_url: String::new(),
            api_key: None,
            model_name: String::new(),
            is_local: false,
            temperature: 0.2,
            max_tokens: 2048,
            extra_system_prompt: String::new(),
            enabled: true,
        }
    }
}

// ============ 第 8 节 MetricsSnapshot ============

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CpuSnapshot {
    pub total_pct: f64,
    pub user_pct: f64,
    pub sys_pct: f64,
    pub iowait_pct: f64,
    pub cores: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LoadSnapshot {
    pub load1: f64,
    pub load5: f64,
    pub load15: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MemorySnapshot {
    pub total_mb: u64,
    pub used_mb: u64,
    pub available_mb: u64,
    pub pct: f64,
    pub buffers_cached_mb: u64,
    pub swap_total_mb: u64,
    pub swap_used_mb: u64,
    pub swap_pct: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DiskSnapshot {
    pub mount: String,
    pub total_gb: f64,
    pub used_gb: f64,
    pub pct: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NetSnapshot {
    pub iface: String,
    pub rx_bps: f64,
    pub tx_bps: f64,
    pub rx_bytes_total: u64,
    pub tx_bytes_total: u64,
    pub errors: u64,
    pub dropped: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProcessSnapshot {
    pub pid: u32,
    pub user: String,
    pub cpu_pct: f64,
    pub mem_pct: f64,
    pub command: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProcessTable {
    pub by_cpu: Vec<ProcessSnapshot>,
    pub by_mem: Vec<ProcessSnapshot>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MetricsSnapshot {
    pub host_id: i64,
    pub timestamp: i64,
    pub cpu: CpuSnapshot,
    pub load: LoadSnapshot,
    pub memory: MemorySnapshot,
    pub disks: Vec<DiskSnapshot>,
    pub network: Vec<NetSnapshot>,
    pub processes: ProcessTable,
    pub uptime_sec: u64,
}

// ============ 告警 ============

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AlertType {
    Cpu,
    Memory,
    Swap,
    Disk,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertDecision {
    pub alert_type: AlertType,
    pub value: f64,
    pub threshold: f64,
    pub sustained_secs: i64,
}

// ============ 命令执行审计（F6.2） ============

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuditEntry {
    pub id: Option<i64>,
    pub created_at: Option<String>,
    pub host_id: Option<i64>,
    pub user_input: Option<String>,
    pub generated_cmd: String,
    pub risk_level: Option<String>,
    pub executed: bool,
    pub result_summary: Option<String>,
    pub memory_ids_used: Option<String>,
}
