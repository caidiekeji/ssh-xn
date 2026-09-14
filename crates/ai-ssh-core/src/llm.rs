//! LLM 适配层（F2）：多 Provider（OpenAI 兼容 / Anthropic / Ollama）、
//! SSE 流式、超时重试、路由降级、Token 用量记录。

use std::pin::Pin;

use futures_util::{Stream, StreamExt};
use rusqlite::{Connection, OptionalExtension};
use bytes::Bytes;
use serde_json::json;

use crate::error::{Error, Result};
use crate::prompt::ChatMessage;
use crate::schema::{LlmProtocol, ProviderConfig};

/// 默认超时（PRD F2.4：60s）
const TIMEOUT_SECS: u64 = 60;

/// 流式响应块：`Ok(文本增量)` 或 `Err(中断/错误)`。
pub type LlmChunk = Result<String>;

fn build_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(TIMEOUT_SECS))
        .build()
        .expect("reqwest client 构建失败")
}

/// 按协议构造请求体与请求头。
fn build_request(provider: &ProviderConfig, messages: &[ChatMessage]) -> (reqwest::RequestBuilder, bool) {
    let client = build_client();
    let url = normalize_base(&provider.base_url);
    let is_sse = true;
    let req = match provider.protocol {
        LlmProtocol::OpenaiCompatible => {
            let mut rb = client
                .post(format!("{url}/chat/completions"))
                .json(&json!({
                    "model": provider.model_name,
                    "messages": messages,
                    "stream": true,
                    "temperature": provider.temperature,
                    "max_tokens": provider.max_tokens,
                }));
            if let Some(key) = provider.api_key.as_deref().filter(|k| !k.is_empty()) {
                rb = rb.header("Authorization", format!("Bearer {key}"));
            }
            rb
        }
        LlmProtocol::Anthropic => {
            let mut rb = client
                .post(format!("{url}/v1/messages"))
                .header("anthropic-version", "2023-06-01")
                .json(&json!({
                    "model": provider.model_name,
                    "max_tokens": provider.max_tokens,
                    "stream": true,
                    "temperature": provider.temperature,
                    "messages": messages,
                }));
            if let Some(key) = provider.api_key.as_deref().filter(|k| !k.is_empty()) {
                rb = rb.header("x-api-key", key);
            }
            rb
        }
        LlmProtocol::Ollama => client
            .post(format!("{url}/api/chat"))
            .json(&json!({
                "model": provider.model_name,
                "messages": messages,
                "stream": true,
                "options": { "temperature": provider.temperature, "num_predict": provider.max_tokens },
            })),
    };
    (req, is_sse)
}

/// base_url 规整：去掉末尾斜杠。
pub fn normalize_base(url: &str) -> String {
    url.trim_end_matches('/').to_string()
}

/// 拼接 System Prompt（extra_system_prompt 追加到最后一个 system 消息）。
pub fn merge_system_prompt(mut messages: Vec<ChatMessage>, extra: &str) -> Vec<ChatMessage> {
    if extra.trim().is_empty() {
        return messages;
    }
    if let Some(m) = messages.iter_mut().find(|m| m.role == "system") {
        m.content.push_str("\n");
        m.content.push_str(extra);
    } else {
        messages.insert(0, ChatMessage::system(extra));
    }
    messages
}

/// 流式调用 LLM（SSE）。失败时按 F2.4 重试 1 次。
pub async fn stream_chat(
    provider: &ProviderConfig,
    messages: Vec<ChatMessage>,
) -> Result<Pin<Box<dyn Stream<Item = LlmChunk> + Send>>> {
    let messages = merge_system_prompt(messages, &provider.extra_system_prompt);
    let mut attempt = 0;
    loop {
        attempt += 1;
        match try_stream_once(provider, &messages).await {
            Ok(stream) => return Ok(stream),
            Err(e) => {
                if attempt < 2 {
                    // 首连失败重试一次
                    continue;
                }
                return Err(e);
            }
        }
    }
}

