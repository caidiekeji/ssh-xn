// SSH 管理器（模块A）：russh 客户端，多会话标签页、PTY、心跳、断线自动重连、
// 独立 exec channel（监控/命令执行不写入交互 PTY，实现注意事项 #10）、SFTP（exec 通道实现）
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use rand::Rng;
use russh::client::{Config, DisconnectReason, Handle, Session as SshSession};
use russh::keys::decode_secret_key;
use russh::{ChannelId, Disconnect};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use ai_ssh_core::audit;
use ai_ssh_core::schema::{AuditEntry, RiskLevel};
use ai_ssh_core::Error;
use ai_ssh_core::Result;

use crate::context;
use crate::hosts::{self, HostRow};
use crate::state::AppState;

#[derive(Clone)]
pub struct Session {
    pub host_id: i64,
    pub handle: Arc<Handle<ClientHandler>>,
    pub channel: Arc<russh::Channel<russh::client::Msg>>,
    pub connected: bool,
}

/// russh Handler：把远端输出转发为 Tauri 事件，并触发上下文记录/报错检测。
#[derive(Clone)]
pub struct ClientHandler {
    pub session_id: String,
    pub app: AppHandle,
    pub state: Arc<AppState>,
}

#[async_trait]
impl russh::client::Handler for ClientHandler {
    type Error = russh::Error;

    /// 演示环境信任所有主机密钥；生产环境应改为 known_hosts 校验（README 注明）。
    async fn check_server_key(&mut self, _key: &russh::keys::key::PublicKey) -> Result<bool, Self::Error> {
        Ok(true)
    }

    async fn channel_open_confirmation(&mut self, _c: ChannelId, _mps: u32, _ws: u32, _s: &mut SshSession) -> Result<(), Self::Error> {
        Ok(())
    }

    async fn data(&mut self, _channel: ChannelId, data: &[u8], _session: &mut SshSession) -> Result<(), Self::Error> {
        let text = String::from_utf8_lossy(data).to_string();
        let _ = self
            .app
            .emit("terminal_output", serde_json::json!({ "session_id": self.session_id, "data": text }));
        // F4.1 上下文记录 + F4.2 规则层报错检测（防抖推送）
        context::on_terminal_output(&self.state, &self.app, &self.session_id, &text);
        Ok(())
    }

    async fn channel_eof(&mut self, _c: ChannelId, _s: &mut SshSession) -> Result<(), Self::Error> {
        Ok(())
    }

    async fn channel_close(&mut self, _c: ChannelId, _s: &mut SshSession) -> Result<(), Self::Error> {
        self.mark_disconnected(None);
        Ok(())
    }

    async fn channel_failure(&mut self, _c: ChannelId, _s: &mut SshSession) -> Result<(), Self::Error> {
        Ok(())
    }

    async fn channel_success(&mut self, _c: ChannelId, _s: &mut SshSession) -> Result<(), Self::Error> {
        Ok(())
    }

    async fn auth_banner(&mut self, banner: &str, _s: &mut SshSession) -> Result<(), Self::Error> {
        let _ = self.app.emit(
            "terminal_output",
            serde_json::json!({ "session_id": self.session_id, "data": format!("\r\n\x1b[90m{banner}\x1b[0m") }),
        );
        Ok(())
    }

    async fn disconnected(&mut self, reason: DisconnectReason<Self::Error>) -> Result<(), Self::Error> {
        match reason {
            DisconnectReason::Error(e) => {
                self.mark_disconnected(Some(&e.to_string()));
                Err(e)
            }
            _ => {
                self.mark_disconnected(None);
                Ok(())
            }
        }
    }
}

impl ClientHandler {
    fn mark_disconnected(&self, reason: Option<&str>) {
        {
            let mut sessions = self.state.sessions.lock().unwrap();
            if let Some(s) = sessions.get_mut(&self.session_id) {
                s.connected = false;
            }
        }
        let code = reason.map(|r| format!(": {r}")).unwrap_or_default();
        let _ = self
            .app
            .emit("terminal_exit", serde_json::json!({ "session_id": self.session_id, "code": null, "reason": code }));
    }
}

#[derive(Serialize)]
pub struct HostConnected {
    pub ok: bool,
    pub message: String,
}

fn build_config() -> Config {
    let mut config = Config::default();
    config.keepalive_interval = Some(Duration::from_secs(15));
    config.keepalive_max = 3;
    config.inactivity_timeout = Some(Duration::from_secs(30));
    config
}

