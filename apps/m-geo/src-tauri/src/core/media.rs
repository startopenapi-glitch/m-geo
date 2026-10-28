//! Portable image/video media stored inside the encrypted SQLite workspace.
//! No media bytes or metadata are read from, written to, or referenced by filesystem paths.
use super::*;
use serde::Deserialize;

pub(super) const TABLE_SQL: &str = "
CREATE TABLE IF NOT EXISTS m_media(
    id TEXT PRIMARY KEY,
    brand_id TEXT NOT NULL,
    dedup TEXT NOT NULL,
    data TEXT NOT NULL,
    blob BLOB NOT NULL,
    UNIQUE(brand_id,dedup),
    UNIQUE(id,brand_id),
    FOREIGN KEY(brand_id) REFERENCES m_brands(id)
);
CREATE INDEX IF NOT EXISTS m_media_brand ON m_media(brand_id);
";

const MAX_MEDIA_BYTES: usize = 1_500_000;
const MAX_BASE64_BYTES: usize = 2_000_000;
const MAX_FILENAME_CHARS: usize = 240;
const MAX_ALT_CHARS: usize = 500;
const MAX_CAPTION_CHARS: usize = 2_000;
const MAX_SOURCE_CHARS: usize = 2_000;
const MAX_RIGHTS_CHARS: usize = 4_000;
const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ImportInput {
    brand_id: String,
    filename: String,
    mime: String,
    data: String,
    #[serde(default)]
    alt: String,
    #[serde(default)]
    caption: String,
    #[serde(default)]
    source: String,
    #[serde(default)]
    rights_note: String,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListInput {
    brand_id: String,
    #[serde(default)]
    archived: Option<bool>,
    #[serde(default)]
    review_status: Option<String>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GetInput {
    brand_id: String,
    id: String,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReviseInput {
    brand_id: String,
    id: String,
    expected_revision: i64,
    filename: String,
    #[serde(default)]
    mime: Option<String>,
    #[serde(default)]
    data: Option<String>,
    #[serde(default)]
    alt: String,
    #[serde(default)]
    caption: String,
    #[serde(default)]
    source: String,
    #[serde(default)]
    rights_note: String,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReviewInput {
    brand_id: String,
    id: String,
    expected_revision: i64,
    decision: String,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ArchiveInput {
    brand_id: String,
    id: String,
    expected_revision: i64,
    archived: bool,
}

pub(super) fn encode_base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(4 * bytes.len().div_ceil(3));
    for chunk in bytes.chunks(3) {
        let value = (chunk[0] as u32) << 16
            | (chunk.get(1).copied().unwrap_or(0) as u32) << 8
            | chunk.get(2).copied().unwrap_or(0) as u32;
        out.push(BASE64[((value >> 18) & 63) as usize] as char);
        out.push(BASE64[((value >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            BASE64[((value >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            BASE64[(value & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

fn base64_value(byte: u8) -> Result<u32> {
    match byte {
        b'A'..=b'Z' => Ok((byte - b'A') as u32),
        b'a'..=b'z' => Ok((byte - b'a' + 26) as u32),
        b'0'..=b'9' => Ok((byte - b'0' + 52) as u32),
        b'+' => Ok(62),
        b'/' => Ok(63),
        _ => Err("媒体数据不是有效的 base64".into()),
    }
}

pub(super) fn decode_base64(value: &str) -> Result<Vec<u8>> {
    let input = value.as_bytes();
    if input.is_empty() || input.len() > MAX_BASE64_BYTES || input.len() % 4 != 0 {
        return Err("媒体数据为空、不是有效的 base64，或超过 1.5MB 上限".into());
    }
    let mut out = Vec::with_capacity(input.len() / 4 * 3);
    for (index, chunk) in input.chunks_exact(4).enumerate() {
        let last = index + 1 == input.len() / 4;
        let a = base64_value(chunk[0])?;
        let b = base64_value(chunk[1])?;
        let c_padding = chunk[2] == b'=';
        let d_padding = chunk[3] == b'=';
        if (!last && (c_padding || d_padding)) || (c_padding && !d_padding) {
            return Err("媒体数据不是有效的 base64".into());
        }
        if c_padding && b & 15 != 0 {
            return Err("媒体数据包含非规范 base64 编码".into());
        }
        let c = if c_padding {
            0
        } else {
            base64_value(chunk[2])?
        };
        if d_padding && !c_padding && c & 3 != 0 {
            return Err("媒体数据包含非规范 base64 编码".into());
        }
        let d = if d_padding {
            0
        } else {
            base64_value(chunk[3])?
        };
        let value = a << 18 | b << 12 | c << 6 | d;
        out.push((value >> 16) as u8);
        if !c_padding {
            out.push((value >> 8) as u8);
        }
        if !d_padding {
            out.push(value as u8);
        }
    }
    if out.len() > MAX_MEDIA_BYTES {
        return Err("单个媒体文件不能超过 1.5MB".into());
    }
    Ok(out)
}

fn detect_mime(bytes: &[u8]) -> Result<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Ok("image/png");
    }
    if bytes.len() >= 4 && bytes.starts_with(&[0xff, 0xd8, 0xff]) && bytes.ends_with(&[0xff, 0xd9])
    {
        return Ok("image/jpeg");
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Ok("image/webp");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Ok("image/gif");
    }
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        return Ok("video/mp4");
    }
    let webm_probe = &bytes[..bytes.len().min(4096)];
    if bytes.starts_with(&[0x1a, 0x45, 0xdf, 0xa3])
        && webm_probe
            .windows(4)
            .any(|window| window.eq_ignore_ascii_case(b"webm"))
    {
        return Ok("video/webm");
    }
    Err("无法识别媒体格式；仅支持 PNG、JPEG、WebP、GIF、MP4 和 WebM".into())
}

fn checked_bytes(encoded: &str, declared_mime: &str) -> Result<(Vec<u8>, String)> {
    let bytes = decode_base64(encoded)?;
    let detected = detect_mime(&bytes)?;
    if declared_mime != detected {
        return Err(format!(
            "媒体 MIME 与真实内容不符；检测到 {detected}，声明为 {declared_mime}"
        ));
    }
    let sha256 = hex::encode(Sha256::digest(&bytes));
    Ok((bytes, sha256))
}

fn clean_filename(value: &str) -> Result<String> {
    if value.is_empty() || value.chars().count() > 4096 || value.contains('\0') {
        return Err("媒体文件名称为空、过长或含非法字符".into());
    }
    // Browser paths are never needed. Keep only a display basename even for non-browser callers.
    let basename = value.replace('\\', "/");
    let basename = basename.rsplit('/').next().unwrap_or("").trim();
    let cleaned: String = basename
        .chars()
        .filter(|c| {
            !c.is_control() && !matches!(*c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
        .collect();
    let cleaned = cleaned.trim();
    if cleaned.is_empty() || cleaned.chars().count() > MAX_FILENAME_CHARS {
        return Err("媒体文件名称为空或过长".into());
    }
    Ok(cleaned.to_owned())
}

fn metadata_text(value: &str, field: &str, max: usize) -> Result<String> {
    let value = value.trim();
    if value.chars().count() > max || value.contains('\0') {
        return Err(format!("字段 {field} 过长或含非法字符"));
    }
    let lower = value.to_lowercase();
    let bytes = value.as_bytes();
    let windows_drive = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\');
    if lower.contains("file://")
        || value.starts_with('/')
        || value.starts_with("~/")
        || value.starts_with("\\\\")
        || windows_drive
    {
        return Err(format!("字段 {field} 不能包含本机文件路径或 file:// 链接"));
    }
    Ok(value.to_owned())
}

fn ensure_brand(input: &str, brand_id: &str) -> Result<()> {
    if input == brand_id {
        Ok(())
    } else {
        Err("媒体品牌与当前品牌不一致".into())
    }
}
fn ensure_active(media: &Value) -> Result<()> {
    if media["archived"] == false {
        Ok(())
    } else {
        Err("媒体已归档，请先恢复再修订或审核".into())
    }
}
fn next_revision(media: &Value) -> Result<i64> {
    number(media, "revision")?
        .checked_add(1)
        .ok_or_else(|| "媒体版本已超限".to_string())
}
fn save(c: &Connection, brand_id: &str, media: &Value) -> Result<Value> {
    let changed = c
        .execute(
            "UPDATE m_media SET data=?1 WHERE id=?2 AND brand_id=?3",
            params![media.to_string(), media["id"].as_str(), brand_id],
        )
        .map_err(db_error)?;
    if changed != 1 {
        return Err("媒体不存在或版本已变化".into());
    }
    Ok(media.clone())
}

pub(super) fn dispatch(
    c: &Connection,
    brand_id: &str,
    action: &str,
    payload: &Value,
) -> Result<Value> {
    match action {
        "media.import" => import(c, brand_id, payload),
        "media.list" => list(c, brand_id, payload),
        "media.get" => get(c, brand_id, payload),
        "media.review" => review(c, brand_id, payload),
        "media.revise" => revise(c, brand_id, payload),
        "media.setArchived" => set_archived(c, brand_id, payload),
        _ => Err("不支持的媒体操作".into()),
    }
}

fn import(c: &Connection, brand_id: &str, payload: &Value) -> Result<Value> {
    let input: ImportInput = serde_json::from_value(payload.clone())
        .map_err(|_| "媒体导入字段缺失、未知或格式错误".to_string())?;
    ensure_brand(&input.brand_id, brand_id)?;
    let filename = clean_filename(&input.filename)?;
    let alt = metadata_text(&input.alt, "alt", MAX_ALT_CHARS)?;
    let caption = metadata_text(&input.caption, "caption", MAX_CAPTION_CHARS)?;
    let source = metadata_text(&input.source, "source", MAX_SOURCE_CHARS)?;
    let rights_note = metadata_text(&input.rights_note, "rightsNote", MAX_RIGHTS_CHARS)?;
    let (bytes, dedup) = checked_bytes(&input.data, &input.mime)?;
    let existing = c
        .query_row(
            "SELECT data,blob FROM m_media WHERE brand_id=?1 AND dedup=?2",
            params![brand_id, dedup],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)),
        )
        .optional()
        .map_err(db_error)?;
    if let Some((data, blob)) = existing {
        if blob != bytes {
            return Err("媒体内容摘要不一致，未覆盖已有媒体".into());
        }
        return parse_data(data);
    }
    let timestamp = now();
    let media = json!({
        "id":id(), "brandId":brand_id, "filename":filename, "mime":input.mime,
        "size":bytes.len(), "sha256":dedup, "alt":alt, "caption":caption,
        "source":source, "rightsNote":rights_note, "reviewStatus":"pending",
        "archived":false, "revision":1, "createdAt":timestamp, "updatedAt":timestamp,
        "reviewedAt":null,
    });
    c.execute(
        "INSERT INTO m_media(id,brand_id,dedup,data,blob) VALUES(?1,?2,?3,?4,?5)",
        params![
            media["id"].as_str(),
            brand_id,
            dedup,
            media.to_string(),
            bytes
        ],
    )
    .map_err(db_error)?;
    Ok(media)
}

fn list(c: &Connection, brand_id: &str, payload: &Value) -> Result<Value> {
    let input: ListInput = serde_json::from_value(payload.clone())
        .map_err(|_| "媒体查询字段缺失、未知或格式错误".to_string())?;
    ensure_brand(&input.brand_id, brand_id)?;
    if let Some(status) = input.review_status.as_deref() {
        if !["pending", "approved", "rejected"].contains(&status) {
            return Err("不支持的媒体审核状态".into());
        }
    }
    let mut statement = c
        .prepare("SELECT data FROM m_media WHERE brand_id=?1 ORDER BY rowid DESC")
        .map_err(db_error)?;
    let rows = statement
        .query_map([brand_id], |row| row.get::<_, String>(0))
        .map_err(db_error)?;
    let mut media = Vec::new();
    for row in rows {
        let item = parse_data(row.map_err(db_error)?)?;
        if input
            .archived
            .is_none_or(|archived| item["archived"] == archived)
            && input
                .review_status
                .as_deref()
                .is_none_or(|status| item["reviewStatus"] == status)
        {
            media.push(item);
        }
    }
    Ok(json!(media))
}

fn get(c: &Connection, brand_id: &str, payload: &Value) -> Result<Value> {
    let input: GetInput = serde_json::from_value(payload.clone())
        .map_err(|_| "媒体读取字段缺失、未知或格式错误".to_string())?;
    ensure_brand(&input.brand_id, brand_id)?;
    guid(payload, "id")?;
    let (data, blob) = c
        .query_row(
            "SELECT data,blob FROM m_media WHERE id=?1 AND brand_id=?2",
            params![input.id, brand_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)),
        )
        .optional()
        .map_err(db_error)?
        .ok_or("媒体不存在或不属于当前品牌")?;
    let mut media = parse_data(data)?;
    if blob.len() != media["size"].as_u64().unwrap_or(0) as usize
        || hex::encode(Sha256::digest(&blob)) != media["sha256"].as_str().unwrap_or("")
        || detect_mime(&blob)? != media["mime"].as_str().unwrap_or("")
    {
        return Err("媒体内容校验失败，请从可信备份恢复".into());
    }
    media["data"] = json!(encode_base64(&blob));
    Ok(media)
}

fn revise(c: &Connection, brand_id: &str, payload: &Value) -> Result<Value> {
    let input: ReviseInput = serde_json::from_value(payload.clone())
        .map_err(|_| "媒体修订字段缺失、未知或格式错误".to_string())?;
    ensure_brand(&input.brand_id, brand_id)?;
    guid(payload, "id")?;
    expected_revision(input.expected_revision)?;
    let filename = clean_filename(&input.filename)?;
    let alt = metadata_text(&input.alt, "alt", MAX_ALT_CHARS)?;
    let caption = metadata_text(&input.caption, "caption", MAX_CAPTION_CHARS)?;
    let source = metadata_text(&input.source, "source", MAX_SOURCE_CHARS)?;
    let rights_note = metadata_text(&input.rights_note, "rightsNote", MAX_RIGHTS_CHARS)?;
    let replacement = match (&input.data, &input.mime) {
        (None, None) => None,
        (Some(data), Some(mime)) => Some(checked_bytes(data, mime)?),
        _ => return Err("替换媒体时必须同时提供 data 和 mime".into()),
    };
    let mut media = load(c, "m_media", brand_id, &input.id)?;
    revision(&media, payload)?;
    ensure_active(&media)?;
    media["filename"] = json!(filename);
    media["alt"] = json!(alt);
    media["caption"] = json!(caption);
    media["source"] = json!(source);
    media["rightsNote"] = json!(rights_note);
    media["reviewStatus"] = json!("pending");
    media["reviewedAt"] = Value::Null;
    media["revision"] = json!(next_revision(&media)?);
    media["updatedAt"] = json!(now());
    if let Some((bytes, dedup)) = replacement {
        media["mime"] = json!(input.mime);
        media["size"] = json!(bytes.len());
        media["sha256"] = json!(dedup);
        let changed = c.execute(
            "UPDATE m_media SET dedup=?1,data=?2,blob=?3 WHERE id=?4 AND brand_id=?5",
            params![dedup, media.to_string(), bytes, input.id, brand_id],
        );
        match changed {
            Ok(1) => Ok(media),
            Ok(_) => Err("媒体不存在或版本已变化".into()),
            Err(_) => Err("相同内容已存在于当前品牌，或媒体无法保存".into()),
        }
    } else {
        save(c, brand_id, &media)
    }
}

fn review(c: &Connection, brand_id: &str, payload: &Value) -> Result<Value> {
    let input: ReviewInput = serde_json::from_value(payload.clone())
        .map_err(|_| "媒体审核字段缺失、未知或格式错误".to_string())?;
    ensure_brand(&input.brand_id, brand_id)?;
    guid(payload, "id")?;
    expected_revision(input.expected_revision)?;
    if !["approved", "rejected"].contains(&input.decision.as_str()) {
        return Err("媒体审核决定仅支持 approved 或 rejected".into());
    }
    let mut media = load(c, "m_media", brand_id, &input.id)?;
    revision(&media, payload)?;
    ensure_active(&media)?;
    if media["reviewStatus"] != "pending" {
        return Err("仅待审核媒体可以审核，请刷新后核对当前状态".into());
    }
    if input.decision == "approved"
        && (media["source"].as_str().unwrap_or("").trim().is_empty()
            || media["rightsNote"].as_str().unwrap_or("").trim().is_empty())
    {
        return Err("媒体缺少来源或权利说明，不能审核通过".into());
    }
    media["reviewStatus"] = json!(input.decision);
    media["reviewedAt"] = json!(now());
    media["revision"] = json!(next_revision(&media)?);
    media["updatedAt"] = json!(now());
    save(c, brand_id, &media)
}

fn set_archived(c: &Connection, brand_id: &str, payload: &Value) -> Result<Value> {
    let input: ArchiveInput = serde_json::from_value(payload.clone())
        .map_err(|_| "媒体归档字段缺失、未知或格式错误".to_string())?;
    ensure_brand(&input.brand_id, brand_id)?;
    guid(payload, "id")?;
    expected_revision(input.expected_revision)?;
    let mut media = load(c, "m_media", brand_id, &input.id)?;
    revision(&media, payload)?;
    if media["archived"] == input.archived {
        return Err("媒体归档状态未变化，请刷新后核对".into());
    }
    media["archived"] = json!(input.archived);
    media["revision"] = json!(next_revision(&media)?);
    media["updatedAt"] = json!(now());
    save(c, brand_id, &media)
}

fn expected_revision(value: i64) -> Result<()> {
    if value > 0 {
        Ok(())
    } else {
        Err("请检查版本字段 expectedRevision".into())
    }
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn base64_is_strict_and_bounded() {
        for bytes in [vec![0], vec![0, 1], vec![0, 1, 2], (0..=255).collect()] {
            assert_eq!(decode_base64(&encode_base64(&bytes)).unwrap(), bytes);
        }
        for invalid in ["", "A", "AA=A", "AA==AAAA", "AB==", "AAB=", "****"] {
            assert!(decode_base64(invalid).is_err(), "accepted {invalid}");
        }
    }

    #[test]
    fn signatures_do_not_trust_extensions() {
        assert_eq!(detect_mime(b"\x89PNG\r\n\x1a\n").unwrap(), "image/png");
        assert_eq!(
            detect_mime(&[0xff, 0xd8, 0xff, 0xd9]).unwrap(),
            "image/jpeg"
        );
        assert_eq!(
            detect_mime(b"RIFF\x00\x00\x00\x00WEBP").unwrap(),
            "image/webp"
        );
        assert_eq!(detect_mime(b"GIF89a").unwrap(), "image/gif");
        assert_eq!(
            detect_mime(b"\x00\x00\x00\x18ftypisom").unwrap(),
            "video/mp4"
        );
        assert_eq!(detect_mime(b"\x1a\x45\xdf\xa3webm").unwrap(), "video/webm");
        assert!(detect_mime(b"\x89PNG\r\n\x1a").is_err());
        assert!(detect_mime(b"\x1a\x45\xdf\xa3matroska").is_err());
    }
}
