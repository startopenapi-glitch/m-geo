//! Explicit public-URL readback and human-recorded withdrawal lifecycle.
//! The database dispatcher never performs I/O; `fetch_and_verify` is called by
//! the outer runtime before `publication.recordReadback` is dispatched.
use super::{date_time, guid, load, now, raw, text, update, web_url, Result};
use rusqlite::Connection;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::net::IpAddr;
use std::time::Duration;
use url::Url;

const MAX_READBACKS: usize = 12;
const MAX_WITHDRAWAL_HISTORY: usize = 12;
const MAX_RESULT_BYTES: usize = 16 * 1024;
const MAX_RESPONSE_BYTES: usize = 512 * 1024;
const MAX_REDIRECTS: usize = 5;
const ERROR_CATEGORIES: &[&str] = &[
    "dns",
    "timeout",
    "transport",
    "http_status",
    "response_too_large",
    "invalid_response",
];

/// Handles only durable writes. Unknown actions are left for the core dispatcher.
pub(super) fn dispatch(c: &Connection, a: &str, p: &Value, b: &str) -> Result<Option<Value>> {
    match a {
        "publication.recordReadback" => record_readback(c, p, b).map(Some),
        "publication.withdraw" => withdraw(c, p, b).map(Some),
        _ => Ok(None),
    }
}

fn publication<'a>(c: &Connection, p: &Value, b: &str) -> Result<Value> {
    let id = guid(p, "id")?;
    load(c, "m_publications", b, &id)
}

fn optional_hash(v: &Value) -> Result<Value> {
    match v.get("bodySha256") {
        None | Some(Value::Null) => Ok(Value::Null),
        Some(Value::String(s))
            if s.len() == 64
                && s.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) =>
        {
            Ok(json!(s))
        }
        _ => Err("正文 SHA256 必须为 64 位小写十六进制".into()),
    }
}