fn handler_for(state: &Arc<AppState>, app: &AppHandle, session_id: String) -> ClientHandler {
    ClientHandler {
        session_id,
        app: app.clone(),
        state: state.clone(),
    }
}

async fn open_shell(
    handle: &mut Handle<ClientHandler>,
) -> Result<russh::Channel<russh::client::Msg>> {
    let channel = handle
        .channel_open_session()
        .await
        .map_err(|e| Error::Ssh(format!("打开会话失败: {e}")))?;
    channel
        .request_pty(true, "xterm-256color", 80, 24, 0, 0, &[])
        .await
        .map_err(|e| Error::Ssh(format!("申请 PTY 失败: {e}")))?;
    channel
        .request_shell(true)
        .await
        .map_err(|e| Error::Ssh(format!("启动 shell 失败: {e}")))?;
    Ok(channel)
}

/// F1.2 连接：密码 / 私钥（含 passphrase）。
pub async fn connect(state: &Arc<AppState>, app: &AppHandle, host_id: i64) -> Result<String> {
    let conn = state.conn()?;
    let host = hosts::get_host(&conn, host_id)?;

    let session_id = format!("s{}", rand::thread_rng().gen::<u32>());
    let addr = tokio::net::lookup_host((host.host.as_str(), host.port as u16))
        .await
        .map_err(|e| Error::Ssh(format!("解析主机地址失败: {e}")))?
        .next()
        .ok_or_else(|| Error::Ssh("无法解析主机地址".into()))?;

    let handler = handler_for(state, app, session_id.clone());
    let mut handle = russh::client::connect(Arc::new(build_config()), addr, handler)
        .await
        .map_err(|e| Error::Ssh(format!("连接失败: {e}")))?;

    authenticate(&mut handle, &conn, &host, host_id).await?;
    let channel = open_shell(&mut handle).await?;

    // 多会话：每个标签页独立 SSH channel + PTY（F1.2）
    state.sessions.lock().unwrap().insert(
        session_id.clone(),
        Session {
            host_id,
            handle: Arc::new(handle),
            channel: Arc::new(channel),
            connected: true,
        },
    );

    // 断线自动重连（指数退避，最多 5 次，AC-8）
    spawn_reconnect_guard(state.clone(), app.clone(), session_id.clone(), host);

    Ok(session_id)
}

async fn authenticate(
    handle: &mut Handle<ClientHandler>,
    conn: &rusqlite::Connection,
    host: &HostRow,
    host_id: i64,
) -> Result<()> {
    let authed = match host.auth_type.as_str() {
        "private_key" => {
            let pass = hosts::get_passphrase(conn, host_id)?;
            let key_path = host.key_path.as_deref().ok_or_else(|| Error::Ssh("未配置私钥路径".into()))?;
            let key_text = std::fs::read_to_string(key_path)
                .map_err(|e| Error::Ssh(format!("读取私钥失败: {e}")))?;
            let key = decode_secret_key(&key_text, pass.as_deref())
                .map_err(|e| Error::Ssh(format!("解析私钥失败: {e}")))?;
            handle
                .authenticate_publickey(&host.username, Arc::new(key))
                .await
                .map_err(|e| Error::Ssh(format!("私钥认证失败: {e}")))?
        }
        _ => {
            let password = hosts::get_secret(conn, host_id)?
                .ok_or_else(|| Error::Ssh("主机未保存密码".into()))?;
            handle
                .authenticate_password(&host.username, &password)
                .await
                .map_err(|e| Error::Ssh(format!("密码认证失败: {e}")))?
        }
    };
    if !authed {
        return Err(Error::Ssh("认证被拒绝".into()));
    }
    Ok(())
}

/// 断线重连守卫：会话标记断开后按 1s,2s,4s,8s,16s 退避重试，最多 5 次。
fn spawn_reconnect_guard(state: Arc<AppState>, app: AppHandle, session_id: String, host: HostRow) {
    tokio::spawn(async move {
        let mut attempt = 0;
        loop {
            tokio::time::sleep(Duration::from_secs(1 << attempt.min(4))).await;
            let disconnected = state
                .sessions
                .lock()
                .unwrap()
                .get(&session_id)
                .map(|s| !s.connected)
                .unwrap_or(false);
            if !disconnected {
                return;
            }
            if attempt >= 5 {
                let _ = app.emit(
                    "terminal_exit",
                    serde_json::json!({ "session_id": session_id, "code": null, "reason": "重连超过 5 次，放弃" }),
                );
                return;
            }
            attempt += 1;
            if reconnect_attempt(&state, &app, &session_id, &host).await.is_ok() {
                return;
            }
        }
    });
}

