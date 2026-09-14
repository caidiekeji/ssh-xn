// 监控模块（模块G + F8）：独立 exec 通道周期采集（不干扰交互终端）、
// 解析 → 快照缓存 → 事件推送 → 分钟聚合入库 → 阈值告警（F8.5）→ 静音/回看
use std::sync::Arc;

use tauri::{AppHandle, Emitter};

use ai_ssh_core::metrics::{self, AlertFilter, MinuteAccumulator};
use ai_ssh_core::monitor;
use ai_ssh_core::schema::MetricsSnapshot;
use ai_ssh_core::{Error, Result};

use crate::state::{AppState, MonitorState};
use crate::ssh;

/// 取最近快照。
pub fn get_snapshot(state: &Arc<AppState>, host_id: i64) -> Result<MetricsSnapshot> {
    state
        .monitor
        .lock()
        .unwrap()
        .get(&host_id)
        .and_then(|m| m.last_snapshot.clone())
        .ok_or_else(|| Error::NotFound(format!("主机 {host_id} 暂无监控快照")))
}

/// 资源摘要文本（Prompt 注入用）。
pub async fn resource_summary(state: &Arc<AppState>, host_id: i64) -> String {
    match get_snapshot(state, host_id) {
        Ok(s) => format!(
            "CPU {:.1}% | 内存 {:.1}% ({}/{}MB) | Swap {:.1}% | 负载 {:.2}/{:.2}/{:.2} | 磁盘最高 {:.1}%",
            s.cpu.total_pct,
            s.memory.pct,
            s.memory.used_mb,
            s.memory.total_mb,
            s.memory.swap_pct,
            s.load.load1,
            s.load.load5,
            s.load.load15,
            s.disks.iter().map(|d| d.pct).fold(0.0_f64, f64::max),
        ),
        Err(_) => String::from("（监控未开启或暂无数据）"),
    }
}

/// 30 分钟趋势摘要（F8.6 Prompt 注入）。
pub async fn trend_summary(state: &Arc<AppState>, host_id: i64) -> String {
    let conn = match state.conn() {
        Ok(c) => c,
        Err(_) => return String::from("（无趋势数据）"),
    };
    let now = chrono::Utc::now().timestamp();
    let rows = metrics::get_history(&conn, host_id, "cpu", "1h", now).unwrap_or_default();
    if rows.is_empty() {
        return String::from("（近 30 分钟无聚合数据）");
    }
    let last30: Vec<_> = rows.iter().rev().take(30).collect();
    let max_cpu = last30.iter().map(|p| p.max).fold(0.0_f64, f64::max);
    let max_mem = metrics::get_history(&conn, host_id, "mem", "1h", now)
        .unwrap_or_default()
        .iter()
        .rev()
        .take(30)
        .map(|p| p.max)
        .fold(0.0_f64, f64::max);
    format!("近30分钟 CPU 峰值 {max_cpu:.1}%，内存峰值 {max_mem:.1}%（1min 粒度）")
}

