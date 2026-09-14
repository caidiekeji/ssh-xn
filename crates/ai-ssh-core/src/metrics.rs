//! 指标历史降采样与阈值告警（F8.4 / F8.5 / F8.6）。
//! - 原始粒度（3s）仅保留内存滑动窗口；入库 1min（7 天）/ 1hour（90 天）
//! - 告警防抖：恢复正常 5 分钟后才允许再次告警；CPU 需持续 60s 超阈值
//! - 批量聚合每分钟 flush 一次，避免高频磁盘写入（实现注意事项 #13）

use rusqlite::{params, Connection, OptionalExtension};

use crate::db::get_monitor_settings;
use crate::error::{Error, Result};
use crate::schema::{AlertDecision, AlertType, MetricsSnapshot};

/// CPU 持续超阈值判定窗口（F8.5）。
pub const CPU_SUSTAIN_SECS: i64 = 60;
/// 恢复后防抖窗口。
pub const RE_ALERT_DEBOUNCE_SECS: i64 = 300;

// ============ 历史降采样 ============

/// 单分钟桶累加器（每个主机一个，内存态）。
#[derive(Debug, Clone, Default)]
pub struct MinuteAccumulator {
    pub bucket_start: i64,
    pub n: i64,
    pub cpu_user_sum: f64,
    pub cpu_user_max: f64,
    pub cpu_sys_sum: f64,
    pub cpu_sys_max: f64,
    pub cpu_iowait_sum: f64,
    pub cpu_iowait_max: f64,
    pub load1_sum: f64,
    pub load1_max: f64,
    pub mem_pct_sum: f64,
    pub mem_pct_max: f64,
    pub swap_pct_sum: f64,
    pub swap_pct_max: f64,
    pub disk_max_pct_sum: f64,
    pub disk_max_pct_max: f64,
    pub net_rx_sum: f64,
    pub net_tx_sum: f64,
}

impl MinuteAccumulator {
    pub fn new(bucket_start: i64) -> Self {
        Self { bucket_start, ..Default::default() }
    }

    pub fn add(&mut self, s: &MetricsSnapshot) {
        self.n += 1;
        self.cpu_user_sum += s.cpu.user_pct;
        self.cpu_user_max = self.cpu_user_max.max(s.cpu.user_pct);
        self.cpu_sys_sum += s.cpu.sys_pct;
        self.cpu_sys_max = self.cpu_sys_max.max(s.cpu.sys_pct);
        self.cpu_iowait_sum += s.cpu.iowait_pct;
        self.cpu_iowait_max = self.cpu_iowait_max.max(s.cpu.iowait_pct);
        self.load1_sum += s.load.load1;
        self.load1_max = self.load1_max.max(s.load.load1);
        self.mem_pct_sum += s.memory.pct;
        self.mem_pct_max = self.mem_pct_max.max(s.memory.pct);
        self.swap_pct_sum += s.memory.swap_pct;
        self.swap_pct_max = self.swap_pct_max.max(s.memory.swap_pct);
        let disk_max = s.disks.iter().map(|d| d.pct).fold(0.0_f64, f64::max);
        self.disk_max_pct_sum += disk_max;
        self.disk_max_pct_max = self.disk_max_pct_max.max(disk_max);
        let rx = s.network.iter().map(|n| n.rx_bps).sum::<f64>();
        let tx = s.network.iter().map(|n| n.tx_bps).sum::<f64>();
        self.net_rx_sum += rx;
        self.net_tx_sum += tx;
    }

    fn avg(&self, sum: f64) -> f64 {
        if self.n > 0 { sum / self.n as f64 } else { 0.0 }
    }
}