async fn reconnect_attempt(state: &Arc<AppState>, app: &AppHandle, session_id: &str, host: &HostRow) -> Result<()> {
    let conn = state.conn()?;
    let addr = tokio::net::lookup_host((host.host.as_str(), host.port as u16))
        .await
        .map_err(|e| Error::Ssh(format!("解析主机地址失败: {e}")))?
        .next()
        .ok_or_else(|| Error::Ssh("无法解析主机地址".into()))?;

    let handler = handler_for(state, app, session_id.to_string());
    let mut handle = russh::client::connect(Arc::new(build_config()), addr, handler)
        .await
        .map_err(|e| Error::Ssh(format!("重连失败: {e}")))?;
    authenticate(&mut handle, &conn, host, host.id).await?;
    let channel = open_shell(&mut handle).await?;

    if let Some(s) = state.sessions.lock().unwrap().get_mut(session_id) {
        s.handle = Arc::new(handle);
        s.channel = Arc::new(channel);
        s.connected = true;
    }
    let _ = app.emit(
        "terminal_output",
        serde_json::json!({ "session_id": session_id, "data": "\r\n\x1b[32m[已自动重连]\x1b[0m\r\n" }),
    );
    Ok(())
}

/// 用户输入 → SSH channel（终端交互）
pub async fn write(state: &Arc<AppState>, session_id: &str, data: String) -> Result<()> {
    let s = state
        .sessions
        .lock()
        .unwrap()
        .get(session_id)
        .cloned()
        .ok_or_else(|| Error::NotFound(format!("会话 {session_id}")))?;
    s.channel
        .data(data.as_bytes())
        .await
        .map_err(|e| Error::Ssh(format!("写入失败: {e}")))?;
    // F4.1：记录整行用户输入命令
    if data.ends_with('\r') || data.ends_with('\n') {
        if let Ok(conn) = state.conn() {
            let _ = context::record_context(&conn, session_id, "user", data.trim());
        }
    }
    Ok(())
}

pub async fn resize(state: &Arc<AppState>, session_id: &str, cols: u32, rows: u32) -> Result<()> {
    let s = state
        .sessions
        .lock()
        .unwrap()
        .get(session_id)
        .cloned()
        .ok_or_else(|| Error::NotFound(format!("会话 {session_id}")))?;
    s.channel
        .window_change(cols, rows, cols * 8, rows * 16)
        .await
        .map_err(|e| Error::Ssh(format!("调整窗口失败: {e}")))?;
    Ok(())
}

pub async fn close(state: &Arc<AppState>, session_id: &str) -> Result<()> {
    let s = state
        .sessions
        .lock()
        .unwrap()
        .get(session_id)
        .cloned()
        .ok_or_else(|| Error::NotFound(format!("会话 {session_id}")))?;
    let _ = s.channel.eof().await;
    let _ = s.channel.close().await;
    let _ = s.handle.disconnect(Disconnect::ByApplication, "bye", "en").await;
    state.sessions.lock().unwrap().remove(session_id);
    Ok(())
}

