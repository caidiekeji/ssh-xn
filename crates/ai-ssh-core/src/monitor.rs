//! 资源采集器解析（F8）：解析远端采集命令输出 → MetricsSnapshot。
//! - 通过已建立 SSH 连接执行采集命令（复用 russh exec channel，独立于交互 PTY，实现注意事项 #10）
//! - CPU/网络速率为差值型指标，首次采样只记录基线不输出速率（实现注意事项 #11）

use crate::error::{Error, Result};
use crate::schema::{
    CpuSnapshot, DiskSnapshot, LoadSnapshot, MemorySnapshot, MetricsSnapshot, NetSnapshot,
    ProcessSnapshot, ProcessTable,
};

/// Linux 采集命令：单次执行一次性输出全部指标（F8.1）。
pub fn linux_collect_cmd() -> &'static str {
    r#"{ cat /proc/stat; echo '__SEP_LOAD__'; cat /proc/loadavg; echo '__SEP_MEM__'; cat /proc/meminfo; echo '__SEP_DF__'; df -Pk; echo '__SEP_NET__'; cat /proc/net/dev; echo '__SEP_PSCPU__'; ps -eo pid,user,pcpu,pmem,comm --sort=-pcpu --no-headers 2>/dev/null | head -10; echo '__SEP_PSMEM__'; ps -eo pid,user,pcpu,pmem,comm --sort=-pmem --no-headers 2>/dev/null | head -10; echo '__SEP_UPTIME__'; cat /proc/uptime; } 2>&1"#
}

/// macOS 采集命令（F8.1 系统探测的 macOS 分支）。
pub fn macos_collect_cmd() -> &'static str {
    r#"{ sysctl -n hw.ncpu; echo '__SEP_LOAD__'; sysctl -n vm.loadavg; echo '__SEP_MEM__'; vm_stat; echo '__SEP_DF__'; df -Pk; echo '__SEP_PS__'; ps -A -o pid,user,%cpu,%mem,comm 2>/dev/null | head -21; echo '__SEP_UPTIME__'; uptime; } 2>&1"#
}

/// 分隔符（与采集命令保持一致）。
pub const SEP_STAT: &str = "__STAT__";
pub const SEP_LOAD: &str = "__SEP_LOAD__";
pub const SEP_MEM: &str = "__SEP_MEM__";
pub const SEP_DF: &str = "__SEP_DF__";
pub const SEP_NET: &str = "__SEP_NET__";
pub const SEP_PSCPU: &str = "__SEP_PSCPU__";
pub const SEP_PSMEM: &str = "__SEP_PSMEM__";
pub const SEP_UPTIME: &str = "__SEP_UPTIME__";

/// 将组合命令输出拆段。首个分段（/proc/stat）归入 SEP_STAT 键。
pub fn split_sections(raw: &str) -> std::collections::HashMap<&'static str, String> {
    let mut map = std::collections::HashMap::new();
    let mut current: &'static str = SEP_STAT;
    let mut buf = String::new();
    for line in raw.lines() {
        let seg = match line {
            "__SEP_LOAD__" => Some(SEP_LOAD),
            "__SEP_MEM__" => Some(SEP_MEM),
            "__SEP_DF__" => Some(SEP_DF),
            "__SEP_NET__" => Some(SEP_NET),
            "__SEP_PSCPU__" => Some(SEP_PSCPU),
            "__SEP_PSMEM__" => Some(SEP_PSMEM),
            "__SEP_UPTIME__" => Some(SEP_UPTIME),
            _ => None,
        };
        if let Some(s) = seg {
            if !current.is_empty() {
                map.insert(current, std::mem::take(&mut buf));
            }
            current = s;
        } else {
            buf.push_str(line);
            buf.push('\n');
        }
    }
    if !current.is_empty() {
        map.insert(current, buf);
    }
    map
}

// ============ CPU（/proc/stat） ============

/// CPU 原始计数（差值计算用）。
#[derive(Debug, Clone, Default)]
pub struct CpuRaw {
    pub total: u64,
    pub idle: u64,
    pub iowait: u64,
    pub user: u64,
    pub system: u64,
}

