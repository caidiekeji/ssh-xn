// AI-SSH Tauri 应用入口：状态初始化 + 全部 IPC 命令注册（PRD 第 8 节）
mod autosave;
mod context;
mod hosts;
mod llm;
mod monitor;
mod safety;
mod ssh;
mod state;

use std::sync::Arc;

use tauri::Manager;
use tauri::State;

use ai_ssh_core::db;
use ai_ssh_core::schema::{MemoryCase, ProviderConfig};

use crate::state::AppState;

#[tauri::command]
fn app_version() -> String {
    ai_ssh_core::VERSION.to_string()
}

// ===== 主机（F1） =====

#[tauri::command]
fn list_hosts(state: State<Arc<AppState>>) -> ai_ssh_core::Result<Vec<hosts::HostRow>> {
    let conn = state.conn()?;
    hosts::list_hosts(&conn)
}

#[tauri::command]
fn save_host(
    state: State<Arc<AppState>>,
    input: hosts::HostInput,
    id: Option<i64>,
) -> ai_ssh_core::Result<i64> {
    let conn = state.conn()?;
    hosts::save_host(&conn, &input, id)
}

#[tauri::command]
fn delete_host(state: State<Arc<AppState>>, id: i64) -> ai_ssh_core::Result<()> {
    let conn = state.conn()?;
    hosts::delete_host(&conn, id)
}

#[tauri::command]
async fn test_host_connection(
    state: State<'_, Arc<AppState>>,
    app: tauri::AppHandle,
    id: i64,
) -> ai_ssh_core::Result<ssh::HostConnected> {
    ssh::test_connection(&state, &app, id).await
}

#[tauri::command]
async fn ssh_connect(
    state: State<'_, Arc<AppState>>,
    app: tauri::AppHandle,
    host_id: i64,
) -> ai_ssh_core::Result<String> {
    ssh::connect(&state, &app, host_id).await
}

#[tauri::command]
async fn ssh_close(state: State<'_, Arc<AppState>>, session_id: String) -> ai_ssh_core::Result<()> {
    ssh::close(&state, &session_id).await
}

#[tauri::command]
async fn ssh_write(
    state: State<'_, Arc<AppState>>,
    session_id: String,
    data: String,
) -> ai_ssh_core::Result<()> {
    ssh::write(&state, &session_id, data).await
}

#[tauri::command]
async fn ssh_resize(
    state: State<'_, Arc<AppState>>,
    session_id: String,
    cols: u32,
    rows: u32,
) -> ai_ssh_core::Result<()> {
    ssh::resize(&state, &session_id, cols, rows).await
}

#[tauri::command]
async fn ssh_execute(
    state: State<'_, Arc<AppState>>,
    session_id: String,
    command: String,
    audit: Option<ssh::ExecAudit>,
    confirmed: bool,
) -> ai_ssh_core::Result<ssh::ExecResult> {
    ssh::execute_command(&state, &session_id, &command, audit, confirmed).await
}

#[tauri::command]
async fn sftp_list(
    state: State<'_, Arc<AppState>>,
    session_id: String,
    path: String,
) -> ai_ssh_core::Result<Vec<ssh::SftpEntry>> {
    ssh::sftp_list(&state, &session_id, &path).await
}

// ===== LLM（F2 / F3 / F4 / F8.6） =====

#[tauri::command]
fn llm_list_providers(state: State<Arc<AppState>>) -> ai_ssh_core::Result<Vec<serde_json::Value>> {
    llm::list_providers(&state)
}

#[tauri::command]
fn llm_save_provider(
    state: State<Arc<AppState>>,
    provider: ProviderConfig,
    id: Option<i64>,
) -> ai_ssh_core::Result<i64> {
    llm::save_provider(&state, provider, id)
}

#[tauri::command]
async fn llm_test_provider(
    state: State<'_, Arc<AppState>>,
    provider_id: i64,
) -> ai_ssh_core::Result<serde_json::Value> {
    let (ok, latency_ms, reply) = llm::test_provider(&state, provider_id).await?;
    Ok(serde_json::json!({ "ok": ok, "latency_ms": latency_ms, "reply": reply }))
}

#[tauri::command]
async fn llm_generate_command(
    state: State<'_, Arc<AppState>>,
    app: tauri::AppHandle,
    session_id: String,
    user_input: String,
) -> ai_ssh_core::Result<()> {
    llm::generate_command(&state, &app, &session_id, &user_input).await
}

#[tauri::command]
async fn llm_analyze_error(
    state: State<'_, Arc<AppState>>,
    app: tauri::AppHandle,
    session_id: String,
    error_text: String,
) -> ai_ssh_core::Result<()> {
    llm::analyze_error(&state, &app, &session_id, &error_text).await
}

#[tauri::command]
async fn llm_analyze_alert(
    state: State<'_, Arc<AppState>>,
    app: tauri::AppHandle,
    host_id: i64,
    alert_id: i64,
) -> ai_ssh_core::Result<()> {
    llm::analyze_alert(&state, &app, host_id, alert_id).await
}

// ===== 历史记忆（F5） =====

#[tauri::command]
fn memory_search(
    state: State<Arc<AppState>>,
    host_id: Option<i64>,
    query: String,
) -> ai_ssh_core::Result<Vec<MemoryCase>> {
    llm::memory_search(&state, host_id, &query)
}

