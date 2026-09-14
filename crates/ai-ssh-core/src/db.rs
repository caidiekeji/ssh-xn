//! 数据库初始化：按 PRD 第 7 节 Schema 建表 + 常用索引。
//! 所有本地数据在单一数据目录（N9），本模块负责把 schema 落库。

use rusqlite::Connection;

use crate::error::Result;

/// 应用启动时执行建表；幂等（IF NOT EXISTS）。
pub fn init_db(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
PRAGMA journal_mode=WAL;
PRAGMA foreign_keys=ON;

CREATE TABLE IF NOT EXISTS hosts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    host TEXT NOT NULL,
    port INTEGER DEFAULT 22,
    username TEXT NOT NULL,
    auth_type TEXT NOT NULL CHECK(auth_type IN ('password','private_key')),
    secret_encrypted BLOB,
    key_path TEXT,
    passphrase_encrypted BLOB,
    jump_host_id INTEGER REFERENCES hosts(id),
    group_name TEXT,
    tags TEXT,
    notes TEXT,
    memory_enabled INTEGER DEFAULT 1,
    monitor_enabled INTEGER DEFAULT 1,
    created_at DATETIME, updated_at DATETIME
);

CREATE TABLE IF NOT EXISTS llm_providers (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    protocol TEXT NOT NULL CHECK(protocol IN ('openai_compatible','anthropic','ollama')),
    base_url TEXT NOT NULL,
    api_key_encrypted BLOB,
    model_name TEXT NOT NULL,
    is_local INTEGER DEFAULT 0,
    temperature REAL DEFAULT 0.2,
    max_tokens INTEGER DEFAULT 2048,
    extra_system_prompt TEXT DEFAULT '',
    enabled INTEGER DEFAULT 1
);

CREATE TABLE IF NOT EXISTS llm_routing (
    scene TEXT PRIMARY KEY CHECK(scene IN ('command_gen','error_analysis','metrics_analysis')),
    provider_id INTEGER REFERENCES llm_providers(id),
    fallback_ids TEXT
);

CREATE TABLE IF NOT EXISTS memory_cases (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at DATETIME, updated_at DATETIME,
    host_id INTEGER REFERENCES hosts(id),
    os_info TEXT,
    problem_type TEXT,
    error_snippet TEXT,
    description TEXT NOT NULL,
    keywords TEXT,
    root_cause TEXT,
    solution_cmd TEXT,
    solution_text TEXT,
    verify_cmd TEXT,
    rollback_cmd TEXT,
    hit_count INTEGER DEFAULT 0,
    verified INTEGER DEFAULT 0,
    source TEXT CHECK(source IN ('ai_resolved','user_manual','imported')),
    failed_for TEXT
);

CREATE TABLE IF NOT EXISTS session_context (
    session_id TEXT NOT NULL,
    seq INTEGER NOT NULL,
    role TEXT NOT NULL CHECK(role IN ('user','terminal','assistant')),
    content TEXT NOT NULL,
    created_at DATETIME,
    PRIMARY KEY (session_id, seq)
);

CREATE TABLE IF NOT EXISTS command_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT,
    host_id INTEGER,
    command TEXT NOT NULL,
    user_input TEXT,
    risk_level TEXT,
    executed INTEGER,
    exit_code INTEGER,
    created_at DATETIME
);

CREATE TABLE IF NOT EXISTS metrics_history (
    host_id INTEGER NOT NULL REFERENCES hosts(id),
    granularity TEXT NOT NULL CHECK(granularity IN ('1min','1hour')),
    bucket_start DATETIME NOT NULL,
    cpu_user_avg REAL, cpu_user_max REAL,
    cpu_sys_avg REAL, cpu_sys_max REAL,
    cpu_iowait_avg REAL, cpu_iowait_max REAL,
    load1_avg REAL, load1_max REAL,
    mem_pct_avg REAL, mem_pct_max REAL,
    swap_pct_avg REAL, swap_pct_max REAL,
    disk_max_pct_avg REAL, disk_max_pct_max REAL,
    net_rx_avg REAL, net_tx_avg REAL,
    PRIMARY KEY (host_id, granularity, bucket_start)
);

CREATE TABLE IF NOT EXISTS metrics_alerts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    host_id INTEGER NOT NULL,
    alert_type TEXT NOT NULL CHECK(alert_type IN ('cpu','memory','swap','disk')),
    value REAL NOT NULL,
    threshold REAL NOT NULL,
    triggered_at DATETIME NOT NULL,
    recovered_at DATETIME,
    muted INTEGER DEFAULT 0,
    muted_until DATETIME,
    analysis_log TEXT
);