fn record_readback(c: &Connection, p: &Value, b: &str) -> Result<Value> {
    let mut publication = publication(c, p, b)?;
    if publication.get("withdrawal").is_some() {
        return Err("该记录已进入撤回流程，不能用普通回读改为在线".into());
    }
    let result = p
        .get("result")
        .filter(|v| v.is_object() && v.to_string().len() <= MAX_RESULT_BYTES)
        .ok_or("回读结果为空、格式错误或超出大小限制")?;
    if number_or_null(result, "draftRevision")? != publication["draftRevision"].as_i64() {
        return Err("回读结果与发布稿件版本不一致".into());
    }
    let fetched_at = date_time(result, "fetchedAt")?;
    let http_status = match result.get("httpStatus") {
        None | Some(Value::Null) => Value::Null,
        Some(Value::Number(n)) if n.as_u64().is_some_and(|s| (100..=599).contains(&s)) => {
            json!(n.as_u64().unwrap())
        }
        _ => return Err("HTTP 状态必须为 100 至 599 或空值".into()),
    };
    let final_url = text(result, "finalUrl", 2000, true)?;
    web_url(&final_url, true)?;
    let title = text(result, "title", 300, false)?;
    let body_sha256 = optional_hash(result)?;
    let matched_content = result
        .get("matchedContent")
        .and_then(Value::as_bool)
        .ok_or("回读结果缺少内容匹配结论")?;
    let error_category = match result.get("errorCategory") {
        None | Some(Value::Null) => Value::Null,
        Some(Value::String(s)) if ERROR_CATEGORIES.contains(&s.as_str()) => json!(s),
        _ => return Err("不支持的回读错误分类".into()),
    };
    let success = http_status
        .as_u64()
        .is_some_and(|s| (200..300).contains(&s));
    if success {
        if !error_category.is_null() || body_sha256.is_null() {
            return Err("成功回读必须包含正文 SHA256 且不能包含错误分类".into());
        }
    } else {
        if matched_content {
            return Err("未成功读取公开页面时不能标记内容匹配".into());
        }
        if error_category.is_null() {
            return Err("失败回读必须包含错误分类".into());
        }
        if http_status.as_u64().is_some() != (error_category == "http_status") {
            return Err("HTTP 状态与错误分类不一致".into());
        }
    }
    let lifecycle = if !success {
        "unreachable"
    } else if matched_content {
        "online"
    } else {
        "content_changed"
    };
    let transport = if final_url.starts_with("https://") {
        "https"
    } else {
        "http"
    };
    // Copy only the allow-listed metadata. Response bodies, error details,
    // filesystem paths and arbitrary transport fields are never persisted.
    let readback = json!({
        "fetchedAt":fetched_at,
        "httpStatus":http_status,
        "finalUrl":final_url,
        "title":title,
        "bodySha256":body_sha256,
        "draftRevision":publication["draftRevision"],
        "matchedContent":matched_content,
        "errorCategory":error_category,
        "transport":transport,
    });
    let mut readbacks = publication
        .get("readbacks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    readbacks.insert(0, readback);
    readbacks.truncate(MAX_READBACKS);
    publication["lifecycle"] = json!(lifecycle);
    publication["readbacks"] = json!(readbacks);
    update(c, "m_publications", &publication)?;
    Ok(publication)
}

fn number_or_null(v: &Value, key: &str) -> Result<Option<i64>> {
    v.get(key)
        .and_then(Value::as_i64)
        .ok_or_else(|| format!("请检查数值字段 {}", key))
        .map(Some)
}

fn withdraw(c: &Connection, p: &Value, b: &str) -> Result<Value> {
    if !p.get("confirm").and_then(Value::as_bool).unwrap_or(false) {
        return Err("登记撤回前必须明确确认".into());
    }
    let reason = raw(p, "reason", 2000, true)?;
    let status = match text(p, "status", 40, true)?.as_str() {
        "withdrawing" => "withdrawing",
        "withdrawn" => "withdrawn",
        "withdrawal_unverified" => "withdrawal_unverified",
        _ => return Err("不支持的撤回状态".into()),
    };
    let mut publication = publication(c, p, b)?;
    let previous = publication.get("withdrawal").cloned();
    if previous.as_ref().and_then(|w| w["status"].as_str()) == Some("withdrawn")
        && status == "withdrawing"
    {
        return Err("已撤回记录不能直接退回撤回中；如有新情况请登记撤回待核查".into());
    }
    let timestamp = now();
    let requested_at = previous
        .as_ref()
        .and_then(|w| w["requestedAt"].as_str())
        .unwrap_or(&timestamp)
        .to_owned();
    let mut history = previous
        .as_ref()
        .and_then(|w| w.get("history"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if let Some(old) = previous {
        history.insert(
            0,
            json!({"status":old["status"],"reason":old["reason"],"recordedAt":old["updatedAt"]}),
        );
        history.truncate(MAX_WITHDRAWAL_HISTORY);
    }
    // This is a human attestation only. It is not evidence of remote deletion,
    // de-indexing, or platform processing.
    publication["withdrawal"] = json!({
        "status":status,
        "reason":reason,
        "requestedAt":requested_at,
        "updatedAt":timestamp,
        "history":history,
    });
    publication["lifecycle"] = json!(status);
    update(c, "m_publications", &publication)?;
    Ok(publication)
}

pub(super) fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let o = ip.octets();
            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_broadcast()
                || ip.is_multicast()
                || o[0] == 0
                || o[0] >= 224
                || (o[0] == 100 && (64..=127).contains(&o[1]))
                || (o[0] == 192 && o[1] == 0 && o[2] == 0)
                || (o[0] == 192 && o[1] == 0 && o[2] == 2)
                || (o[0] == 198 && (o[1] == 18 || o[1] == 19))
                || (o[0] == 198 && o[1] == 51 && o[2] == 100)
                || (o[0] == 203 && o[1] == 0 && o[2] == 113))
        }
        IpAddr::V6(ip) => {
            if let Some(mapped) = ip.to_ipv4_mapped() {
                return public_ip(IpAddr::V4(mapped));
            }
            if ip.is_loopback() || ip.is_unspecified() || ip.is_multicast() {
                return false;
            }
            let segments = ip.segments();
            (segments[0] & 0xfe00) != 0xfc00
                && (segments[0] & 0xffc0) != 0xfe80
                && !(segments[0] == 0x2001 && segments[1] == 0x0db8)
        }
    }
}

async fn public_socket_addrs(url: &Url) -> std::result::Result<Vec<std::net::SocketAddr>, String> {
    let host = url.host_str().ok_or("公开网址缺少主机")?;
    let port = url.port_or_known_default().ok_or("公开网址缺少端口")?;
    let addrs = tokio::net::lookup_host((host, port))
        .await
        .map_err(|_| "dns".to_string())?
        .collect::<Vec<_>>();
    if addrs.is_empty() || addrs.iter().any(|a| !public_ip(a.ip())) {
        return Err("dns".into());
    }
    Ok(addrs)
}

async fn client_for(url: &Url) -> std::result::Result<reqwest::Client, String> {
    let host = url.host_str().ok_or("invalid_response")?.to_owned();
    let addrs = public_socket_addrs(url).await?;
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(8))
        .connect_timeout(Duration::from_secs(5))
        .pool_max_idle_per_host(0)
        .resolve_to_addrs(&host, &addrs)
        .user_agent("M-GEO-Publication-Readback/0.19")
        .build()
        .map_err(|_| "transport".to_string())
}