async fn try_stream_once(
    provider: &ProviderConfig,
    messages: &[ChatMessage],
) -> Result<Pin<Box<dyn Stream<Item = LlmChunk> + Send>>> {
    let (req, _is_sse) = build_request(provider, messages);
    let resp = req.send().await.map_err(|e| Error::Http(format!("请求失败: {e}")))?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(Error::Llm(format!("HTTP {status}: {}", truncate(&body, 500))));
    }
    let byte_stream = resp.bytes_stream();
    Ok(stream_sse(provider.protocol, byte_stream))
}

/// 把字节流解析为文本增量流（兼容 SSE 与 Ollama 换行 JSON）。
fn stream_sse(
    protocol: LlmProtocol,
    byte_stream: impl Stream<Item = std::result::Result<Bytes, reqwest::Error>> + Send + 'static,
) -> Pin<Box<dyn Stream<Item = LlmChunk> + Send>> {
    Box::pin(async_stream::stream! {
        let mut buffer = String::new();
        let mut byte_stream = byte_stream;
        let mut byte_stream = Box::pin(byte_stream);
        while let Some(chunk) = byte_stream.next().await {
            match chunk {
                Ok(bytes) => {
                    buffer.push_str(&String::from_utf8_lossy(&bytes));
                    // 按行切分处理
                    let mut start = 0;
                    while let Some(rel) = buffer[start..].find('\n') {
                        let line = buffer[start..start + rel].trim_end_matches('\r').to_string();
                        start += rel + 1;
                        if let Some(delta) = parse_sse_line(protocol, &line) {
                            yield Ok(delta);
                        }
                    }
                    buffer = buffer[start..].to_string();
                }
                Err(e) => {
                    yield Err(Error::Http(format!("流中断: {e}")));
                    return;
                }
            }
        }
        // 尾部残行
        let line = buffer.trim();
        if !line.is_empty() {
            if let Some(delta) = parse_sse_line(protocol, line) {
                yield Ok(delta);
            }
        }
    })
}

/// 解析单行 SSE / JSONL，返回文本增量（None 表示无内容）。
fn parse_sse_line(protocol: LlmProtocol, line: &str) -> Option<String> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let data = if let Some(rest) = line.strip_prefix("data:") {
        rest.trim()
    } else {
        line
    };
    if data == "[DONE]" {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(data).ok()?;
    match protocol {
        LlmProtocol::OpenaiCompatible | LlmProtocol::Ollama => {
            // OpenAI: choices[0].delta.content；Ollama: message.content
            if let Some(choices) = v.get("choices").and_then(|c| c.as_array()) {
                if let Some(choice) = choices.first() {
                    if let Some(content) = choice.pointer("/delta/content").and_then(|c| c.as_str()) {
                        return Some(content.to_string());
                    }
                    if let Some(content) = choice.get("message").and_then(|m| m.get("content")).and_then(|c| c.as_str()) {
                        return Some(content.to_string());
                    }
                }
                return None;
            }
            v.pointer("/message/content")
                .and_then(|c| c.as_str())
                .map(String::from)
        }
        LlmProtocol::Anthropic => {
            // SSE: event: content_block_delta → data: {"type":"content_block_delta","delta":{"type":"text_delta","text":"..."}}
            if v.get("type").and_then(|t| t.as_str()) == Some("content_block_delta") {
                return v.pointer("/delta/text").and_then(|t| t.as_str()).map(String::from);
            }
            // 兼容非流式包装
            v.pointer("/content").and_then(|c| c.as_array()).and_then(|a| {
                a.first()
                    .and_then(|x| x.get("text"))
                    .and_then(|t| t.as_str())
                    .map(String::from)
            })
        }
    }
}