/// 启动监控循环（每主机一个 tokio 任务；间隔取全局设置）。
pub async fn start(state: &Arc<AppState>, app: AppHandle, host_id: i64) -> Result<()> {
    {
        let mut tasks = state.monitor_tasks.lock().unwrap();
        if tasks.contains_key(&host_id) {
            return Ok(()); // 已在运行
        }
        state.monitor.lock().unwrap().entry(host_id).or_insert_with(MonitorState::default);
    }

    let state = state.clone();
    let task = tokio::spawn(async move {
        let mut fail_count = 0u32;
        loop {
            let interval = state
                .conn()
                .ok()
                .and_then(|c| ai_ssh_core::db::get_monitor_settings(&c).ok())
                .map(|s| s.interval_sec.max(1))
                .unwrap_or(3);

            // 找到该主机任一已连接会话（监控不建立新连接，复用 SSH）
            let sid = state
                .sessions
                .lock()
                .unwrap()
                .iter()
                .find(|(_, s)| s.host_id == host_id && s.connected)
                .map(|(k, _)| k.clone());

            match sid {
                Some(sid) => {
                    // F8.1：独立 exec channel 执行组合采集命令（注意事项 #10）
                    match ssh::exec(&state, &sid, monitor::linux_collect_cmd()).await {
                        Ok((raw, _)) => {
                            fail_count = 0;
                            let now = chrono::Utc::now().timestamp();
                            let (prev_cpu, prev_net) = {
                                let m = state.monitor.lock().unwrap();
                                let ms = m.get(&host_id);
                                (ms.and_then(|x| x.prev_cpu.clone()), ms.map(|x| x.prev_net.clone()).unwrap_or_default())
                            };
                            let cores = count_cores(&raw).max(1);
                            match monitor::assemble_linux_snapshot(host_id, now, &raw, prev_cpu.as_ref(), if prev_net.is_empty() { None } else { Some(&prev_net) }, interval as f64, cores) {
                                Ok(snap) => {
                                    // 更新基线 + 快照缓存
                                    {
                                        let sections = monitor::split_sections(&raw);
                                        let cur_cpu = monitor::parse_proc_stat(sections.get(monitor::SEP_STAT).map(|s| s.as_str()).unwrap_or(""));
                                        let cur_net = monitor::parse_proc_net_dev(sections.get(monitor::SEP_NET).map(|s| s.as_str()).unwrap_or(""));
                                        let mut m = state.monitor.lock().unwrap();
                                        if let Some(ms) = m.get_mut(&host_id) {
                                            if let Some(c) = cur_cpu {
                                                ms.prev_cpu = Some(c);
                                            }
                                            ms.prev_net = cur_net;
                                            ms.last_snapshot = Some(snap.clone());
                                        }
                                    }

                                    // 分钟聚合入库（注意事项 #13：每分钟 flush）
                                    let bucket = metrics::minute_bucket(now);
                                    {
                                        let mut accs = state.minute_acc.lock().unwrap();
                                        let acc = accs.entry(host_id).or_insert_with(|| MinuteAccumulator::new(bucket));
                                        if acc.bucket_start != bucket {
                                            if let Ok(conn) = state.conn() {
                                                let _ = metrics::flush_minute(&conn, host_id, acc);
                                                let _ = metrics::rollup_hour(&conn, host_id, metrics::hour_bucket(acc.bucket_start));
                                            }
                                            *acc = MinuteAccumulator::new(bucket);
                                        }
                                        acc.add(&snap);
                                    }

                                    // 告警评估（F8.5）
                                    if let Ok(conn) = state.conn() {
                                        let mut eval = state.alert_eval.lock().unwrap();
                                        if let Ok(decisions) = eval.evaluate(&conn, &snap, now) {
                                            for d in decisions {
                                                if let Ok(id) = metrics::create_alert(&conn, host_id, &d, now) {
                                                    let _ = app.emit(
                                                        "monitor_alert",
                                                        serde_json::json!({
                                                            "id": id,
                                                            "host_id": host_id,
                                                            "alert_type": metrics::typ_str(d.alert_type),
                                                            "value": d.value,
                                                            "threshold": d.threshold,
                                                            "triggered_at": chrono::Utc::now().to_rfc3339(),
                                                        }),
                                                    );
                                                }
                                            }
                                        }
                                    }

                                    // 事件推送（每采集周期一条）
                                    let _ = app.emit("monitor_metrics", serde_json::to_value(&snap).unwrap_or_default());
                                }
                                Err(e) => {
                                    fail_count += 1;
                                    if fail_count >= 3 {
                                        let _ = app.emit("monitor_metrics", serde_json::json!({ "host_id": host_id, "error": format!("采集失败: {e}") }));
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            fail_count += 1;
                            if fail_count >= 3 {
                                let _ = app.emit("monitor_metrics", serde_json::json!({ "host_id": host_id, "error": format!("采集失败: {e}") }));
                            }
                        }
                    }
                }
                None => {
                    // 无活动会话：暂停采集（连接恢复后自动重启，F8.1）
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(interval as u64)).await;
        }
    });
    state.monitor_tasks.lock().unwrap().insert(host_id, task);
    Ok(())
}

/// 统计 stat 段中 cpuN 行数（远端核心数）。
fn count_cores(raw: &str) -> u32 {
    raw.lines().filter(|l| l.starts_with("cpu") && !l.starts_with("cpu ")).count() as u32
}

pub async fn stop(state: &Arc<AppState>, host_id: i64) -> Result<()> {
    if let Some(task) = state.monitor_tasks.lock().unwrap().remove(&host_id) {
        task.abort();
    }
    state.monitor.lock().unwrap().remove(&host_id);
    state.minute_acc.lock().unwrap().remove(&host_id);
    Ok(())
}

pub fn get_history(state: &Arc<AppState>, host_id: i64, metric: &str, range: &str) -> Result<Vec<metrics::SeriesPoint>> {
    let conn = state.conn()?;
    metrics::get_history(&conn, host_id, metric, range, chrono::Utc::now().timestamp())
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertFilterInput {
    pub host_id: Option<i64>,
    pub only_active: Option<bool>,
}

pub fn list_alerts(state: &Arc<AppState>, filter: Option<AlertFilterInput>) -> Result<Vec<metrics::AlertRecord>> {
    let conn = state.conn()?;
    let f = AlertFilter {
        host_id: filter.as_ref().and_then(|f| f.host_id),
        only_active: filter.as_ref().and_then(|f| f.only_active).unwrap_or(false),
        limit: 50,
        ..Default::default()
    };
    metrics::list_alerts(&conn, &f)
}

pub fn mute_alert(state: &Arc<AppState>, id: i64, duration_secs: Option<i64>) -> Result<()> {
    let conn = state.conn()?;
    metrics::mute_alert(&conn, id, duration_secs)
}

pub fn get_settings(state: &Arc<AppState>) -> Result<ai_ssh_core::db::MonitorSettings> {
    let conn = state.conn()?;
    ai_ssh_core::db::get_monitor_settings(&conn)
}

pub fn update_settings(state: &Arc<AppState>, s: ai_ssh_core::db::MonitorSettings) -> Result<()> {
    let conn = state.conn()?;
    ai_ssh_core::db::update_monitor_settings(&conn, &s)
}

/// 保留期清理 + 会话上下文清理（启动时 + 每小时）。
pub fn cleanup(state: &Arc<AppState>) {
    if let Ok(conn) = state.conn() {
        let _ = ai_ssh_core::db::cleanup_expired(&conn);
        let _ = crate::context::cleanup_old_context(&conn);
    }
}