/// 把一分钟桶 flush 到 metrics_history（UPSERT，1min 粒度）。
pub fn flush_minute(conn: &Connection, host_id: i64, acc: &MinuteAccumulator) -> Result<()> {
    if acc.n == 0 {
        return Ok(());
    }
    let bucket = bucket_str(acc.bucket_start);
    conn.execute(
        "INSERT INTO metrics_history
         (host_id, granularity, bucket_start,
          cpu_user_avg, cpu_user_max, cpu_sys_avg, cpu_sys_max, cpu_iowait_avg, cpu_iowait_max,
          load1_avg, load1_max, mem_pct_avg, mem_pct_max, swap_pct_avg, swap_pct_max,
          disk_max_pct_avg, disk_max_pct_max, net_rx_avg, net_tx_avg)
         VALUES (?1,'1min',?2, ?3,?4, ?5,?6, ?7,?8, ?9,?10, ?11,?12, ?13,?14, ?15,?16, ?17,?18)
         ON CONFLICT(host_id, granularity, bucket_start) DO UPDATE SET
           cpu_user_avg=excluded.cpu_user_avg, cpu_user_max=excluded.cpu_user_max,
           cpu_sys_avg=excluded.cpu_sys_avg, cpu_sys_max=excluded.cpu_sys_max,
           cpu_iowait_avg=excluded.cpu_iowait_avg, cpu_iowait_max=excluded.cpu_iowait_max,
           load1_avg=excluded.load1_avg, load1_max=excluded.load1_max,
           mem_pct_avg=excluded.mem_pct_avg, mem_pct_max=excluded.mem_pct_max,
           swap_pct_avg=excluded.swap_pct_avg, swap_pct_max=excluded.swap_pct_max,
           disk_max_pct_avg=excluded.disk_max_pct_avg, disk_max_pct_max=excluded.disk_max_pct_max,
           net_rx_avg=excluded.net_rx_avg, net_tx_avg=excluded.net_tx_avg",
        params![
            host_id, bucket,
            acc.avg(acc.cpu_user_sum), acc.cpu_user_max,
            acc.avg(acc.cpu_sys_sum), acc.cpu_sys_max,
            acc.avg(acc.cpu_iowait_sum), acc.cpu_iowait_max,
            acc.avg(acc.load1_sum), acc.load1_max,
            acc.avg(acc.mem_pct_sum), acc.mem_pct_max,
            acc.avg(acc.swap_pct_sum), acc.swap_pct_max,
            acc.avg(acc.disk_max_pct_sum), acc.disk_max_pct_max,
            acc.avg(acc.net_rx_sum), acc.avg(acc.net_tx_sum),
        ],
    )?;
    Ok(())
}

/// 从 1min 行聚合生成 1hour 行（每小时末尾调用）。
pub fn rollup_hour(conn: &Connection, host_id: i64, hour_start: i64) -> Result<()> {
    let bucket = bucket_str(hour_start);
    conn.execute(
        r#"INSERT INTO metrics_history
           (host_id, granularity, bucket_start,
            cpu_user_avg, cpu_user_max, cpu_sys_avg, cpu_sys_max, cpu_iowait_avg, cpu_iowait_max,
            load1_avg, load1_max, mem_pct_avg, mem_pct_max, swap_pct_avg, swap_pct_max,
            disk_max_pct_avg, disk_max_pct_max, net_rx_avg, net_tx_avg)
         SELECT ?1,'1hour',?2,
            AVG(cpu_user_avg), MAX(cpu_user_max), AVG(cpu_sys_avg), MAX(cpu_sys_max),
            AVG(cpu_iowait_avg), MAX(cpu_iowait_max), AVG(load1_avg), MAX(load1_max),
            AVG(mem_pct_avg), MAX(mem_pct_max), AVG(swap_pct_avg), MAX(swap_pct_max),
            AVG(disk_max_pct_avg), MAX(disk_max_pct_max), AVG(net_rx_avg), AVG(net_tx_avg)
         FROM metrics_history
         WHERE host_id=?1 AND granularity='1min' AND bucket_start >= ?2 AND bucket_start < ?3
         ON CONFLICT(host_id, granularity, bucket_start) DO UPDATE SET
           cpu_user_avg=excluded.cpu_user_avg, cpu_user_max=excluded.cpu_user_max,
           cpu_sys_avg=excluded.cpu_sys_avg, cpu_sys_max=excluded.cpu_sys_max,
           cpu_iowait_avg=excluded.cpu_iowait_avg, cpu_iowait_max=excluded.cpu_iowait_max,
           load1_avg=excluded.load1_avg, load1_max=excluded.load1_max,
           mem_pct_avg=excluded.mem_pct_avg, mem_pct_max=excluded.mem_pct_max,
           swap_pct_avg=excluded.swap_pct_avg, swap_pct_max=excluded.swap_pct_max,
           disk_max_pct_avg=excluded.disk_max_pct_avg, disk_max_pct_max=excluded.disk_max_pct_max,
           net_rx_avg=excluded.net_rx_avg, net_tx_avg=excluded.net_tx_avg"#,
        params![host_id, bucket, bucket_hour_end(hour_start)],
    )?;
    Ok(())
}

