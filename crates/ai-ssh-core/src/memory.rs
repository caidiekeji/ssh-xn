//! 历史问题记录（F5，简化版）：SQLite 存储 + 简单关键词检索。
//! 不依赖向量数据库、嵌入模型或 FTS5（PRD v1.2.0 精简记忆库方案）。

use rusqlite::{params, Connection, Row};

use crate::error::{Error, Result};
use crate::schema::{CaseSource, MemoryCase};

const COLS: &str = "id, created_at, updated_at, host_id, os_info, problem_type, error_snippet,
 description, keywords, root_cause, solution_cmd, solution_text, verify_cmd, rollback_cmd,
 hit_count, verified, source, failed_for";

fn row_to_case(r: &Row) -> rusqlite::Result<MemoryCase> {
    let source: String = r.get(16)?;
    Ok(MemoryCase {
        id: r.get(0)?,
        created_at: r.get(1)?,
        updated_at: r.get(2)?,
        host_id: r.get(3)?,
        os_info: r.get(4)?,
        problem_type: r.get(5)?,
        error_snippet: r.get(6)?,
        description: r.get(7)?,
        keywords: r.get(8)?,
        root_cause: r.get(9)?,
        solution_cmd: r.get(10)?,
        solution_text: r.get(11)?,
        verify_cmd: r.get(12)?,
        rollback_cmd: r.get(13)?,
        hit_count: r.get(14)?,
        verified: r.get::<_, i64>(15)? != 0,
        source: match source.as_str() {
            "ai_resolved" => CaseSource::AiResolved,
            "imported" => CaseSource::Imported,
            _ => CaseSource::UserManual,
        },
        failed_for: r.get(17)?,
    })
}

/// 保存一条记录；有 id 则更新，否则插入。返回 id。
pub fn save_case(conn: &Connection, case: &MemoryCase) -> Result<i64> {
    if let Some(id) = case.id {
        conn.execute(
            "UPDATE memory_cases SET updated_at=datetime('now'),
               host_id=?1, os_info=?2, problem_type=?3, error_snippet=?4, description=?5,
               keywords=?6, root_cause=?7, solution_cmd=?8, solution_text=?9, verify_cmd=?10,
               rollback_cmd=?11, verified=?12, source=?13, failed_for=?14
             WHERE id=?15",
            params![
                case.host_id, case.os_info, case.problem_type, case.error_snippet, case.description,
                case.keywords, case.root_cause, case.solution_cmd, case.solution_text, case.verify_cmd,
                case.rollback_cmd, case.verified as i64, source_str(case.source), case.failed_for, id
            ],
        )?;
        Ok(id)
    } else {
        conn.execute(
            "INSERT INTO memory_cases
             (created_at, updated_at, host_id, os_info, problem_type, error_snippet, description,
              keywords, root_cause, solution_cmd, solution_text, verify_cmd, rollback_cmd,
              hit_count, verified, source, failed_for)
             VALUES (datetime('now'), datetime('now'), ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 0, ?12, ?13, ?14)",
            params![
                case.host_id, case.os_info, case.problem_type, case.error_snippet, case.description,
                case.keywords, case.root_cause, case.solution_cmd, case.solution_text, case.verify_cmd,
                case.rollback_cmd, case.verified as i64, source_str(case.source), case.failed_for
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }
}

fn source_str(s: CaseSource) -> &'static str {
    match s {
        CaseSource::AiResolved => "ai_resolved",
        CaseSource::UserManual => "user_manual",
        CaseSource::Imported => "imported",
    }
}

pub fn get_case(conn: &Connection, id: i64) -> Result<MemoryCase> {
    conn.query_row(
        &format!("SELECT {COLS} FROM memory_cases WHERE id=?1"),
        params![id],
        row_to_case,
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => Error::NotFound(format!("历史记录 {id}")),
        other => other.into(),
    })
}

/// 列表查询（F5.7）：支持按主机/问题类型/时间/verified 筛选 + 全文关键词过滤。
#[derive(Debug, Default, Clone)]
pub struct CaseFilter {
    pub host_id: Option<i64>,
    pub problem_type: Option<String>,
    pub verified: Option<bool>,
    pub search: Option<String>,
    pub limit: i64,
    pub offset: i64,
}

