// 应用状态：数据库连接（每命令短连接，便于多线程）、SSH 会话表、监控任务表
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use rusqlite::Connection;

use ai_ssh_core::metrics::{AlertEvaluator, MinuteAccumulator};
use ai_ssh_core::monitor::{CpuRaw, NetRaw};
use ai_ssh_core::schema::MetricsSnapshot;

// 会话定义统一在 ssh.rs（含 SSH handle/channel），此处直接复用
pub use crate::ssh::Session;

#[derive(Default)]
pub struct MonitorState {
    /// 上一次采样原始值（CPU / 网络），用于速率差值计算（F8.1）
    pub prev_cpu: Option<CpuRaw>,
    pub prev_net: Vec<(String, NetRaw)>,
    pub last_snapshot: Option<MetricsSnapshot>,
}

#[derive(Default)]
pub struct AppState {
    pub db_path: PathBuf,
    pub sessions: Mutex<HashMap<String, Session>>,
    pub monitor_tasks: Mutex<HashMap<i64, tokio::task::JoinHandle<()>>>,
    pub monitor: Mutex<HashMap<i64, MonitorState>>,
    pub minute_acc: Mutex<HashMap<i64, MinuteAccumulator>>,
    pub alert_eval: Mutex<AlertEvaluator>,
    /// 报错提示条防抖：session_id → 上次推送时间
    pub last_error_emit: Mutex<HashMap<String, i64>>,
}

impl AppState {
    pub fn new(db_path: PathBuf) -> Self {
        Self {
            db_path,
            ..Default::default()
        }
    }

    /// 打开数据库连接并确保 schema（每命令调用，SQLite 短连接开销可接受）。
    pub fn conn(&self) -> rusqlite::Result<Connection> {
        Connection::open(&self.db_path)
    }
}
