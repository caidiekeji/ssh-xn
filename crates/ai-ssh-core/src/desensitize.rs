//! 终端上下文脱敏（F4.1 / F6.3）：发送给云端 LLM 前替换密码、token、
//! 私钥内容，IP 中间段打码。指向 localhost/Ollama 的 provider 视为本地，
//! 默认不脱敏（可配置）。

use regex::Regex;
use std::sync::OnceLock;


#[derive(Debug, Clone)]
pub struct RedactOptions {
    /// 是否对 IP 中间段打码（PRD：用户可关闭）
    pub mask_ip: bool,
    /// provider 指向本机（localhost/127.0.0.1/Ollama）→ 默认不脱敏（可配置强制）
    pub local_trusted: bool,
}

impl Default for RedactOptions {
    fn default() -> Self {
        Self {
            mask_ip: true,
            local_trusted: false,
        }
    }
}

/// 密码类：`password=xxx` / `--password xxx` / `-p xxx` / `PASSWORD: xxx`
static RE_PASSWORD: OnceLock<Regex> = OnceLock::new();
/// token 类：`token=xxx` / `Bearer xxx` / `Authorization: xxx`
static RE_TOKEN: OnceLock<Regex> = OnceLock::new();
/// 私钥块
static RE_PRIVATE_KEY: OnceLock<Regex> = OnceLock::new();
/// IPv4 地址
static RE_IPV4: OnceLock<Regex> = OnceLock::new();

fn password_re() -> &'static Regex {
    RE_PASSWORD.get_or_init(|| {
        Regex::new(r#"(?i)((?:password|passwd|pwd)\s*[=:]\s*|--password\s+|--passwd\s+|\s-p\s+)(['"]?)[^\s'",;]+"#)
            .expect("password regex")
    })
}

fn token_re() -> &'static Regex {
    RE_TOKEN.get_or_init(|| {
        Regex::new(r#"(?i)((?:token|api[_-]?key|secret|access[_-]?key)\s*[=:]\s*|authorization:\s*bearer\s+)(['"]?)[^\s'",;]+"#)
            .expect("token regex")
    })
}

fn private_key_re() -> &'static Regex {
    RE_PRIVATE_KEY.get_or_init(|| {
        Regex::new(r#"-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----"#)
            .expect("private key regex")
    })
}

fn ipv4_re() -> &'static Regex {
    RE_IPV4.get_or_init(|| {
        Regex::new(r#"\b(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})\b"#).expect("ipv4 regex")
    })
}

/// 对一段文本做脱敏。本地 trusted 时原样返回（除非调用方强制）。
pub fn redact(input: &str, opts: &RedactOptions) -> String {
    if opts.local_trusted {
        return input.to_string();
    }
    let mut out = input.to_string();

    // 先整块替换私钥（多行），避免行内 regex 破坏块结构
    out = private_key_re()
        .replace_all(&out, "[REDACTED PRIVATE KEY]")
        .into_owned();

    // 密码：保留前缀键名，仅打码值
    out = password_re()
        .replace_all(&out, |caps: &regex::Captures| {
            let prefix = &caps[1];
            format!("{prefix}[REDACTED]")
        })
        .into_owned();

    out = token_re()
        .replace_all(&out, |caps: &regex::Captures| {
            let prefix = &caps[1];
            format!("{prefix}[REDACTED]")
        })
        .into_owned();

    if opts.mask_ip {
        out = ipv4_re()
            .replace_all(&out, |caps: &regex::Captures| {
                // 中间段打码：保留首尾段
                format!("{}.{}.{}.{}", &caps[1], "[*]", "[*]", &caps[4])
            })
            .into_owned();
    }
    out
}

/// 会话上下文采集时的滚动截断辅助：保留最近 N 行（PRD 实现注意事项 #5）。
pub fn tail_lines(input: &str, max_lines: usize) -> String {
    let mut lines: Vec<&str> = input.lines().collect();
    if lines.len() > max_lines {
        lines = lines[lines.len() - max_lines..].to_vec();
    }
    lines.join("\n")
}

/// 报错片段提取：检测点向上 N 行上下文（F4.3）。
pub fn extract_error_context(input: &str, match_line: usize, up_lines: usize) -> String {
    let lines: Vec<&str> = input.lines().collect();
    let start = match_line.saturating_sub(up_lines);
    lines
        .get(start..=match_line.min(lines.len().saturating_sub(1)))
        .unwrap_or(&[])
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cloud() -> RedactOptions {
        RedactOptions::default()
    }

    #[test]
    fn redacts_password_forms() {
        let s = "mysql -u root -p SecretPass123 --host 1.2.3.4";
        let out = redact(s, &cloud());
        assert!(!out.contains("SecretPass123"), "{out}");
        assert!(out.contains("-p [REDACTED]"), "{out}");
    }

    #[test]
    fn redacts_token_and_authorization() {
        let s = "curl -H 'Authorization: Bearer sk-abc123xyz' https://api.example.com";
        let out = redact(s, &cloud());
        assert!(!out.contains("sk-abc123xyz"), "{out}");
        assert!(out.contains("Bearer [REDACTED]"), "{out}");
    }

    #[test]
    fn redacts_private_key_block() {
        let s = "key:\n-----BEGIN RSA PRIVATE KEY-----\nMIIEpA==\n-----END RSA PRIVATE KEY-----";
        let out = redact(s, &cloud());
        assert!(!out.contains("MIIEpA=="), "{out}");
        assert!(out.contains("[REDACTED PRIVATE KEY]"), "{out}");
    }

    #[test]
    fn masks_ip_middle_segments() {
        let s = "server at 192.168.10.23 failed";
        let out = redact(s, &cloud());
        assert!(out.contains("192.[*].[*].23"), "{out}");
        assert!(!out.contains("192.168.10.23"), "{out}");
    }

    #[test]
    fn local_trusted_skips_redaction() {
        let s = "password=abc123 at 10.0.0.5";
        let out = redact(s, &RedactOptions { local_trusted: true, ..Default::default() });
        assert_eq!(out, s);
    }

    #[test]
    fn tail_truncation() {
        let s = (0..10).map(|i| format!("line{i}")).collect::<Vec<_>>().join("\n");
        let out = tail_lines(&s, 3);
        assert_eq!(out, "line7\nline8\nline9");
    }

    #[test]
    fn error_context_extraction() {
        let s = "a\nb\nc\nd\nerror here";
        let out = extract_error_context(s, 4, 2);
        assert_eq!(out, "c\nd\nerror here");
    }
}