pub fn list_cases(conn: &Connection, f: &CaseFilter) -> Result<Vec<MemoryCase>> {
    let mut sql = format!("SELECT {COLS} FROM memory_cases WHERE 1=1");
    let mut args: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    if let Some(h) = f.host_id {
        sql.push_str(" AND host_id=?");
        args.push(Box::new(h));
    }
    if let Some(pt) = &f.problem_type {
        sql.push_str(" AND problem_type=?");
        args.push(Box::new(pt.clone()));
    }
    if let Some(v) = f.verified {
        sql.push_str(" AND verified=?");
        args.push(Box::new(v as i64));
    }
    if let Some(s) = &f.search {
        let kw = tokenize_keywords(s);
        if !kw.is_empty() {
            sql.push_str(" AND (");
            for (i, k) in kw.iter().enumerate() {
                if i > 0 {
                    sql.push_str(" OR ");
                }
                let like = format!("%{}%", escape_like(k));
                sql.push_str(
                    "description LIKE ? ESCAPE '\\' OR error_snippet LIKE ? ESCAPE '\\' \
                     OR keywords LIKE ? ESCAPE '\\' OR root_cause LIKE ? ESCAPE '\\' OR solution_text LIKE ? ESCAPE '\\'",
                );
                for _ in 0..5 {
                    args.push(Box::new(like.clone()));
                }
            }
            sql.push(')');
        }
    }
    sql.push_str(" ORDER BY created_at DESC, id DESC");
    if f.limit > 0 {
        sql.push_str(" LIMIT ? OFFSET ?");
        args.push(Box::new(f.limit));
        args.push(Box::new(f.offset));
    }
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(args.iter()), row_to_case)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// F5.2 简单检索：同主机优先 → 最近 50 条 → 关键词 LIKE 匹配计数排序 → Top N；
/// 无匹配回退同主机最近 3 条。
pub fn search_cases(conn: &Connection, host_id: i64, query: &str, limit: usize) -> Result<Vec<MemoryCase>> {
    let keywords = tokenize_keywords(query);
    let recent = recent_by_host(conn, host_id, 50)?;

    if keywords.is_empty() {
        return Ok(recent.into_iter().take(limit).collect());
    }

    // 多关键词查询要求至少命中 2 个词（避免单词碰巧命中误导）；单关键词 1 个即可
    let min_score = if keywords.len() >= 2 { 2 } else { 1 };
    let mut scored: Vec<(i64, MemoryCase)> = Vec::new();
    for case in recent {
        let score = count_matches(&case, &keywords);
        if score >= min_score {
            scored.push((score, case));
        }
    }
    // 匹配关键词数量降序，稳定保持 created_at DESC 的原始顺序
    scored.sort_by(|a, b| b.0.cmp(&a.0));

    if !scored.is_empty() {
        return Ok(scored.into_iter().take(limit).map(|(_, c)| c).collect());
    }
    // 回退：同主机最近 3 条
    Ok(recent_by_host(conn, host_id, 3)?.into_iter().take(limit).collect())
}

fn recent_by_host(conn: &Connection, host_id: i64, n: i64) -> Result<Vec<MemoryCase>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM memory_cases WHERE host_id=?1 ORDER BY created_at DESC, id DESC LIMIT ?2"
    ))?;
    let rows = stmt.query_map(params![host_id, n], row_to_case)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// 关键词切分：按空格/标点切分；CJK 连续串再拆为二元组（让中文检索可用）。
pub fn tokenize_keywords(input: &str) -> Vec<String> {
    let mut raw: Vec<String> = Vec::new();
    for part in input.split(|c: char| c.is_whitespace() || c.is_ascii_punctuation() || c.is_ascii_control()) {
        if part.is_empty() {
            continue;
        }
        raw.push(part.to_lowercase());
    }
    let mut out: Vec<String> = Vec::new();
    for tok in raw {
        if tok.chars().any(is_cjk) {
            let chars: Vec<char> = tok.chars().collect();
            if chars.len() <= 1 {
                out.push(tok.clone());
            } else {
                for w in chars.windows(2) {
                    let bigram: String = w.iter().collect();
                    if !out.contains(&bigram) {
                        out.push(bigram);
                    }
                }
            }
        } else if !out.contains(&tok) {
            out.push(tok);
        }
    }
    out
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x3000..=0x303F | 0xFF00..=0xFFEF)
}

