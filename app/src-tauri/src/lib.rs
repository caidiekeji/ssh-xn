// AI-SSH Tauri 应用入口：状态初始化 + 全部 IPC 命令注册。
// 启动序列：建数据目录 → 建库(PRD §7) → 日志/panic hook → AppState → tauri run。
// 任何启动期 panic 都会写 <data_dir>/logs/panic.log，Windows 闪退排查靠它。

use std::sync::{Arc, Mutex};

use ai_ssh_core::db;
use rusqlite::Connection;
use tauri::Manager;

use crate::context::AppContext;
use crate::monitor::{self, MetricsSnapshot};
use crate::ssh::SshState;
use crate::state::AppState;
use crate::state::{HostInfo, ProviderConfig};

mod autosave;
mod context;
mod hosts;
mod llm;
mod monitor;
mod safety;
mod ssh;
mod state;

pub fn run() {
    let data_dir = app_data_dir();
    init_logging(&data_dir);
    let db_path = data_dir.join("ai-ssh.db");
    // 首次启动建库（PRD 第 7 节全部表）
    {
        let conn = rusqlite::Connection::open(&db_path).expect("无法打开数据库");
        db::init_db(&conn).expect("初始化数据库失败");
    }

    let state = Arc::new(AppState::new(db_path));

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(state.clone())
        .invoke_handler(tauri::generate_handler![
            app_version,
            list_hosts,
            save_host,
            delete_host,
            test_host_connection,
            ssh_connect,
            ssh_close,
            ssh_write,
            ssh_resize,
            ssh_execute,
            sftp_list,
            llm_list_providers,
            llm_save_provider,
            llm_test_provider,
            llm_generate_command,
            llm_analyze_error,
            llm_analyze_alert,
            memory_search,
            memory_save,
            memory_list,
            memory_update,
            memory_delete,
            memory_import_markdown,
            memory_export_markdown,
            memory_feedback,
            monitor_start,
            monitor_stop,
            monitor_get_snapshot,
            monitor_get_history,
            monitor_list_alerts,
            monitor_mute_alert,
            monitor_update_settings,
            safety_check,
            audit_export_csv,
        ])
        .setup(move |app| {
            // 启动清理：过期指标/告警/会话上下文（失败仅记录，不阻塞启动）
            let _ = monitor::cleanup(&state);
            let _ = context::cleanup_old_context(&state);
            let _ = autosave::autosave_loop(state.clone());
            let _ = app;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// 应用数据目录（跨平台）：<data_dir>/ai-ssh
/// 必须先行创建目录：首次运行（尤其 Windows 首装）目录不存在，
/// 否则 Connection::open 失败 panic 导致应用闪退无法打开。
fn app_data_dir() -> std::path::PathBuf {
    let dir = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("ai-ssh");
    std::fs::create_dir_all(&dir).expect("无法创建 AI-SSH 数据目录");
    dir
}

/// 简单文件日志 + panic hook：
/// Windows release 版无控制台（windows_subsystem=windows），任何启动期错误都是静默闪退，
/// 这里把启动步骤与 panic 信息写入 <data_dir>/logs/，闪退后用户可提供日志精确定位。
fn init_logging(dir: &std::path::Path) {
    let log_dir = dir.join("logs");
    let _ = std::fs::create_dir_all(&log_dir);
    let log_file = log_dir.join("ai-ssh.log");
    let panic_file = log_dir.join("panic.log");

    std::panic::set_hook(Box::new(move |info| {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&panic_file) {
            let _ = writeln!(f, "[{}] {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), info);
        }
        eprintln!("{}", info);
    }));

    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&log_file) {
        let _ = writeln!(f, "[{}] AI-SSH {} 启动", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), ai_ssh_core::VERSION);
        #[cfg(target_os = "windows")]
        {
            let _ = writeln!(
                f,
                "WebView2 检测: {}",
                if webview2_installed() { "已安装" } else { "未检测到（界面闪退最常见原因）" }
            );
        }
    }
}

/// Windows 注册表检测 WebView2 Runtime（EdgeUpdate Client 键的 pv 版本号）。
#[cfg(target_os = "windows")]
fn webview2_installed() -> bool {
    let keys = [
        r"HKLM\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}",
        r"HKLM\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}",
        r"HKCU\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}",
    ];
    keys.iter().any(|k| {
        std::process::Command::new("reg")
            .args(["query", k, "/v", "pv"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    })
}