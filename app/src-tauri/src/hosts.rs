// 主机 CRUD（F1.1）：密码/私钥凭据 AES-256-GCM 加密存储（F1.1 / N7）
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use ai_ssh_core::crypto;
use ai_ssh_core::Error;
use ai_ssh_core::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct HostRow {
    pub id: i64,
    pub name: String,
    pub host: String,
    pub port: i64,
    pub username: String,
    pub auth_type: String,
    pub key_path: Option<String>,
    pub group_name: Option<String>,
    pub tags: Option<String>,
    pub notes: Option<String>,
    pub memory_enabled: bool,
    pub monitor_enabled: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct HostInput {
    pub name: String,
    pub host: String,
    pub port: Option<i64>,
    pub username: String,
    pub auth_type: String,
    #[serde(default)]
    pub secret: String, // 明文密码（仅接收时出现）
    pub key_path: Option<String>,
    #[serde(default)]
    pub passphrase: String, // 明文 passphrase（仅接收时出现）
    pub jump_host_id: Option<i64>,
    pub group_name: Option<String>,
    pub tags: Option<String>,
    pub notes: Option<String>,
    pub memory_enabled: bool,
    pub monitor_enabled: bool,
}

fn encrypt_secret(plain: &str) -> Option<Vec<u8>> {
    if plain.is_empty() {
        return None;
    }
    let key = crypto::derive_key(&crypto::device_fingerprint());
    crypto::encrypt(plain.as_bytes(), &key).ok()
}

fn decrypt_secret(data: &Option<Vec<u8>>) -> Option<String> {
    let data = data.as_ref()?;
    let key = crypto::derive_key(&crypto::device_fingerprint());
    crypto::decrypt(data, &key).ok().map(|b| String::from_utf8_lossy(&b).to_string())
}

pub fn list_hosts(conn: &Connection) -> Result<Vec<HostRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, host, port, username, auth_type, key_path, group_name, tags, notes,
                memory_enabled, monitor_enabled
         FROM hosts ORDER BY group_name, name",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(HostRow {
            id: r.get(0)?,
            name: r.get(1)?,
            host: r.get(2)?,
            port: r.get(3)?,
            username: r.get(4)?,
            auth_type: r.get(5)?,
            key_path: r.get(6)?,
            group_name: r.get(7)?,
            tags: r.get(8)?,
            notes: r.get(9)?,
            memory_enabled: r.get::<_, i64>(10)? != 0,
            monitor_enabled: r.get::<_, i64>(11)? != 0,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn get_host(conn: &Connection, id: i64) -> Result<HostRow> {
    list_hosts(conn)?.into_iter().find(|h| h.id == id).ok_or_else(|| Error::NotFound(format!("主机 {id}")))
}

pub fn get_secret(conn: &Connection, id: i64) -> Result<Option<String>> {
    let row = conn
        .query_row(
            "SELECT secret_encrypted FROM hosts WHERE id=?1",
            params![id],
            |r| r.get::<_, Option<Vec<u8>>>(0),
        )
        .optional()?;
    Ok(row.and_then(|d| decrypt_secret(&d)))
}

pub fn get_passphrase(conn: &Connection, id: i64) -> Result<Option<String>> {
    let row = conn
        .query_row(
            "SELECT passphrase_encrypted FROM hosts WHERE id=?1",
            params![id],
            |r| r.get::<_, Option<Vec<u8>>>(0),
        )
        .optional()?;
    Ok(row.and_then(|d| decrypt_secret(&d)))
}

pub fn save_host(conn: &Connection, input: &HostInput, id: Option<i64>) -> Result<i64> {
    let secret_enc = encrypt_secret(&input.secret);
    let pass_enc = encrypt_secret(&input.passphrase);
    let port = input.port.unwrap_or(22);

    if let Some(id) = id {
        // 留空表示保持原凭据
        let (cur_secret, cur_pass) = if secret_enc.is_none() || pass_enc.is_none() {
            (
                conn.query_row("SELECT secret_encrypted FROM hosts WHERE id=?1", params![id], |r| r.get::<_, Option<Vec<u8>>>(0))
                    .optional()?
                    .flatten(),
                conn.query_row("SELECT passphrase_encrypted FROM hosts WHERE id=?1", params![id], |r| r.get::<_, Option<Vec<u8>>>(0))
                    .optional()?
                    .flatten(),
            )
        } else {
            (secret_enc, pass_enc)
        };
        conn.execute(
            "UPDATE hosts SET name=?1, host=?2, port=?3, username=?4, auth_type=?5,
                    secret_encrypted=?6, key_path=?7, passphrase_encrypted=?8, jump_host_id=?9,
                    group_name=?10, tags=?11, notes=?12, memory_enabled=?13, monitor_enabled=?14,
                    updated_at=datetime('now')
             WHERE id=?15",
            params![
                input.name, input.host, port, input.username, input.auth_type,
                cur_secret, input.key_path, cur_pass, input.jump_host_id,
                input.group_name, input.tags, input.notes,
                input.memory_enabled as i64, input.monitor_enabled as i64, id
            ],
        )?;
        Ok(id)
    } else {
        conn.execute(
            "INSERT INTO hosts (name, host, port, username, auth_type, secret_encrypted, key_path,
                    passphrase_encrypted, jump_host_id, group_name, tags, notes,
                    memory_enabled, monitor_enabled, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14, datetime('now'), datetime('now'))",
            params![
                input.name, input.host, port, input.username, input.auth_type,
                secret_enc, input.key_path, pass_enc, input.jump_host_id,
                input.group_name, input.tags, input.notes,
                input.memory_enabled as i64, input.monitor_enabled as i64
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }
}

pub fn delete_host(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM hosts WHERE id=?1", params![id])?;
    Ok(())
}