/// 连通性测试（F2.1）：非流式最小请求，返回耗时与模型回复。
pub async fn test_provider(provider: &ProviderConfig) -> Result<(u128, String)> {
    let messages = merge_system_prompt(vec![ChatMessage::user("ping")], &provider.extra_system_prompt);
    let url = match provider.protocol {
        LlmProtocol::OpenaiCompatible => format!("{}/chat/completions", normalize_base(&provider.base_url)),
        LlmProtocol::Anthropic => format!("{}/v1/messages", normalize_base(&provider.base_url)),
        LlmProtocol::Ollama => format!("{}/api/chat", normalize_base(&provider.base_url)),
    };
    let body = match provider.protocol {
        LlmProtocol::OpenaiCompatible => json!({
            "model": provider.model_name, "messages": messages,
            "stream": false, "max_tokens": 8, "temperature": 0.0,
        }),
        LlmProtocol::Anthropic => json!({
            "model": provider.model_name, "messages": messages,
            "stream": false, "max_tokens": 8, "temperature": 0.0,
        }),
        LlmProtocol::Ollama => json!({
            "model": provider.model_name, "messages": messages,
            "stream": false, "options": { "num_predict": 8 },
        }),
    };
    let mut rb = build_client().post(url).json(&body);
    if let Some(key) = provider.api_key.as_deref().filter(|k| !k.is_empty()) {
        match provider.protocol {
            LlmProtocol::Anthropic => rb = rb.header("x-api-key", key).header("anthropic-version", "2023-06-01"),
            _ => rb = rb.header("Authorization", format!("Bearer {key}")),
        }
    }
    let started = std::time::Instant::now();
    let resp = rb.send().await.map_err(|e| Error::Http(format!("请求失败: {e}")))?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    let latency = started.elapsed().as_millis();
    if !status.is_success() {
        return Err(Error::Llm(format!("HTTP {status}: {}", truncate(&text, 300))));
    }
    let reply = extract_reply(provider.protocol, &text);
    Ok((latency, reply))
}

fn extract_reply(protocol: LlmProtocol, text: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(text).unwrap_or_default();
    match protocol {
        LlmProtocol::OpenaiCompatible => v
            .pointer("/choices/0/message/content")
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string(),
        LlmProtocol::Anthropic => v
            .pointer("/content/0/text")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string(),
        LlmProtocol::Ollama => v
            .pointer("/message/content")
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string(),
    }
}

/// 记录 Token 用量（F2.3，本地粗略估算：每 4 字符 ≈ 1 token）。
pub fn record_token_usage(
    conn: &Connection,
    provider_id: Option<i64>,
    scene: &str,
    prompt_tokens: i64,
    completion_tokens: i64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO token_usage (date, provider_id, scene, prompt_tokens, completion_tokens)
         VALUES (date('now'), ?1, ?2, ?3, ?4)",
        rusqlite::params![provider_id, scene, prompt_tokens, completion_tokens],
    )?;
    Ok(())
}

pub fn estimate_tokens(text: &str) -> i64 {
    (text.chars().count() / 4 + 1) as i64
}

/// 读取某场景的路由（主 provider + fallback 列表，按顺序）。
pub fn resolve_routing(conn: &Connection, scene: &str) -> Result<Vec<ProviderConfig>> {
    let row = conn
        .query_row(
            "SELECT provider_id, fallback_ids FROM llm_routing WHERE scene=?1",
            rusqlite::params![scene],
            |r| {
                Ok((
                    r.get::<_, Option<i64>>(0)?,
                    r.get::<_, Option<String>>(1)?,
                ))
            },
        )
        .optional()?;

    let mut ids: Vec<i64> = Vec::new();
    if let Some((primary, fallbacks)) = row {
        if let Some(p) = primary {
            ids.push(p);
        }
        if let Some(fb) = fallbacks {
            for part in fb.split(',') {
                if let Ok(id) = part.trim().parse::<i64>() {
                    if !ids.contains(&id) {
                        ids.push(id);
                    }
                }
            }
        }
    }
    // 无路由配置时回退：所有 enabled provider（按 id 升序）
    if ids.is_empty() {
        let mut stmt = conn.prepare("SELECT id FROM llm_providers WHERE enabled=1 ORDER BY id")?;
        let rows = stmt.query_map([], |r| r.get::<_, i64>(0))?;
        for r in rows {
            ids.push(r?);
        }
    }
    let mut out = Vec::new();
    for id in ids {
        if let Ok(p) = get_provider(conn, id) {
            if p.enabled {
                out.push(p);
            }
        }
    }
    Ok(out)
}