CREATE TABLE IF NOT EXISTS audit_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at DATETIME,
    host_id INTEGER,
    user_input TEXT,
    generated_cmd TEXT,
    risk_level TEXT,
    executed INTEGER,
    result_summary TEXT,
    memory_ids_used TEXT
);

CREATE TABLE IF NOT EXISTS token_usage (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    date TEXT, provider_id INTEGER, scene TEXT,
    prompt_tokens INTEGER, completion_tokens INTEGER
);

CREATE TABLE IF NOT EXISTS monitor_settings (
    id INTEGER PRIMARY KEY CHECK(id = 1),
    interval_sec INTEGER DEFAULT 3,
    cpu_alert REAL DEFAULT 90,
    mem_alert REAL DEFAULT 90,
    swap_alert REAL DEFAULT 80,
    disk_alert REAL DEFAULT 95,
    notification_mode TEXT DEFAULT 'app'
);

CREATE INDEX IF NOT EXISTS idx_memory_host ON memory_cases(host_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_memory_desc ON memory_cases(description);
CREATE INDEX IF NOT EXISTS idx_metrics_host ON metrics_history(host_id, granularity, bucket_start);
CREATE INDEX IF NOT EXISTS idx_alerts_host ON metrics_alerts(host_id, triggered_at DESC);
CREATE INDEX IF NOT EXISTS idx_audit_time ON audit_log(created_at);
CREATE INDEX IF NOT EXISTS idx_cmdhist_time ON command_history(created_at);
"#,
    )?;
    Ok(())
}

/// 数据清理：删除超保留期数据（PRD 第 7 节末尾）。
/// 1min 保留 7 天；1hour 保留 90 天；已恢复超过 90 天的告警删除。
pub fn cleanup_expired(conn: &Connection) -> Result<()> {
    conn.execute(
        "DELETE FROM metrics_history WHERE granularity='1min' AND bucket_start < datetime('now','-7 days')",
        [],
    )?;
    conn.execute(
        "DELETE FROM metrics_history WHERE granularity='1hour' AND bucket_start < datetime('now','-90 days')",
        [],
    )?;
    conn.execute(
        "DELETE FROM metrics_alerts WHERE recovered_at IS NOT NULL AND recovered_at < datetime('now','-90 days')",
        [],
    )?;
    Ok(())
}

/// 读取全局监控设置（无记录则插入默认）。
pub fn get_monitor_settings(conn: &Connection) -> Result<MonitorSettings> {
    let row = conn.query_row(
        "SELECT interval_sec, cpu_alert, mem_alert, swap_alert, disk_alert, notification_mode
         FROM monitor_settings WHERE id=1",
        [],
        |r| {
            Ok(MonitorSettings {
                interval_sec: r.get(0)?,
                mem_alert: r.get(1)?,
                cpu_alert: r.get(2)?,
                swap_alert: r.get(3)?,
                disk_alert: r.get(4)?,
                notification_mode: r.get(5)?,
            })
        },
    );
    match row {
        Ok(s) => Ok(s),
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            let s = MonitorSettings::default();
            conn.execute(
                "INSERT INTO monitor_settings (id, interval_sec, cpu_alert, mem_alert, swap_alert, disk_alert, notification_mode)
                 VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![s.interval_sec, s.cpu_alert, s.mem_alert, s.swap_alert, s.disk_alert, s.notification_mode],
            )?;
            Ok(s)
        }
        Err(e) => Err(e.into()),
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MonitorSettings {
    pub interval_sec: i64,
    pub cpu_alert: f64,
    pub mem_alert: f64,
    pub swap_alert: f64,
    pub disk_alert: f64,
    pub notification_mode: String,
}

impl Default for MonitorSettings {
    fn default() -> Self {
        Self {
            interval_sec: 3,
            cpu_alert: 90.0,
            mem_alert: 90.0,
            swap_alert: 80.0,
            disk_alert: 95.0,
            notification_mode: "app".into(),
        }
    }
}

pub fn update_monitor_settings(conn: &Connection, s: &MonitorSettings) -> Result<()> {
    conn.execute(
        "INSERT INTO monitor_settings (id, interval_sec, cpu_alert, mem_alert, swap_alert, disk_alert, notification_mode)
         VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(id) DO UPDATE SET
           interval_sec=excluded.interval_sec, cpu_alert=excluded.cpu_alert,
           mem_alert=excluded.mem_alert, swap_alert=excluded.swap_alert,
           disk_alert=excluded.disk_alert, notification_mode=excluded.notification_mode",
        rusqlite::params![s.interval_sec, s.cpu_alert, s.mem_alert, s.swap_alert, s.disk_alert, s.notification_mode],
    )?;
    Ok(())
}