/// 时间戳 → "YYYY-MM-DD HH:MM:SS"（UTC 落库；前端展示时转本地）。
pub fn bucket_str(ts: i64) -> String {
    use chrono::TimeZone;
    chrono::Utc.timestamp_opt(ts, 0).unwrap().format("%Y-%m-%d %H:%M:%S").to_string()
}

fn bucket_hour_end(hour_start: i64) -> String {
    use chrono::TimeZone;
    chrono::Utc
        .timestamp_opt(hour_start + 3600, 0)
        .unwrap()
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}

/// 分钟桶起始（对齐到分钟）。
pub fn minute_bucket(ts: i64) -> i64 {
    ts - ts % 60
}

/// 小时桶起始（对齐到小时）。
pub fn hour_bucket(ts: i64) -> i64 {
    ts - ts % 3600
}

/// 时间范围 → 查询粒度（PRD F8.4：1h/24h 用 1min，7d/30d 用 1hour）。
pub fn granularity_for(range: &str) -> &'static str {
    match range {
        "7d" | "30d" => "1hour",
        _ => "1min",
    }
}

/// 回看：查询某主机的聚合序列。
#[derive(Debug, Clone, serde::Serialize)]
pub struct SeriesPoint {
    pub ts: i64,
    pub avg: f64,
    pub max: f64,
}