/// 独立 exec 通道执行命令（AI 命令执行 / 监控采集共用）。
/// 返回 (输出文本, exit code)。不写入交互 PTY，不产生 terminal_output 事件（注意事项 #10）。
pub async fn exec(state: &Arc<AppState>, session_id: &str, command: &str) -> Result<(String, Option<u32>)> {
    let s = state
        .sessions
        .lock()
        .unwrap()
        .get(session_id)
        .cloned()
        .ok_or_else(|| Error::NotFound(format!("会话 {session_id}")))?;
    let mut channel = s
        .handle
        .channel_open_session()
        .await
        .map_err(|e| Error::Ssh(format!("打开 exec 通道失败: {e}")))?;
    channel
        .exec(true, command)
        .await
        .map_err(|e| Error::Ssh(format!("exec 失败: {e}")))?;

    let mut output = String::new();
    let mut exit: Option<u32> = None;
    while let Some(msg) = channel.wait().await {
        match msg {
            russh::ChannelMsg::Data { data } => output.push_str(&String::from_utf8_lossy(&data)),
            russh::ChannelMsg::ExitStatus { exit_status } => exit = Some(exit_status),
            russh::ChannelMsg::Close => break,
            _ => {}
        }
    }
    Ok((output, exit))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ExecAudit {
    pub user_input: Option<String>,
    pub risk: Option<String>,
}

#[derive(Serialize)]
pub struct ExecResult {
    pub exit_code: Option<u32>,
}

/// ssh_execute：AI 命令执行（写交互 shell + 回车，输出回传终端）。
/// 服务端强制再跑一次 safety_check（实现注意事项 #1：无绕过路径）。
pub async fn execute_command(
    state: &Arc<AppState>,
    session_id: &str,
    command: &str,
    audit: Option<ExecAudit>,
    confirmed: bool,
) -> Result<ExecResult> {
    let s = state
        .sessions
        .lock()
        .unwrap()
        .get(session_id)
        .cloned()
        .ok_or_else(|| Error::NotFound(format!("会话 {session_id}")))?;

    // F6.1：服务端强制安全检测（无任何绕过路径）
    let verdict = ai_ssh_core::safety::safety_check(command, None);
    if verdict.level == RiskLevel::High && !confirmed {
        return Err(Error::Forbidden(format!("高危命令需确认后执行：{}", verdict.reasons.join("; "))));
    }
    if verdict.level == RiskLevel::Medium && !confirmed {
        return Err(Error::Forbidden(format!("中危命令需确认后执行：{}", verdict.reasons.join("; "))));
    }

    let audit_input = audit.as_ref().and_then(|a| a.user_input.clone());
    let risk_str = match verdict.level {
        RiskLevel::High => "high",
        RiskLevel::Medium => "medium",
        RiskLevel::Low => "low",
    };

    // F6.2 审计（只增不改）
    if let Ok(conn) = state.conn() {
        let _ = audit::record(
            &conn,
            &AuditEntry {
                id: None,
                created_at: None,
                host_id: Some(s.host_id),
                user_input: audit_input.clone(),
                generated_cmd: command.to_string(),
                risk_level: Some(risk_str.to_string()),
                executed: true,
                result_summary: None,
                memory_ids_used: None,
            },
        );
    }

    // F7.1 命令历史
    if let Ok(conn) = state.conn() {
        let _ = conn.execute(
            "INSERT INTO command_history (session_id, host_id, command, user_input, risk_level, executed, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 1, datetime('now'))",
            rusqlite::params![session_id, s.host_id, command, audit_input, risk_str],
        );
    }

    // 写入交互 shell 执行（输出经 handler 回传终端）
    write(state, session_id, format!("{command}\r")).await?;
    Ok(ExecResult { exit_code: None })
}

/// SFTP（F1.4，经 exec 通道实现）：列目录 / 上传 / 下载，进度事件。
pub async fn sftp_list(state: &Arc<AppState>, session_id: &str, path: &str) -> Result<Vec<SftpEntry>> {
    let (out, _) = exec(state, session_id, &format!("ls -la --time-style=long-iso {} 2>&1", shell_quote(path))).await?;
    let mut entries = Vec::new();
    for line in out.lines().skip(1) {
        let mut parts = line.split_whitespace();
        let perms = parts.next().unwrap_or("").to_string();
        let _ = parts.next(); // links
        let _ = parts.next(); // owner
        let _ = parts.next(); // group
        let size: i64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        let _ = parts.next(); // date
        let _ = parts.next(); // time
        let name: String = parts.collect::<Vec<_>>().join(" ");
        if name.is_empty() || name == "." || name == ".." {
            continue;
        }
        entries.push(SftpEntry {
            name,
            is_dir: perms.starts_with('d'),
            size,
        });
    }
    Ok(entries)
}

/// 简易 shell 引号转义（防命令注入）。
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[derive(Serialize)]
pub struct SftpEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: i64,
}

/// F1.1 连接测试：真实建连认证后立即断开。
pub async fn test_connection(state: &Arc<AppState>, app: &AppHandle, host_id: i64) -> Result<HostConnected> {
    let conn = state.conn()?;
    let host = hosts::get_host(&conn, host_id)?;
    let addr = tokio::net::lookup_host((host.host.as_str(), host.port as u16))
        .await
        .map_err(|e| Error::Ssh(format!("解析主机地址失败: {e}")))?
        .next()
        .ok_or_else(|| Error::Ssh("无法解析主机地址".into()))?;

    let mut config = Config::default();
    config.inactivity_timeout = Some(Duration::from_secs(10));

    let handler = handler_for(state, app, format!("test{}", host_id));
    let mut handle = russh::client::connect(Arc::new(config), addr, handler)
        .await
        .map_err(|e| Error::Ssh(format!("连接失败: {e}")))?;
    authenticate(&mut handle, &conn, &host, host_id).await?;
    let _ = handle.disconnect(Disconnect::ByApplication, "test done", "en").await;
    Ok(HostConnected { ok: true, message: "连接成功，认证通过".into() })
}
