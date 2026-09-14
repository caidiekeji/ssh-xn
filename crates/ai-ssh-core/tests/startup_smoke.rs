//! 启动链路冒烟测试：模拟 lib.rs run() 的启动序列（建库 → 初始化 → 设置 → 清理），
//! 验证首次启动与二次启动均无运行时 panic（修复"界面打开闪退"排查用）。

use ai_ssh_core::db;
use rusqlite::Connection;

fn fresh_db(name: &str) -> (std::path::PathBuf, Connection) {
    let dir = std::env::temp_dir().join(format!("ai-ssh-startup-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("ai-ssh.db");
    (dir, Connection::open(&path).expect("打开数据库失败"))
}

#[test]
fn first_startup_sequence() {
    let (dir, conn) = fresh_db("first");
    // 与 lib.rs run() 完全一致的启动序列
    db::init_db(&conn).expect("初始化数据库失败");
    let settings = db::get_monitor_settings(&conn).expect("读取监控设置失败");
    assert_eq!(settings.interval_sec, 3);
    assert_eq!(settings.cpu_alert, 90.0);
    db::cleanup_expired(&conn).expect("清理过期数据失败");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn second_startup_idempotent() {
    let (dir, conn) = fresh_db("second");
    db::init_db(&conn).unwrap();
    // 二次启动（模拟应用重启）：表已存在，必须幂等
    let conn2 = Connection::open(dir.join("ai-ssh.db")).unwrap();
    db::init_db(&conn2).expect("二次初始化失败（非幂等）");
    // 所有 PRD 表都应可查
    for t in [
        "hosts", "llm_providers", "llm_routing", "memory_cases", "session_context",
        "command_history", "metrics_history", "metrics_alerts", "audit_log",
        "token_usage", "monitor_settings",
    ] {
        let n: i64 = conn2
            .query_row(&format!("SELECT COUNT(*) FROM {}", t), [], |r| r.get(0))
            .unwrap_or_else(|e| panic!("表 {} 查询失败: {}", t, e));
        assert!(n >= 0, "表 {} 不可用", t);
    }
    // 模拟 hosts 写读（crypto 依赖设备指纹，仅测 SQL 层）
    conn2
        .execute(
            "INSERT INTO hosts (name, host, port, username, auth_type, memory_enabled, monitor_enabled, created_at)
             VALUES ('测试机','127.0.0.1',22,'root','password',1,1,datetime('now'))",
            [],
        )
        .unwrap();
    let cnt: i64 = conn2.query_row("SELECT COUNT(*) FROM hosts", [], |r| r.get(0)).unwrap();
    assert_eq!(cnt, 1);
    db::cleanup_expired(&conn2).unwrap();
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn monitor_settings_upsert() {
    let (dir, conn) = fresh_db("settings");
    db::init_db(&conn).unwrap();
    let s = db::MonitorSettings { interval_sec: 5, cpu_alert: 85.0, mem_alert: 88.0, swap_alert: 70.0, disk_alert: 92.0, notification_mode: "app".into() };
    db::update_monitor_settings(&conn, &s).unwrap();
    let got = db::get_monitor_settings(&conn).unwrap();
    assert_eq!(got.interval_sec, 5);
    assert_eq!(got.cpu_alert, 85.0);
    std::fs::remove_dir_all(&dir).ok();
}