/// 解析 /proc/stat 第一行（cpu 聚合行）。
pub fn parse_proc_stat(raw: &str) -> Option<CpuRaw> {
    let line = raw.lines().find(|l| l.starts_with("cpu "))?;
    let mut it = line.split_whitespace();
    it.next()?;
    let nums: Vec<u64> = it.filter_map(|v| v.parse().ok()).collect();
    if nums.len() < 4 {
        return None;
    }
    let user = nums[0];
    let nice = nums[1];
    let system = nums[2];
    let idle = nums[3];
    let iowait = nums.get(4).copied().unwrap_or(0);
    let irq = nums.get(5).copied().unwrap_or(0);
    let softirq = nums.get(6).copied().unwrap_or(0);
    let steal = nums.get(7).copied().unwrap_or(0);
    let total = user + nice + system + idle + iowait + irq + softirq + steal;
    Some(CpuRaw {
        total,
        idle,
        iowait,
        user: user + nice,
        system: system + irq + softirq,
    })
}

pub fn cpu_snapshot(prev: Option<&CpuRaw>, cur: &CpuRaw, cores: u32) -> CpuSnapshot {
    let mut s = CpuSnapshot { cores, ..Default::default() };
    if let Some(p) = prev {
        let d_total = cur.total.saturating_sub(p.total);
        if d_total > 0 {
            let d_idle = cur.idle.saturating_sub(p.idle);
            let d_iowait = cur.iowait.saturating_sub(p.iowait);
            let d_user = cur.user.saturating_sub(p.user);
            let d_sys = cur.system.saturating_sub(p.system);
            let busy = d_total.saturating_sub(d_idle);
            s.total_pct = busy as f64 * 100.0 / d_total as f64;
            s.user_pct = d_user as f64 * 100.0 / d_total as f64;
            s.sys_pct = d_sys as f64 * 100.0 / d_total as f64;
            s.iowait_pct = d_iowait as f64 * 100.0 / d_total as f64;
        }
    }
    s
}

/// 从 `nproc` / `sysctl hw.ncpu` 读取核心数（本地；远端核心数随 ps/uptime 等一并返回）。
pub fn local_cores() -> u32 {
    std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(1)
}

// ============ 负载 ============

pub fn parse_loadavg(raw: &str) -> Option<LoadSnapshot> {
    let line = raw.lines().next()?;
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 3 {
        return None;
    }
    Some(LoadSnapshot {
        load1: parts[0].parse().ok()?,
        load5: parts[1].parse().ok()?,
        load15: parts[2].parse().ok()?,
    })
}

// ============ 内存（/proc/meminfo） ============

pub fn parse_meminfo(raw: &str) -> Option<MemorySnapshot> {
    let mut kb = |key: &str| -> Option<u64> {
        raw.lines()
            .find(|l| l.starts_with(key))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|v| v.parse().ok())
    };
    let total_kb = kb("MemTotal:")?;
    let free_kb = kb("MemFree:").unwrap_or(0);
    let buffers_kb = kb("Buffers:").unwrap_or(0);
    let cached_kb = kb("Cached:").unwrap_or(0);
    let sreclaimable_kb = kb("SReclaimable:").unwrap_or(0);
    // 老内核无 MemAvailable 时回退到 free+buffers+cache 近似
    let available_kb = kb("MemAvailable:").unwrap_or_else(|| free_kb + buffers_kb + cached_kb + sreclaimable_kb);
    let swap_total_kb = kb("SwapTotal:").unwrap_or(0);
    let swap_free_kb = kb("SwapFree:").unwrap_or(0);

    let used_kb = total_kb.saturating_sub(available_kb);
    let buffers_cached = buffers_kb + cached_kb + sreclaimable_kb;
    let swap_used = swap_total_kb.saturating_sub(swap_free_kb);
    Some(MemorySnapshot {
        total_mb: total_kb / 1024,
        used_mb: used_kb / 1024,
        available_mb: available_kb / 1024,
        pct: if total_kb > 0 { used_kb as f64 * 100.0 / total_kb as f64 } else { 0.0 },
        buffers_cached_mb: buffers_cached / 1024,
        swap_total_mb: swap_total_kb / 1024,
        swap_used_mb: swap_used / 1024,
        swap_pct: if swap_total_kb > 0 { swap_used as f64 * 100.0 / swap_total_kb as f64 } else { 0.0 },
    })
}

