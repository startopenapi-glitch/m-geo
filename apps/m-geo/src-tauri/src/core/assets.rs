//! Manually maintained keyword/title assets. Assets are not product facts and never publish.
use super::*;
use serde::Deserialize;

pub(super) const TABLE_SQL: &str = "
CREATE TABLE IF NOT EXISTS m_assets(
    id TEXT PRIMARY KEY,
    brand_id TEXT NOT NULL,
    product_id TEXT,
    request_id TEXT NOT NULL,
    request_hash TEXT NOT NULL,
    created_at TEXT NOT NULL,
    data TEXT NOT NULL,
    UNIQUE(brand_id,request_id),
    UNIQUE(id,brand_id),
    FOREIGN KEY(brand_id) REFERENCES m_brands(id),
    FOREIGN KEY(product_id,brand_id) REFERENCES m_products(id,brand_id)
);
CREATE INDEX IF NOT EXISTS m_assets_brand_created
    ON m_assets(brand_id,created_at DESC,id DESC);
";
const MAX_TEXT_CHARS: usize = 500;
const MAX_SOURCE_CHARS: usize = 2000;
const MAX_NOTE_CHARS: usize = 2000;

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum AssetKind {
    Keyword,
    Title,
}
impl AssetKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Keyword => "keyword",
            Self::Title => "title",
        }
    }
}
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum ReviewDecision {
    Approved,
    Rejected,
}
impl ReviewDecision {
    fn as_str(self) -> &'static str {
        match self {
            Self::Approved => "approved",
            Self::Rejected => "rejected",
        }
    }
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListInput {
    brand_id: String,
    #[serde(default)]
    kind: Option<AssetKind>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateInput {
    brand_id: String,
    kind: AssetKind,
    text: String,
    source: String,
    #[serde(default)]
    product_id: Option<String>,
    request_id: String,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReviseInput {
    brand_id: String,
    id: String,
    expected_revision: i64,
    text: String,
    source: String,
    #[serde(default)]
    product_id: Option<String>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReviewInput {
    brand_id: String,
    id: String,
    expected_revision: i64,
    decision: ReviewDecision,
    #[serde(default)]
    note: Option<String>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ArchiveInput {
    brand_id: String,
    id: String,
    expected_revision: i64,
    archived: bool,
    #[serde(default)]
    note: Option<String>,
}

fn bounded(value: &str, field: &str, max: usize) -> Result<String> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > max || value.contains('\0') {
        return Err(format!("字段 {field} 为空、过长或含非法字符"));
    }
    Ok(value.to_owned())
}
fn optional_note(value: Option<&str>) -> Result<String> {
    let value = value.unwrap_or("").trim();
    if value.chars().count() > MAX_NOTE_CHARS || value.contains('\0') {
        return Err("审核理由过长或含非法字符".into());
    }
    Ok(value.to_owned())
}
fn expected_revision(value: i64) -> Result<i64> {
    if value <= 0 {
        return Err("请检查版本字段 expectedRevision".into());
    }
    Ok(value)
}
fn parse_create(brand_id: &str, payload: &Value) -> Result<CreateInput> {
    let input: CreateInput = serde_json::from_value(payload.clone())
        .map_err(|_| "内容资产字段缺失、未知或格式错误".to_string())?;
    if input.brand_id != brand_id {
        return Err("内容资产品牌与当前品牌不一致".into());
    }
    guid(payload, "requestId")?;
    if input.product_id.is_some() {
        guid(payload, "productId")?;
    }
    bounded(&input.text, "text", MAX_TEXT_CHARS)?;
    bounded(&input.source, "source", MAX_SOURCE_CHARS)?;
    Ok(input)
}
fn create_fingerprint(input: &CreateInput) -> String {
    fingerprint(&json!({
        "brandId":input.brand_id,
        "kind":input.kind.as_str(),
        "text":input.text.trim(),
        "source":input.source.trim(),
        "productId":input.product_id,
        "requestId":input.request_id,
    }))
}
fn existing_creation(
    c: &Connection,
    brand_id: &str,
    input: &CreateInput,
) -> Result<Option<String>> {
    let existing = c
        .query_row(
            "SELECT id,request_hash FROM m_assets WHERE brand_id=?1 AND request_id=?2",
            params![brand_id, input.request_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(db_error)?;
    match existing {
        Some((id, hash)) if hash == create_fingerprint(input) => Ok(Some(id)),
        Some(_) => Err("内容资产创建请求标识已用于不同内容".into()),
        None => Ok(None),
    }
}
pub(super) fn create_is_replay(c: &Connection, payload: &Value) -> Result<bool> {
    let brand_id = guid(payload, "brandId")?;
    let input = parse_create(&brand_id, payload)?;
    Ok(existing_creation(c, &brand_id, &input)?.is_some())
}

pub(super) fn list(c: &Connection, brand_id: &str, payload: &Value) -> Result<Value> {
    let input: ListInput = serde_json::from_value(payload.clone())
        .map_err(|_| "内容资产查询字段缺失、未知或格式错误".to_string())?;
    if input.brand_id != brand_id {
        return Err("内容资产查询品牌与当前品牌不一致".into());
    }
    let mut statement = c
        .prepare("SELECT data FROM m_assets WHERE brand_id=?1 ORDER BY created_at DESC,id DESC")
        .map_err(db_error)?;
    let rows = statement
        .query_map([brand_id], |row| row.get::<_, String>(0))
        .map_err(db_error)?;
    let mut assets = Vec::new();
    for row in rows {
        let asset = parse_data(row.map_err(db_error)?)?;
        if input.kind.is_none_or(|kind| asset["kind"] == kind.as_str()) {
            assets.push(asset);
        }
    }
    Ok(json!(assets))
}

pub(super) fn create(c: &Connection, brand_id: &str, payload: &Value) -> Result<Value> {
    let input = parse_create(brand_id, payload)?;
    if let Some(existing) = existing_creation(c, brand_id, &input)? {
        return load(c, "m_assets", brand_id, &existing);
    }
    if let Some(product_id) = &input.product_id {
        load(c, "m_products", brand_id, product_id)?;
    }
    let timestamp = now();
    let request_hash = create_fingerprint(&input);
    let asset_id = id();
    let mut asset = json!({
        "id":asset_id,
        "brandId":brand_id,
        "kind":input.kind.as_str(),
        "productId":input.product_id,
        "text":input.text.trim(),
        "source":input.source.trim(),
        "reviewStatus":"pending",
        "reviewNote":"",
        "reviewedAt":null,
        "archivedAt":null,
        "archiveNote":"",
        "revision":1,
        "createdAt":timestamp,
        "updatedAt":timestamp,
        "history":[],
    });
    let entry = history_entry(&asset, "created")?;
    asset["history"].as_array_mut().unwrap().push(entry);
    c.execute(
        "INSERT INTO m_assets(id,brand_id,product_id,request_id,request_hash,created_at,data) VALUES(?1,?2,?3,?4,?5,?6,?7)",
        params![
            asset["id"].as_str(),
            brand_id,
            asset["productId"].as_str(),
            input.request_id,
            request_hash,
            timestamp,
            asset.to_string()
        ],
    )
    .map_err(db_error)?;
    Ok(asset)
}

fn history_entry(asset: &Value, action: &str) -> Result<Value> {
    Ok(json!({
        "revision":number(asset, "revision")?,
        "action":action,
        "text":asset["text"],
        "source":asset["source"],
        "productId":asset["productId"],
        "reviewStatus":asset["reviewStatus"],
        "reviewNote":asset["reviewNote"],
        "reviewedAt":asset["reviewedAt"],
        "changedAt":asset["updatedAt"],
    }))
}
fn next_revision(asset: &Value) -> Result<i64> {
    number(asset, "revision")?
        .checked_add(1)
        .ok_or_else(|| "内容资产版本已超限".to_string())
}
fn save_revision(c: &Connection, brand_id: &str, mut asset: Value, action: &str) -> Result<Value> {
    let entry = history_entry(&asset, action)?;
    asset["history"]
        .as_array_mut()
        .ok_or("内容资产历史损坏")?
        .push(entry);
    let updated = c
        .execute(
            "UPDATE m_assets SET product_id=?1,data=?2 WHERE id=?3 AND brand_id=?4",
            params![
                asset["productId"].as_str(),
                asset.to_string(),
                asset["id"].as_str(),
                brand_id
            ],
        )
        .map_err(db_error)?;
    if updated != 1 {
        return Err("内容资产不存在或版本已变化".into());
    }
    Ok(asset)
}

fn ensure_active(asset: &Value) -> Result<()> {
    if asset["archivedAt"].is_null() {
        Ok(())
    } else {
        Err("内容资产已归档，请先恢复再修订、审核或用于新稿件".into())
    }
}

pub(super) fn revise(c: &Connection, brand_id: &str, payload: &Value) -> Result<Value> {
    let input: ReviseInput = serde_json::from_value(payload.clone())
        .map_err(|_| "内容资产修订字段缺失、未知或格式错误".to_string())?;
    if input.brand_id != brand_id {
        return Err("内容资产修订品牌与当前品牌不一致".into());
    }
    expected_revision(input.expected_revision)?;
    let text = bounded(&input.text, "text", MAX_TEXT_CHARS)?;
    let source = bounded(&input.source, "source", MAX_SOURCE_CHARS)?;
    if input.product_id.is_some() {
        guid(payload, "productId")?;
    }
    let mut asset = load(c, "m_assets", brand_id, &input.id)?;
    revision(&asset, payload)?;
    ensure_active(&asset)?;
    if let Some(product_id) = &input.product_id {
        load(c, "m_products", brand_id, product_id)?;
    }
    asset["text"] = json!(text);
    asset["source"] = json!(source);
    asset["productId"] = json!(input.product_id);
    asset["reviewStatus"] = json!("pending");
    asset["reviewNote"] = json!("");
    asset["reviewedAt"] = Value::Null;
    asset["revision"] = json!(next_revision(&asset)?);
    asset["updatedAt"] = json!(now());
    save_revision(c, brand_id, asset, "revised")
}

pub(super) fn review(c: &Connection, brand_id: &str, payload: &Value) -> Result<Value> {
    let input: ReviewInput = serde_json::from_value(payload.clone())
        .map_err(|_| "内容资产审核字段缺失、未知或格式错误".to_string())?;
    if input.brand_id != brand_id {
        return Err("内容资产审核品牌与当前品牌不一致".into());
    }
    expected_revision(input.expected_revision)?;
    let note = optional_note(input.note.as_deref())?;
    if matches!(input.decision, ReviewDecision::Rejected) && note.is_empty() {
        return Err("驳回内容资产必须填写审核理由".into());
    }
    let mut asset = load(c, "m_assets", brand_id, &input.id)?;
    revision(&asset, payload)?;
    ensure_active(&asset)?;
    if asset["reviewStatus"] != "pending" {
        return Err("仅待审核内容资产可以审核，请刷新后核对当前状态".into());
    }
    asset["reviewStatus"] = json!(input.decision.as_str());
    asset["reviewNote"] = json!(note);
    asset["reviewedAt"] = json!(now());
    asset["revision"] = json!(next_revision(&asset)?);
    asset["updatedAt"] = json!(now());
    save_revision(c, brand_id, asset, input.decision.as_str())
}

pub(super) fn set_archived(c: &Connection, brand_id: &str, payload: &Value) -> Result<Value> {
    let input: ArchiveInput = serde_json::from_value(payload.clone())
        .map_err(|_| "内容资产归档字段缺失、未知或格式错误".to_string())?;
    if input.brand_id != brand_id {
        return Err("内容资产归档品牌与当前品牌不一致".into());
    }
    expected_revision(input.expected_revision)?;
    let note = optional_note(input.note.as_deref())?;
    let mut asset = load(c, "m_assets", brand_id, &input.id)?;
    revision(&asset, payload)?;
    if input.archived == !asset["archivedAt"].is_null() {
        return Err("内容资产归档状态未变化，请刷新后核对".into());
    }
    asset["archivedAt"] = if input.archived {
        json!(now())
    } else {
        Value::Null
    };
    asset["archiveNote"] = if input.archived {
        json!(note)
    } else {
        json!("")
    };
    asset["revision"] = json!(next_revision(&asset)?);
    asset["updatedAt"] = json!(now());
    save_revision(
        c,
        brand_id,
        asset,
        if input.archived {
            "archived"
        } else {
            "restored"
        },
    )
}