pub fn get_provider(conn: &Connection, id: i64) -> Result<ProviderConfig> {
    conn.query_row(
        "SELECT id, name, protocol, base_url, api_key_encrypted, model_name, is_local,
                temperature, max_tokens, extra_system_prompt, enabled
         FROM llm_providers WHERE id=?1",
        rusqlite::params![id],
        |r| {
            Ok(ProviderConfig {
                id: Some(r.get(0)?),
                name: r.get(1)?,
                protocol: match r.get::<_, String>(2)?.as_str() {
                    "anthropic" => LlmProtocol::Anthropic,
                    "ollama" => LlmProtocol::Ollama,
                    _ => LlmProtocol::OpenaiCompatible,
                },
                base_url: r.get(3)?,
                api_key: r.get::<_, Option<Vec<u8>>>(4)?.map(|v| String::from_utf8_lossy(&v).into_owned()),
                model_name: r.get(5)?,
                is_local: r.get::<_, i64>(6)? != 0,
                temperature: r.get(7)?,
                max_tokens: r.get(8)?,
                extra_system_prompt: r.get(9)?,
                enabled: r.get::<_, i64>(10)? != 0,
            })
        },
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => Error::NotFound(format!("LLM provider {id}")),
        other => other.into(),
    })
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        let mut out: String = s.chars().take(max).collect();
        out.push_str("…");
        out
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_url_normalize() {
        assert_eq!(normalize_base("https://api.openai.com/v1/"), "https://api.openai.com/v1");
    }

    #[test]
    fn sse_line_parsing() {
        let provider = ProviderConfig {
            protocol: LlmProtocol::OpenaiCompatible,
            ..Default::default()
        };
        let d = parse_sse_line(provider.protocol, r#"data: {"choices":[{"delta":{"content":"你好"}}]}"#);
        assert_eq!(d.as_deref(), Some("你好"));
        let d = parse_sse_line(provider.protocol, "data: [DONE]");
        assert!(d.is_none());

        let ollama = ProviderConfig {
            protocol: LlmProtocol::Ollama,
            ..Default::default()
        };
        let d = parse_sse_line(ollama.protocol, r#"{"message":{"content":"世界"},"done":false}"#);
        assert_eq!(d.as_deref(), Some("世界"));

        let anth = ProviderConfig {
            protocol: LlmProtocol::Anthropic,
            ..Default::default()
        };
        let d = parse_sse_line(anth.protocol, r#"event: content_block_delta"#);
        assert!(d.is_none());
        let d = parse_sse_line(
            anth.protocol,
            r#"data: {"type":"content_block_delta","delta":{"type":"text_delta","text":"答案"}}"#,
        );
        assert_eq!(d.as_deref(), Some("答案"));
    }

    #[test]
    fn system_prompt_merge() {
        let msgs = vec![ChatMessage::system("基础"), ChatMessage::user("hi")];
        let out = merge_system_prompt(msgs, "追加段");
        assert!(out[0].content.contains("追加段"));
        let msgs = vec![ChatMessage::user("hi")];
        let out = merge_system_prompt(msgs, "追加段");
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].role, "system");
    }

    #[test]
    fn token_estimate() {
        assert!(estimate_tokens("hello world") >= 1);
    }
}