// ============ 磁盘（df -Pk） ============

pub fn parse_df(raw: &str) -> Vec<DiskSnapshot> {
    let mut out = Vec::new();
    for line in raw.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        // df -Pk: Filesystem 1024-blocks Used Available Capacity Mounted on
        if parts.len() < 6 {
            continue;
        }
        // 伪文件系统（tmpfs/overlay 等）不监控，避免 /run、/dev/shm 噪音
        let fsname = parts[0];
        if matches!(fsname, "tmpfs" | "devtmpfs" | "overlay" | "proc" | "sysfs" | "cgroup" | "cgroup2" | "devfs" | "udev" | "shm" | "none") {
            continue;
        }
        let mount = parts[5..].join(" ");
        let total_kb: f64 = match parts[1].parse() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let used_kb: f64 = match parts[2].parse() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let pct = parts[4].trim_end_matches('%').parse().unwrap_or_else(|_| {
            if total_kb > 0.0 { used_kb * 100.0 / total_kb } else { 0.0 }
        });
        if mount.starts_with('/') || mount == "/" {
            out.push(DiskSnapshot {
                mount,
                total_gb: total_kb / 1024.0 / 1024.0,
                used_gb: used_kb / 1024.0 / 1024.0,
                pct,
            });
        }
    }
    out
}

// ============ 网络（/proc/net/dev） ============

#[derive(Debug, Clone, Default)]
pub struct NetRaw {
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub errors: u64,
    pub dropped: u64,
}

pub fn parse_proc_net_dev(raw: &str) -> Vec<(String, NetRaw)> {
    let mut out = Vec::new();
    for line in raw.lines().skip(2) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let (iface, rest) = match line.split_once(':') {
            Some((i, r)) => (i.trim().to_string(), r.trim()),
            None => continue,
        };
        let nums: Vec<u64> = rest.split_whitespace().filter_map(|v| v.parse().ok()).collect();
        if nums.len() < 10 {
            continue;
        }
        out.push((
            iface,
            NetRaw {
                rx_bytes: nums[0],
                tx_bytes: nums[8],
                errors: nums[2] + nums[10],
                dropped: nums[3] + nums[11],
            },
        ));
    }
    out
}

/// 由两次采样计算速率（bytes/s）。首次采样只记录基线，返回 None。
pub fn net_snapshot(
    prev: Option<&[(String, NetRaw)]>,
    cur: &[(String, NetRaw)],
    dt_secs: f64,
) -> Vec<NetSnapshot> {
    let prev_map: std::collections::HashMap<&str, &NetRaw> = prev
        .map(|p| p.iter().map(|(i, r)| (i.as_str(), r)).collect())
        .unwrap_or_default();
    cur.iter()
        .map(|(iface, raw)| {
            let (rx_bps, tx_bps) = match prev_map.get(iface.as_str()) {
                Some(p) if dt_secs > 0.0 => (
                    raw.rx_bytes.saturating_sub(p.rx_bytes) as f64 / dt_secs,
                    raw.tx_bytes.saturating_sub(p.tx_bytes) as f64 / dt_secs,
                ),
                _ => (0.0, 0.0),
            };
            NetSnapshot {
                iface: iface.clone(),
                rx_bps,
                tx_bps,
                rx_bytes_total: raw.rx_bytes,
                tx_bytes_total: raw.tx_bytes,
                errors: raw.errors,
                dropped: raw.dropped,
            }
        })
        .collect()
}

// ============ 进程（ps） ============

pub fn parse_ps(raw: &str) -> Vec<ProcessSnapshot> {
    let mut out = Vec::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut it = line.split_whitespace();
        let pid: u32 = match it.next().and_then(|v| v.parse().ok()) {
            Some(p) => p,
            None => continue,
        };
        let user = it.next().unwrap_or("?").to_string();
        let cpu_pct: f64 = match it.next().and_then(|v| v.parse().ok()) {
            Some(c) => c,
            None => continue,
        };
        let mem_pct: f64 = match it.next().and_then(|v| v.parse().ok()) {
            Some(m) => m,
            None => continue,
        };
        let command: String = it.collect::<Vec<_>>().join(" ");
        out.push(ProcessSnapshot { pid, user, cpu_pct, mem_pct, command });
    }
    out
}

