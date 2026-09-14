//! AI 命令执行审计（F6.2）：只增不改，可导出 CSV。

use rusqlite::{params, Connection};

use crate::error::Result;
use crate::schema::AuditEntry;

/// 记录一条审计（时间、主机、用户输入、生成命令、风险等级、是否执行、exit code 摘要）。
pub fn record(conn: &Connection, e: &AuditEntry) -> Result<i64> {
    conn.execute(
        "INSERT INTO audit_log (created_at, host_id, user_input, generated_cmd, risk_level,
                                executed, result_summary, memory_ids_used)
         VALUES (datetime('now'), ?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            e.host_id,
            e.user_input,
            e.generated_cmd,
            e.risk_level,
            e.executed as i64,
            e.result_summary,
            e.memory_ids_used
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// 审计日志只增不改：禁止 update/delete（本模块不提供相关函数）。

/// 导出全部审计为 CSV（RFC 4180 简化转义）。
pub fn export_csv(conn: &Connection) -> Result<String> {
    let mut stmt = conn.prepare(
        "SELECT id, created_at, host_id, user_input, generated_cmd, risk_level, executed, result_summary, memory_ids_used
         FROM audit_log ORDER BY id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(AuditEntry {
            id: Some(r.get(0)?),
            created_at: r.get(1)?,
            host_id: r.get(2)?,
            user_input: r.get(3)?,
            generated_cmd: r.get(4)?,
            risk_level: r.get(5)?,
            executed: r.get::<_, i64>(6)? != 0,
            result_summary: r.get(7)?,
            memory_ids_used: r.get(8)?,
        })
    })?;

    let mut out = String::from("id,created_at,host_id,user_input,generated_cmd,risk_level,executed,result_summary,memory_ids_used\n");
    for r in rows {
        let e = r?;
        out.push_str(&format!(
            "{},{},{},{},{},{},{},{},{}\n",
            e.id.unwrap_or_default(),
            csv_field(&e.created_at.unwrap_or_default()),
            e.host_id.unwrap_or_default(),
            csv_field(&e.user_input.unwrap_or_default()),
            csv_field(&e.generated_cmd),
            csv_field(&e.risk_level.unwrap_or_default()),
            e.executed as i64,
            csv_field(&e.result_summary.unwrap_or_default()),
            csv_field(&e.memory_ids_used.unwrap_or_default()),
        ));
    }
    Ok(out)
}

fn csv_field(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_db;
    use tempfile::NamedTempFile;

    #[test]
    fn append_only_and_csv() {
        let f = NamedTempFile::new().unwrap();
        let conn = Connection::open(f.path()).unwrap();
        init_db(&conn).unwrap();
        let e = AuditEntry {
            host_id: Some(1),
            user_input: Some("查看磁盘".into()),
            generated_cmd: "df -h".into(),
            risk_level: Some("low".into()),
            executed: true,
            result_summary: Some("exit 0".into()),
            memory_ids_used: None,
            ..Default::default()
        };
        record(&conn, &e).unwrap();
        record(&conn, &e).unwrap();
        let csv = export_csv(&conn).unwrap();
        assert_eq!(csv.lines().count(), 3); // header + 2 rows
        assert!(csv.contains("df -h"));
        assert!(csv.contains("exit 0"));
    }

    #[test]
    fn csv_escapes() {
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("he said \"hi\""), "\"he said \"\"hi\"\"\"");
    }
}