#[tauri::command]
fn memory_save(state: State<Arc<AppState>>, case: MemoryCase) -> ai_ssh_core::Result<i64> {
    llm::memory_save(&state, case)
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct MemoryFilterInput {
    pub host_id: Option<i64>,
    pub problem_type: Option<String>,
    pub verified: Option<bool>,
    pub search: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[tauri::command]
fn memory_list(
    state: State<Arc<AppState>>,
    filter: Option<MemoryFilterInput>,
) -> ai_ssh_core::Result<Vec<MemoryCase>> {
    let f = ai_ssh_core::memory::CaseFilter {
        host_id: filter.as_ref().and_then(|f| f.host_id),
        problem_type: filter.as_ref().and_then(|f| f.problem_type.clone()),
        verified: filter.as_ref().and_then(|f| f.verified),
        search: filter.as_ref().and_then(|f| f.search.clone()),
        limit: filter.as_ref().and_then(|f| f.limit).unwrap_or(50),
        offset: filter.as_ref().and_then(|f| f.offset).unwrap_or(0),
    };
    llm::memory_list(&state, f)
}

#[tauri::command]
fn memory_update(
    state: State<Arc<AppState>>,
    id: i64,
    patch: MemoryCase,
) -> ai_ssh_core::Result<()> {
    llm::memory_update(&state, id, patch)
}

#[tauri::command]
fn memory_delete(state: State<Arc<AppState>>, id: i64) -> ai_ssh_core::Result<()> {
    llm::memory_delete(&state, id)
}

#[tauri::command]
fn memory_feedback(state: State<Arc<AppState>>, id: i64, up: bool) -> ai_ssh_core::Result<()> {
    llm::memory_feedback(&state, id, up)
}

#[tauri::command]
fn memory_import_markdown(state: State<Arc<AppState>>, text: String) -> ai_ssh_core::Result<i64> {
    llm::memory_import_markdown(&state, &text)
}

#[tauri::command]
fn memory_export_markdown(state: State<Arc<AppState>>, id: i64) -> ai_ssh_core::Result<String> {
    llm::memory_export_markdown(&state, id)
}

#[tauri::command]
async fn memory_auto_save(
    state: State<'_, Arc<AppState>>,
    session_id: String,
    host_id: i64,
    case: autosave::AutoSaveInput,
) -> ai_ssh_core::Result<autosave::AutoSaveResult> {
    autosave::auto_save(&state, &session_id, host_id, case).await
}

// ===== 监控（F8） =====

#[tauri::command]
async fn monitor_start(
    state: State<'_, Arc<AppState>>,
    app: tauri::AppHandle,
    host_id: i64,
) -> ai_ssh_core::Result<()> {
    monitor::start(&state, app, host_id).await
}

#[tauri::command]
async fn monitor_stop(state: State<'_, Arc<AppState>>, host_id: i64) -> ai_ssh_core::Result<()> {
    monitor::stop(&state, host_id).await
}

#[tauri::command]
fn monitor_get_snapshot(
    state: State<Arc<AppState>>,
    host_id: i64,
) -> ai_ssh_core::Result<ai_ssh_core::schema::MetricsSnapshot> {
    monitor::get_snapshot(&state, host_id)
}

#[tauri::command]
fn monitor_get_history(
    state: State<Arc<AppState>>,
    host_id: i64,
    metric: String,
    range: String,
) -> ai_ssh_core::Result<Vec<ai_ssh_core::metrics::SeriesPoint>> {
    monitor::get_history(&state, host_id, &metric, &range)
}

#[tauri::command]
fn monitor_list_alerts(
    state: State<Arc<AppState>>,
    filter: Option<monitor::AlertFilterInput>,
) -> ai_ssh_core::Result<Vec<ai_ssh_core::metrics::AlertRecord>> {
    monitor::list_alerts(&state, filter)
}

#[tauri::command]
fn monitor_mute_alert(
    state: State<Arc<AppState>>,
    id: i64,
    duration_secs: Option<i64>,
) -> ai_ssh_core::Result<()> {
    monitor::mute_alert(&state, id, duration_secs)
}

#[tauri::command]
fn monitor_get_settings(
    state: State<Arc<AppState>>,
) -> ai_ssh_core::Result<ai_ssh_core::db::MonitorSettings> {
    monitor::get_settings(&state)
}

#[tauri::command]
fn monitor_update_settings(
    state: State<Arc<AppState>>,
    settings: ai_ssh_core::db::MonitorSettings,
) -> ai_ssh_core::Result<()> {
    monitor::update_settings(&state, settings)
}

// ===== 安全 / 审计（F6） =====

#[tauri::command]
fn safety_check(command: String) -> ai_ssh_core::safety::SafetyVerdict {
    safety::safety_check(&command)
}

#[tauri::command]
fn audit_export_csv(state: State<Arc<AppState>>) -> ai_ssh_core::Result<String> {
    safety::audit_export_csv(&state)
}

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
            memory_feedback,
            memory_import_markdown,
            memory_export_markdown,
            memory_auto_save,
            monitor_start,
            monitor_stop,
            monitor_get_snapshot,
            monitor_get_history,
            monitor_list_alerts,
            monitor_mute_alert,
            monitor_get_settings,
            monitor_update_settings,
            safety_check,
            audit_export_csv,
        ])
        .setup(move |app| {
            // 启动时清理过期数据（PRD 第 7 节末尾）
            monitor::cleanup(&state);
            // 每小时清理
            let state2 = state.clone();
            tokio::spawn(async move {
                let mut tick = tokio::time::interval(std::time::Duration::from_secs(3600));
                loop {
                    tick.tick().await;
                    monitor::cleanup(&state2);
                }
            });
            let _ = app;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running AI-SSH");
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
    keys.iter().copied().any(|k| {
        std::process::Command::new("reg")
            .arg("query")
            .arg(k)
            .arg("/v")
            .arg("pv")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    })
}
