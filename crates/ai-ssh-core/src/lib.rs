//! AI-SSH 核心逻辑 crate。
//!
//! 纯本地原则：所有逻辑可脱离 Tauri/GUI 独立测试，SSH/监控依赖由上层注入。
//! 模块划分对应 PRD 第 2.2 节：safety_guard / context_engine / memory_store /
//! llm_adapter / monitor / metrics / audit_log。

pub mod audit;
pub mod crypto;
pub mod db;
pub mod desensitize;
pub mod error;
pub mod llm;
pub mod memory;
pub mod metrics;
pub mod monitor;
pub mod prompt;
pub mod safety;
pub mod schema;

pub use error::{Error, Result};

/// 产品版本号（对应 PRD version 1.2.0）
pub const VERSION: &str = "1.2.0";