pub fn process_table(cpu_raw: &str, mem_raw: &str) -> ProcessTable {
    let mut by_cpu = parse_ps(cpu_raw);
    let mut by_mem = parse_ps(mem_raw);
    by_cpu.sort_by(|a, b| b.cpu_pct.partial_cmp(&a.cpu_pct).unwrap_or(std::cmp::Ordering::Equal));
    by_mem.sort_by(|a, b| b.mem_pct.partial_cmp(&a.mem_pct).unwrap_or(std::cmp::Ordering::Equal));
    by_cpu.truncate(10);
    by_mem.truncate(10);
    ProcessTable { by_cpu, by_mem }
}

// ============ 运行时间 ============

pub fn parse_uptime_sec(raw: &str) -> Option<u64> {
    raw.split_whitespace().next()?.parse::<f64>().ok().map(|v| v as u64)
}

/// 拼接 Linux 采集结果（实现注意事项 #11：首次调用传入 prev 为 None 记录基线）。
pub fn assemble_linux_snapshot(
    host_id: i64,
    timestamp: i64,
    raw: &str,
    prev_cpu: Option<&CpuRaw>,
    prev_net: Option<&[(String, NetRaw)]>,
    dt_secs: f64,
    cores: u32,
) -> Result<MetricsSnapshot> {
    let sections = split_sections(raw);
    let cur_cpu = parse_proc_stat(sections.get(SEP_STAT).map(|s| s.as_str()).unwrap_or(""))
        .ok_or_else(|| Error::Monitor("无法解析 /proc/stat".into()))?;
    let cur_net = parse_proc_net_dev(sections.get(SEP_NET).map(|s| s.as_str()).unwrap_or(""));
    Ok(MetricsSnapshot {
        host_id,
        timestamp,
        cpu: cpu_snapshot(prev_cpu, &cur_cpu, cores),
        load: parse_loadavg(sections.get(SEP_LOAD).map(|s| s.as_str()).unwrap_or("")).unwrap_or_default(),
        memory: parse_meminfo(sections.get(SEP_MEM).map(|s| s.as_str()).unwrap_or("")).unwrap_or_default(),
        disks: parse_df(sections.get(SEP_DF).map(|s| s.as_str()).unwrap_or("")),
        network: net_snapshot(prev_net, &cur_net, dt_secs),
        processes: process_table(
            sections.get(SEP_PSCPU).map(|s| s.as_str()).unwrap_or(""),
            sections.get(SEP_PSMEM).map(|s| s.as_str()).unwrap_or(""),
        ),
        uptime_sec: parse_uptime_sec(sections.get(SEP_UPTIME).map(|s| s.as_str()).unwrap_or("")).unwrap_or(0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROC_STAT: &str = "cpu  1000 10 800 5000 300 50 40 0 0 0\ncpu0 500 5 400 2500 150 25 20 0 0 0\n";
    const PROC_STAT2: &str = "cpu  2000 20 1600 7000 600 100 80 0 0 0\n";

    #[test]
    fn cpu_delta() {
        let a = parse_proc_stat(PROC_STAT).unwrap();
        let b = parse_proc_stat(PROC_STAT2).unwrap();
        let s = cpu_snapshot(Some(&a), &b, 8);
        assert!(s.total_pct > 50.0 && s.total_pct < 70.0, "{}", s.total_pct);
        assert_eq!(s.cores, 8);
        // 首次采样无 prev → 0 基线
        let s0 = cpu_snapshot(None, &a, 8);
        assert_eq!(s0.total_pct, 0.0);
    }

    #[test]
    fn loadavg() {
        let l = parse_loadavg("0.52 0.48 0.45 1/123 4567").unwrap();
        assert!((l.load1 - 0.52).abs() < 1e-9);
    }

    #[test]
    fn meminfo() {
        let raw = "MemTotal:       16384000 kB\nMemFree:         2000000 kB\nMemAvailable:    4000000 kB\nBuffers:          500000 kB\nCached:          1000000 kB\nSReclaimable:     100000 kB\nSwapTotal:       2097152 kB\nSwapFree:        2000000 kB\n";
        let m = parse_meminfo(raw).unwrap();
        assert_eq!(m.total_mb, 16000);
        assert!((m.pct - 75.0).abs() < 1.0, "{}", m.pct);
        assert_eq!(m.buffers_cached_mb, 1562);
        assert_eq!(m.swap_used_mb, 94);
    }

    #[test]
    fn df() {
        let raw = "Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/sda1 10000000 8500000 1500000 85% /\ntmpfs 1000000 1000 999000 1% /run\n";
        let d = parse_df(raw);
        assert_eq!(d.len(), 1);
        assert!((d[0].pct - 85.0).abs() < 1e-6);
        assert!((d[0].total_gb - 10000000.0 / 1024.0 / 1024.0).abs() < 1e-6);
    }

    #[test]
    fn net_dev() {
        let raw = "Inter-|   Receive                                                |  Transmit\n face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed\n  eth0: 100000 1000 0 0 0 0 0 0 200000 1000 0 0 0 0 0 0\n  lo: 1000 10 0 0 0 0 0 0 1000 10 0 0 0 0 0 0\n";
        let cur = parse_proc_net_dev(raw);
        let raw2 = raw.replace("100000", "130000").replace("200000", "260000");
        let cur2 = parse_proc_net_dev(&raw2);
        let s = net_snapshot(Some(&cur), &cur2, 2.0);
        assert_eq!(s.len(), 2);
        let eth = s.iter().find(|n| n.iface == "eth0").unwrap();
        assert!((eth.rx_bps - 15000.0).abs() < 1.0, "{}", eth.rx_bps);
        assert!((eth.tx_bps - 30000.0).abs() < 1.0);
        // 首次采样无 prev → 0 速率
        let s0 = net_snapshot(None, &cur, 2.0);
        assert!(s0.iter().all(|n| n.rx_bps == 0.0));
    }

    #[test]
    fn ps() {
        let raw = "1234 www 80.1 12.3 node app.js\n5678 mysql 5.0 30.2 mysqld --datadir=/var/lib/mysql\n";
        let p = parse_ps(raw);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].pid, 1234);
        assert!(p[1].command.contains("--datadir"));
    }

    #[test]
    fn process_table_sorts_and_truncates() {
        let raw = "1 root 1.0 2.0 init\n2 root 99.0 1.0 busy\n";
        let t = process_table(raw, raw);
        assert_eq!(t.by_cpu[0].pid, 2);
        assert_eq!(t.by_cpu[1].pid, 1);
    }

    #[test]
    fn uptime() {
        assert_eq!(parse_uptime_sec("864000.12 123.4"), Some(864000));
    }

    #[test]
    fn assemble_full_linux() {
        let raw = format!(
            "{PROC_STAT}\n{SEP_LOAD}\n1.5 1.2 1.0 1/100 2000\n{SEP_MEM}\nMemTotal: 8388608 kB\nMemFree: 1000000 kB\nMemAvailable: 2000000 kB\nBuffers: 0 kB\nCached: 0 kB\nSReclaimable: 0 kB\nSwapTotal: 0 kB\nSwapFree: 0 kB\n{SEP_DF}\nFilesystem 1024-blocks Used Available Capacity Mounted on\n/dev/sda1 10000000 8500000 1500000 85% /\n{SEP_NET}\nInter-|   Receive |  Transmit\n face |bytes |  eth0: 5000 1 0 0 0 0 0 0 8000 1 0 0 0 0 0 0\n{SEP_PSCPU}\n100 root 9.9 1.0 app\n{SEP_PSMEM}\n200 user 1.0 9.9 other\n{SEP_UPTIME}\n3600.0 0.0\n"
        );
        let s = assemble_linux_snapshot(1, 1700000000, &raw, None, None, 3.0, 4).unwrap();
        assert_eq!(s.host_id, 1);
        assert_eq!(s.load.load1, 1.5);
        assert!((s.memory.pct - 76.16).abs() < 0.5, "pct={}", s.memory.pct);
        assert_eq!(s.disks.len(), 1);
        assert_eq!(s.processes.by_cpu.len(), 1);
        assert_eq!(s.uptime_sec, 3600);
    }
}