pub fn get_history(
    conn: &Connection,
    host_id: i64,
    metric: &str,
    range: &str,
    now: i64,
) -> Result<Vec<SeriesPoint>> {
    let gran = granularity_for(range);
    let hours_back = match range {
        "1h" => 1,
        "24h" => 24,
        "7d" => 24 * 7,
        "30d" => 24 * 30,
        _ => 24,
    };
    let since = now - hours_back * 3600;
    let col = match metric {
        "cpu" => ("cpu_user_avg", "cpu_user_max"),
        "mem" => ("mem_pct_avg", "mem_pct_max"),
        "swap" => ("swap_pct_avg", "swap_pct_max"),
        "disk" => ("disk_max_pct_avg", "disk_max_pct_max"),
        "load1" => ("load1_avg", "load1_max"),
        "net_rx" => ("net_rx_avg", "net_rx_avg"),
        "net_tx" => ("net_tx_avg", "net_tx_avg"),
        _ => return Err(Error::InvalidArgument(format!("未知指标 {metric}"))),
    };
    let since_str = bucket_str(since);
    let mut stmt = conn.prepare(&format!(
        "SELECT bucket_start, {a}, {m} FROM metrics_history
         WHERE host_id=?1 AND granularity=?2 AND bucket_start >= ?3
         ORDER BY bucket_start",
        a = col.0,
        m = col.1
    ))?;
    let rows = stmt.query_map(params![host_id, gran, since_str], |r| {
        Ok(SeriesPoint {
            ts: r.get::<_, String>(0)?.parse_ts(),
            avg: r.get(1)?,
            max: r.get(2)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

trait TsParse {
    fn parse_ts(&self) -> i64;
}
impl TsParse for String {
    fn parse_ts(&self) -> i64 {
        chrono::NaiveDateTime::parse_from_str(self, "%Y-%m-%d %H:%M:%S")
            .map(|d| d.and_utc().timestamp())
            .unwrap_or(0)
    }
}

// ============ 阈值告警 ============

/// 告警评估器：内存态只保留"首次超阈值时间"（CPU 60s 判定），其余状态查库。
#[derive(Debug, Default)]
pub struct AlertEvaluator {
    first_exceed: std::collections::HashMap<(i64, AlertType), i64>,
}

impl AlertEvaluator {
    pub fn new() -> Self {
        Self::default()
    }

    /// 对一份快照做告警评估；触发决策由调用方落库（create_alert）并推送事件。
    pub fn evaluate(&mut self, conn: &Connection, s: &MetricsSnapshot, now: i64) -> Result<Vec<AlertDecision>> {
        let settings = get_monitor_settings(conn)?;
        let mut out = Vec::new();

        let checks: Vec<(AlertType, f64, f64, i64)> = vec![
            (AlertType::Cpu, s.cpu.total_pct, settings.cpu_alert, CPU_SUSTAIN_SECS),
            (AlertType::Memory, s.memory.pct, settings.mem_alert, 0),
            (AlertType::Swap, s.memory.swap_pct, settings.swap_alert, 0),
            (
                AlertType::Disk,
                s.disks.iter().map(|d| d.pct).fold(0.0_f64, f64::max),
                settings.disk_alert,
                0,
            ),
        ];

        for (typ, value, threshold, sustain) in checks {
            let key = (s.host_id, typ);
            if value > threshold {
                let first = *self.first_exceed.entry(key).or_insert(now);
                if now - first < sustain {
                    continue;
                }
                // 已触发过且未恢复 / 在防抖窗口内 → 不重复触发
                let active = conn
                    .query_row(
                        "SELECT 1 FROM metrics_alerts WHERE host_id=?1 AND alert_type=?2 AND recovered_at IS NULL LIMIT 1",
                        params![s.host_id, typ_str(typ)],
                        |_| Ok(()),
                    )
                    .optional()?;
                if active.is_some() {
                    continue;
                }
                let last = conn
                    .query_row(
                        "SELECT recovered_at, muted, muted_until FROM metrics_alerts
                         WHERE host_id=?1 AND alert_type=?2 ORDER BY id DESC LIMIT 1",
                        params![s.host_id, typ_str(typ)],
                        |r| {
                            Ok((
                                r.get::<_, Option<String>>(0)?,
                                r.get::<_, i64>(1)?,
                                r.get::<_, Option<String>>(2)?,
                            ))
                        },
                    )
                    .optional()?;
                if let Some((recovered_at, muted, muted_until)) = last {
                    if muted != 0 {
                        if let Some(until) = muted_until {
                            let until_ts = until.parse_ts();
                            if until_ts > now {
                                continue; // 静音期内
                            }
                        } else {
                            continue; // 永久静音
                        }
                    }
                    if let Some(rec) = recovered_at {
                        if now - rec.parse_ts() < RE_ALERT_DEBOUNCE_SECS {
                            continue;
                        }
                    }
                }
                out.push(AlertDecision { alert_type: typ, value, threshold, sustained_secs: now - first });
            } else {
                // 恢复：解除首超状态，并落库 recovered_at（若有未恢复告警）
                self.first_exceed.remove(&key);
                conn.execute(
                    "UPDATE metrics_alerts SET recovered_at=?1
                     WHERE host_id=?2 AND alert_type=?3 AND recovered_at IS NULL",
                    params![bucket_str(now), s.host_id, typ_str(typ)],
                )?;
            }
        }
        Ok(out)
    }
}

pub fn typ_str(t: AlertType) -> &'static str {
    match t {
        AlertType::Cpu => "cpu",
        AlertType::Memory => "memory",
        AlertType::Swap => "swap",
        AlertType::Disk => "disk",
    }
}

pub fn typ_from_str(s: &str) -> AlertType {
    match s {
        "memory" => AlertType::Memory,
        "swap" => AlertType::Swap,
        "disk" => AlertType::Disk,
        _ => AlertType::Cpu,
    }
}

/// 落库一条新告警。
pub fn create_alert(conn: &Connection, host_id: i64, d: &AlertDecision, now: i64) -> Result<i64> {
    conn.execute(
        "INSERT INTO metrics_alerts (host_id, alert_type, value, threshold, triggered_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![host_id, typ_str(d.alert_type), d.value, d.threshold, bucket_str(now)],
    )?;
    Ok(conn.last_insert_rowid())
}

#[derive(Debug, Clone, Default)]
pub struct AlertFilter {
    pub host_id: Option<i64>,
    pub alert_type: Option<String>,
    pub only_active: bool,
    pub limit: i64,
    pub offset: i64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AlertRecord {
    pub id: i64,
    pub host_id: i64,
    pub alert_type: String,
    pub value: f64,
    pub threshold: f64,
    pub triggered_at: String,
    pub recovered_at: Option<String>,
    pub muted: bool,
    pub analysis_log: Option<String>,
}

pub fn list_alerts(conn: &Connection, f: &AlertFilter) -> Result<Vec<AlertRecord>> {
    let mut sql = "SELECT id, host_id, alert_type, value, threshold, triggered_at, recovered_at, muted, analysis_log
                   FROM metrics_alerts WHERE 1=1"
        .to_string();
    let mut args: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    if let Some(h) = f.host_id {
        sql.push_str(" AND host_id=?");
        args.push(Box::new(h));
    }
    if let Some(t) = &f.alert_type {
        sql.push_str(" AND alert_type=?");
        args.push(Box::new(t.clone()));
    }
    if f.only_active {
        sql.push_str(" AND recovered_at IS NULL");
    }
    sql.push_str(" ORDER BY id DESC");
    if f.limit > 0 {
        sql.push_str(" LIMIT ? OFFSET ?");
        args.push(Box::new(f.limit));
        args.push(Box::new(f.offset));
    }
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(args.iter()), |r| {
        Ok(AlertRecord {
            id: r.get(0)?,
            host_id: r.get(1)?,
            alert_type: r.get(2)?,
            value: r.get(3)?,
            threshold: r.get(4)?,
            triggered_at: r.get(5)?,
            recovered_at: r.get(6)?,
            muted: r.get::<_, i64>(7)? != 0,
            analysis_log: r.get(8)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// 静音告警：duration_secs 为 None 表示永久（F8.5）。
pub fn mute_alert(conn: &Connection, alert_id: i64, duration_secs: Option<i64>) -> Result<()> {
    let muted_until = duration_secs
        .map(|s| chrono::Utc::now().timestamp() + s)
        .map(bucket_str);
    conn.execute(
        "UPDATE metrics_alerts SET muted=1, muted_until=?1 WHERE id=?2",
        params![muted_until, alert_id],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_db;
    use tempfile::NamedTempFile;

    fn test_conn() -> Connection {
        let f = NamedTempFile::new().unwrap();
        let conn = Connection::open(f.path()).unwrap();
        init_db(&conn).unwrap();
        conn
    }

    fn snapshot(mem_pct: f64, cpu_pct: f64) -> MetricsSnapshot {
        MetricsSnapshot {
            host_id: 1,
            timestamp: 0,
            cpu: crate::schema::CpuSnapshot { total_pct: cpu_pct, ..Default::default() },
            memory: crate::schema::MemorySnapshot { pct: mem_pct, ..Default::default() },
            ..Default::default()
        }
    }

    #[test]
    fn mem_alert_triggers_and_debounces() {
        let conn = test_conn();
        let mut ev = AlertEvaluator::new();
        let now = 1_700_000_000i64;
        // 内存 95% > 90% → 立即触发
        let decs = ev.evaluate(&conn, &snapshot(95.0, 10.0), now).unwrap();
        assert_eq!(decs.len(), 1);
        assert_eq!(decs[0].alert_type, AlertType::Memory);
        create_alert(&conn, 1, &decs[0], now).unwrap();
        assert_eq!(list_alerts(&conn, &AlertFilter { only_active: true, ..Default::default() }).unwrap().len(), 1);

        // 仍超阈值 → 不重复触发（active 存在）
        let decs = ev.evaluate(&conn, &snapshot(96.0, 10.0), now + 10).unwrap();
        assert!(decs.is_empty());

        // 恢复 → 标记 recovered
        let _ = ev.evaluate(&conn, &snapshot(50.0, 10.0), now + 20).unwrap();
        let rec = list_alerts(&conn, &AlertFilter { only_active: true, ..Default::default() }).unwrap();
        assert!(rec.is_empty());

        // 5 分钟内再次超阈值 → 防抖不触发
        let decs = ev.evaluate(&conn, &snapshot(95.0, 10.0), now + 30).unwrap();
        assert!(decs.is_empty());

        // 5 分钟后 → 允许再触发
        let decs = ev.evaluate(&conn, &snapshot(95.0, 10.0), now + 400).unwrap();
        assert_eq!(decs.len(), 1);
    }

    #[test]
    fn cpu_requires_sustained_60s() {
        let conn = test_conn();
        let mut ev = AlertEvaluator::new();
        let now = 1_700_000_000i64;
        // 99% CPU 但只持续 30s → 不触发
        let decs = ev.evaluate(&conn, &snapshot(10.0, 99.0), now).unwrap();
        assert!(decs.is_empty());
        let decs = ev.evaluate(&conn, &snapshot(10.0, 99.0), now + 30).unwrap();
        assert!(decs.is_empty());
        // 持续到 60s → 触发
        let decs = ev.evaluate(&conn, &snapshot(10.0, 99.0), now + 60).unwrap();
        assert_eq!(decs.len(), 1);
        assert_eq!(decs[0].sustained_secs, 60);
    }

    #[test]
    fn minute_flush_and_hour_rollup() {
        let conn = test_conn();
        conn.execute("INSERT INTO hosts (name, host, username, auth_type) VALUES ('h','h','u','password')", [])
            .unwrap();
        let now = 1_700_000_000i64;
        let bucket = minute_bucket(now);
        let mut acc = MinuteAccumulator::new(bucket);
        for i in 0..4 {
            let mut s = snapshot(70.0 + i as f64, 20.0);
            s.timestamp = now + i;
            acc.add(&s);
        }
        flush_minute(&conn, 1, &acc).unwrap();
        let hist = get_history(&conn, 1, "mem", "1h", now).unwrap();
        assert_eq!(hist.len(), 1);
        assert!((hist[0].avg - 71.5).abs() < 0.1, "{}", hist[0].avg);
        assert_eq!(hist[0].max, 73.0);

        rollup_hour(&conn, 1, hour_bucket(now)).unwrap();
        let hist_h = get_history(&conn, 1, "mem", "30d", now).unwrap();
        assert_eq!(hist_h.len(), 1);
        assert!((hist_h[0].avg - 71.5).abs() < 0.1);
    }

    #[test]
    fn mute_works() {
        let conn = test_conn();
        let now0 = 1_700_000_100i64;
        let id = create_alert(&conn, 1, &AlertDecision {
            alert_type: AlertType::Cpu,
            value: 99.0,
            threshold: 90.0,
            sustained_secs: 60,
        }, now0)
        .unwrap();
        let mut ev = AlertEvaluator::new();
        let now = 1_700_000_000i64;
        // 先恢复，再静音后尝试触发
        let _ = ev.evaluate(&conn, &snapshot(10.0, 5.0), now).unwrap();
        mute_alert(&conn, id, Some(3600)).unwrap();
        let decs = ev.evaluate(&conn, &snapshot(10.0, 99.0), now + 400).unwrap();
        assert!(decs.is_empty(), "静音期内不应触发");
    }
}