/// LIKE 通配符转义。
pub fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

fn count_matches(case: &MemoryCase, keywords: &[String]) -> i64 {
    let haystacks = [
        case.description.as_str(),
        case.error_snippet.as_deref().unwrap_or(""),
        case.keywords.as_deref().unwrap_or(""),
        case.root_cause.as_deref().unwrap_or(""),
        case.solution_text.as_deref().unwrap_or(""),
    ];
    let mut n = 0;
    for kw in keywords {
        let needle = escape_like(kw);
        if haystacks.iter().any(|h| h.to_lowercase().contains(&needle)) {
            n += 1;
        }
    }
    n
}

/// 命中反馈（F5.7）：👍 +2，👎 -1（下限 0）。
pub fn feedback(conn: &Connection, id: i64, up: bool) -> Result<()> {
    let delta: i64 = if up { 2 } else { -1 };
    conn.execute(
        "UPDATE memory_cases SET hit_count = MAX(0, hit_count + ?1) WHERE id=?2",
        params![delta, id],
    )?;
    Ok(())
}

/// 方案失效标记（F5.7）：记录 failed_for 备注，检索时降权。
pub fn mark_failed(conn: &Connection, id: i64, note: &str) -> Result<()> {
    conn.execute(
        "UPDATE memory_cases SET failed_for=?1 WHERE id=?2",
        params![note, id],
    )?;
    Ok(())
}

pub fn delete_case(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM memory_cases WHERE id=?1", params![id])?;
    Ok(())
}

// ============ Markdown 导入导出（F5.6） ============

/// 解析单条记忆文档（front matter + 问题/根因/解决/验证 区块）。
pub fn parse_memory_markdown(text: &str) -> Result<MemoryCase> {
    let mut case = MemoryCase::default();
    case.source = CaseSource::Imported;

    let body = if let Some(stripped) = text.strip_prefix("---") {
        let end = stripped.find("\n---").ok_or_else(|| Error::Parse("front matter 未闭合".into()))?;
        let fm = &stripped[..end];
        for line in fm.lines() {
            let line = line.trim();
            if let Some(v) = line.strip_prefix("os:") {
                case.os_info = Some(v.trim().to_string());
            }
            if let Some(v) = line.strip_prefix("type:") {
                let _ = v.trim(); // 类型标识，忽略
            }
            if let Some(v) = line.strip_prefix("tags:") {
                case.keywords = Some(v.trim().trim_matches(['[', ']']).replace(',', " "));
            }
        }
        &stripped[end + 4..]
    } else {
        text
    };

    let mut sections: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut current = String::new();
    for line in body.lines() {
        let trimmed = line.trim();
        // PRD F5.6 中「验证」可写为裸标题（无 ##），与 ## 标题同等对待
        if let Some(header) = line.strip_prefix("## ") {
            current = header.trim().to_string();
            continue;
        }
        if trimmed == "验证" {
            current = "验证".to_string();
            continue;
        }
        sections.entry(current.clone()).or_default().push_str(line);
        sections.get_mut(&current).unwrap().push('\n');
    }

    case.description = sections.get("问题").cloned().unwrap_or_default().trim().to_string();
    case.root_cause = sections.get("根因").cloned().map(|v| v.trim().to_string());

    let mut cmds: Vec<String> = Vec::new();
    let mut text_parts: Vec<String> = Vec::new();
    let mut verify = String::new();
    for section in ["解决", "验证"] {
        let content = sections.get(section).cloned().unwrap_or_default();
        // 提取 bash 代码块
        let mut in_block = false;
        let mut buf = String::new();
        for line in content.lines() {
            if line.trim().starts_with("```") {
                if in_block {
                    let cmd = buf.trim();
                    if !cmd.is_empty() {
                        if section == "验证" {
                            verify = cmd.to_string();
                        } else {
                            cmds.push(cmd.to_string());
                        }
                    }
                    buf.clear();
                    in_block = false;
                } else {
                    in_block = true;
                }
                continue;
            }
            if in_block {
                buf.push_str(line);
                buf.push('\n');
            } else {
                let t = line.trim();
                if !t.is_empty() {
                    text_parts.push(t.to_string());
                }
            }
        }
    }
    if !cmds.is_empty() {
        case.solution_cmd = Some(cmds.join("\n"));
    }
    if !verify.is_empty() {
        case.verify_cmd = Some(verify);
    }
    case.solution_text = Some(text_parts.join("\n"));

    if case.description.is_empty() {
        return Err(Error::Parse("缺少「## 问题」区块".into()));
    }
    Ok(case)
}