fn decode_entities(s: &str) -> String {
    s.replace("&nbsp;", " ")
        .replace("&#160;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

/// Conservative visible-text extraction; returned text is hashed/compared and
/// must not be persisted. Unknown markup simply produces a non-match.
pub(super) fn visible_text(html: &str) -> String {
    let mut out = String::new();
    let mut rest = html;
    let mut hidden = false;
    while let Some(start) = rest.find('<') {
        if !hidden {
            out.push_str(&rest[..start]);
            out.push(' ');
        }
        let Some(end) = rest[start..].find('>').map(|i| start + i) else {
            break;
        };
        let tag = rest[start + 1..end].trim().to_ascii_lowercase();
        if ["script", "style", "noscript", "template"]
            .iter()
            .any(|name| tag.starts_with(name) && tag[name.len()..].starts_with(char::is_whitespace))
            || ["script", "style", "noscript", "template"].contains(&tag.as_str())
        {
            hidden = true;
        } else if tag.starts_with('/')
            && ["script", "style", "noscript", "template"]
                .iter()
                .any(|name| tag[1..].starts_with(name))
        {
            hidden = false;
        }
        rest = &rest[end + 1..];
    }
    if !hidden {
        out.push_str(rest);
    }
    decode_entities(&out)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn page_title(html: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let Some(open) = lower.find("<title") else {
        return String::new();
    };
    let Some(start) = lower[open..].find('>').map(|i| open + i + 1) else {
        return String::new();
    };
    let Some(end) = lower[start..].find("</title>").map(|i| start + i) else {
        return String::new();
    };
    visible_text(&html[start..end])
        .chars()
        .take(300)
        .collect::<String>()
}

fn comparable_snapshot(snapshot: &str) -> String {
    snapshot
        .lines()
        .map(|line| line.strip_prefix("# ").unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn content_sha256(text: &str) -> String {
    hex::encode(Sha256::digest(text.as_bytes()))
}

struct Fetched {
    final_url: String,
    status: u16,
    title: String,
    body_sha256: Option<String>,
    matched: bool,
    error_category: Option<&'static str>,
}

async fn fetch_bounded(start: &Url, snapshot: &str) -> std::result::Result<Fetched, String> {
    let mut current = start.clone();
    for redirects in 0..=MAX_REDIRECTS {
        web_url(current.as_str(), true).map_err(|_| "unsafe_url".to_string())?;
        let client = client_for(&current).await?;
        let response = client
            .get(current.clone())
            .header(
                reqwest::header::ACCEPT,
                "text/html,application/xhtml+xml,text/plain;q=0.8",
            )
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    "timeout"
                } else {
                    "transport"
                }
                .to_string()
            })?;
        let status = response.status();
        if status.is_redirection() {
            if redirects == MAX_REDIRECTS {
                return Err("invalid_response".into());
            }
            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .ok_or("invalid_response")?;
            current = current
                .join(location)
                .map_err(|_| "invalid_response".to_string())?;
            // The next loop validates the new URL and re-resolves/pins its host.
            continue;
        }
        if !status.is_success() {
            return Ok(Fetched {
                final_url: current.to_string(),
                status: status.as_u16(),
                title: String::new(),
                body_sha256: None,
                matched: false,
                error_category: Some("http_status"),
            });
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
        {
            return Err("response_too_large".into());
        }
        let mut response = response;
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|e| {
            if e.is_timeout() {
                "timeout"
            } else {
                "transport"
            }
            .to_string()
        })? {
            if bytes.len() + chunk.len() > MAX_RESPONSE_BYTES {
                return Err("response_too_large".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        let html = String::from_utf8_lossy(&bytes);
        let body = visible_text(&html);
        let expected = comparable_snapshot(snapshot);
        return Ok(Fetched {
            final_url: current.to_string(),
            status: status.as_u16(),
            title: page_title(&html),
            body_sha256: Some(content_sha256(&body)),
            matched: !expected.is_empty() && body.contains(&expected),
            error_category: None,
        });
    }
    Err("invalid_response".into())
}

fn unreachable_result(url: &Url, draft_revision: i64, category: &str) -> Value {
    json!({
        "fetchedAt":now(),
        "httpStatus":Value::Null,
        "finalUrl":url.as_str(),
        "title":"",
        "bodySha256":Value::Null,
        "draftRevision":draft_revision,
        "matchedContent":false,
        "errorCategory":category,
    })
}

/// Independent network boundary for the outer runtime. It never writes the DB,
/// retries automatically, follows redirects implicitly, or returns page bodies.
pub(crate) async fn fetch_and_verify(publication: &Value) -> Result<Value> {
    let original = text(publication, "url", 2000, true)?;
    web_url(&original, true)?;
    let url = Url::parse(&original).map_err(|_| "公开网址格式错误".to_string())?;
    let draft_revision = publication["draftRevision"]
        .as_i64()
        .ok_or("发布稿件版本损坏")?;
    let snapshot = publication["contentSnapshot"]
        .as_str()
        .ok_or("发布固定快照损坏")?;
    let fetched =
        tokio::time::timeout(Duration::from_secs(20), fetch_bounded(&url, snapshot)).await;
    match fetched {
        Ok(Ok(page)) => Ok(json!({
            "fetchedAt":now(),
            "httpStatus":page.status,
            "finalUrl":page.final_url,
            "title":page.title,
            "bodySha256":page.body_sha256,
            "draftRevision":draft_revision,
            "matchedContent":page.matched,
            "errorCategory":page.error_category,
        })),
        Ok(Err(category)) if category == "unsafe_url" => Err("重定向目标不是允许的公开网址".into()),
        Ok(Err(category)) => Ok(unreachable_result(&url, draft_revision, &category)),
        Err(_) => Ok(unreachable_result(&url, draft_revision, "timeout")),
    }
}