/// 批量导入 Markdown 文档（一个文档 = 一条记录）。
pub fn import_markdown(conn: &Connection, text: &str) -> Result<i64> {
    let mut count = 0;
    // 按「\n---\n」切块；只有以「---」开头的块才是新文档的 front matter，
    // 其余块（该文档 front matter 之后的正文）拼回上一块，避免把闭合分隔符后的正文误拆。
    let mut docs: Vec<String> = Vec::new();
    for chunk in text.split("\n---\n") {
        let t = chunk.trim();
        if t.is_empty() {
            continue;
        }
        if t.starts_with("---") {
            docs.push(t.to_string());
        } else if let Some(last) = docs.last_mut() {
            last.push_str("\n---\n");
            last.push_str(t);
        } else {
            docs.push(t.to_string());
        }
    }
    for doc in docs {
        let case = parse_memory_markdown(&doc)?;
        save_case(conn, &case)?;
        count += 1;
    }
    Ok(count)
}

/// 导出为 Markdown 文档（F5.6 格式）。
pub fn export_markdown(case: &MemoryCase) -> String {
    let mut s = String::new();
    s.push_str("---\ntype: ai-ssh-memory\n");
    if let Some(os) = &case.os_info {
        s.push_str(&format!("os: {os}\n"));
    }
    if let Some(kw) = &case.keywords {
        let tags: Vec<&str> = kw.split_whitespace().collect();
        s.push_str(&format!("tags: [{}]\n", tags.join(", ")));
    }
    s.push_str("---\n\n");
    s.push_str("## 问题\n");
    s.push_str(&case.description);
    s.push_str("\n\n## 根因\n");
    s.push_str(case.root_cause.as_deref().unwrap_or(""));
    s.push_str("\n\n## 解决\n");
    if let Some(cmd) = &case.solution_cmd {
        for c in cmd.split('\n').filter(|l| !l.trim().is_empty()) {
            s.push_str("```bash\n");
            s.push_str(c);
            s.push_str("\n```\n\n");
        }
    }
    if let Some(txt) = &case.solution_text {
        s.push_str(txt);
        s.push_str("\n\n");
    }
    s.push_str("验证\n\n```bash\n");
    s.push_str(case.verify_cmd.as_deref().unwrap_or(""));
    s.push_str("\n```\n");
    s
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
        // memory_cases.host_id 有外键，先建主机 id=1 供用例引用
        conn.execute(
            "INSERT INTO hosts (name, host, username, auth_type) VALUES ('demo','demo','demo','password')",
            [],
        )
        .unwrap();
        conn
    }

    fn sample_case(desc: &str, kw: &str) -> MemoryCase {
        MemoryCase {
            host_id: Some(1),
            description: desc.to_string(),
            keywords: Some(kw.to_string()),
            source: CaseSource::UserManual,
            ..Default::default()
        }
    }

    #[test]
    fn save_and_get() {
        let conn = test_conn();
        let id = save_case(&conn, &sample_case("nginx 502 网关错误", "nginx 502")).unwrap();
        let c = get_case(&conn, id).unwrap();
        assert_eq!(c.description, "nginx 502 网关错误");
        assert!(!c.created_at.as_deref().unwrap_or("").is_empty());
    }

    #[test]
    fn keyword_search_ranks_by_matches() {
        let conn = test_conn();
        save_case(&conn, &sample_case("磁盘满了导致 nginx 502", "nginx disk")).unwrap();
        save_case(&conn, &sample_case("nginx 配置错误", "nginx config")).unwrap();
        save_case(&conn, &sample_case("数据库连接池耗尽", "mysql pool")).unwrap();
        let hits = search_cases(&conn, 1, "nginx 502 磁盘", 3).unwrap();
        assert_eq!(hits.len(), 1, "仅第 1 条同时命中 nginx/502/磁盘 语义");
        assert!(hits[0].description.contains("磁盘满了"));
    }

    #[test]
    fn chinese_bigram_search() {
        let conn = test_conn();
        save_case(&conn, &sample_case("内存泄漏导致 OOM 被杀", "oom memory")).unwrap();
        let hits = search_cases(&conn, 1, "内存泄漏", 3).unwrap();
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn fallback_to_recent_when_no_match() {
        let conn = test_conn();
        for i in 0..5 {
            save_case(&conn, &sample_case(&format!("案例 {i}"), "tag")).unwrap();
        }
        let hits = search_cases(&conn, 1, "不存在的关键词xyz", 3).unwrap();
        assert_eq!(hits.len(), 3);
        assert!(hits[0].description.contains("案例 4"));
    }

    #[test]
    fn other_host_excluded() {
        let conn = test_conn();
        save_case(&conn, &sample_case("hostA 的问题", "a")).unwrap();
        let hits = search_cases(&conn, 99, "hostA", 3).unwrap();
        assert!(hits.is_empty());
    }

    #[test]
    fn feedback_and_failed() {
        let conn = test_conn();
        let id = save_case(&conn, &sample_case("x", "x")).unwrap();
        feedback(&conn, id, true).unwrap();
        assert_eq!(get_case(&conn, id).unwrap().hit_count, 2);
        feedback(&conn, id, false).unwrap();
        assert_eq!(get_case(&conn, id).unwrap().hit_count, 1);
        mark_failed(&conn, id, "该方案已失效").unwrap();
        assert!(get_case(&conn, id).unwrap().failed_for.is_some());
    }

    #[test]
    fn markdown_roundtrip() {
        let md = r#"---
type: ai-ssh-memory
os: Ubuntu 22.04
tags: [nginx, disk]
---
## 问题
nginx 返回 502
## 根因
磁盘使用率 100%，日志写不进去
## 解决
```bash
rm /var/log/nginx/*.log
systemctl restart nginx
```
清理日志后重启
验证
```bash
curl -I http://localhost
```
"#;
        let case = parse_memory_markdown(md).unwrap();
        assert_eq!(case.description.trim(), "nginx 返回 502");
        assert!(case.solution_cmd.as_deref().unwrap().contains("systemctl restart nginx"));
        assert!(case.verify_cmd.as_deref().unwrap().contains("curl -I"));

        let conn = test_conn();
        let n = import_markdown(&conn, md).unwrap();
        assert_eq!(n, 1);
        let cases = list_cases(&conn, &CaseFilter { limit: 10, ..Default::default() }).unwrap();
        assert_eq!(cases.len(), 1);
        assert_eq!(cases[0].source, CaseSource::Imported);

        let exported = export_markdown(&cases[0]);
        assert!(exported.contains("## 问题"));
        assert!(exported.contains("nginx 返回 502"));
        // 再解析导出结果应等价
        let reparsed = parse_memory_markdown(&exported).unwrap();
        assert_eq!(reparsed.description.trim(), "nginx 返回 502");
    }

    #[test]
    fn list_filter() {
        let conn = test_conn();
        let mut c = sample_case("a", "k");
        c.verified = true;
        c.problem_type = Some("network".into());
        save_case(&conn, &c).unwrap();
        let f = CaseFilter {
            verified: Some(true),
            problem_type: Some("network".into()),
            search: Some("a".to_string()),
            limit: 10,
            offset: 0,
            ..Default::default()
        };
        assert_eq!(list_cases(&conn, &f).unwrap().len(), 1);
    }

    #[test]
    fn tokenizer() {
        let kw = tokenize_keywords("nginx 502 磁盘写满");
        assert!(kw.contains(&"nginx".to_string()));
        assert!(kw.contains(&"502".to_string()));
        assert!(kw.contains(&"磁盘".to_string()));
        assert!(kw.contains(&"写满".to_string()));
    }
}
