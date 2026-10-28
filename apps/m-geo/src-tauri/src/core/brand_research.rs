//! Brand-owned profile metadata and locally sourced question research.
//! No network collection is performed here: suggested entries are explicitly local input.
use chrono::NaiveDate;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};

const MAX_ALIASES: usize = 20;
const MAX_RESEARCH_ITEMS: usize = 5000;
const DEFAULT_LIST_LIMIT: usize = 1000;

type Result<T> = std::result::Result<T, String>;

pub(super) const TABLE_SQL: &str = "
CREATE TABLE IF NOT EXISTS m_question_research(
    id TEXT PRIMARY KEY,
    brand_id TEXT NOT NULL,
    product_id TEXT,
    dedup TEXT NOT NULL,
    data TEXT NOT NULL,
    UNIQUE(brand_id,dedup),
    UNIQUE(id,brand_id),
    FOREIGN KEY(brand_id) REFERENCES m_brands(id),
    FOREIGN KEY(product_id,brand_id) REFERENCES m_products(id,brand_id)
);
CREATE INDEX IF NOT EXISTS m_question_research_brand
    ON m_question_research(brand_id,product_id);
";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ResearchIntent {
    Selection,
    Comparison,
    Usage,
    AfterSales,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum SourceKind {
    Manual,
    Imported,
    Suggested,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum ResearchStatus {
    Candidate,
    Confirmed,
    Archived,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ResearchEntry {
    pub id: String,
    pub brand_id: String,
    pub product_id: Option<String>,
    pub question: String,
    pub intent: ResearchIntent,
    pub source_kind: SourceKind,
    pub source_label: String,
    pub source_url: String,
    pub observed_at: String,
    pub cluster_key: String,
    pub status: ResearchStatus,
    pub revision: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProfileSaveInput {
    brand_id: String,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    official_url: String,
    #[serde(default)]
    manufacturer: String,
    #[serde(default)]
    category: String,
    #[serde(default)]
    description: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ResearchSaveInput {
    brand_id: String,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    expected_revision: Option<i64>,
    #[serde(default)]
    product_id: Option<String>,
    question: String,
    intent: ResearchIntent,
    source_kind: SourceKind,
    source_label: String,
    #[serde(default)]
    source_url: String,
    observed_at: String,
    #[serde(default)]
    cluster_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ResearchListInput {
    brand_id: String,
    #[serde(default)]
    status: Option<ResearchStatus>,
    #[serde(default)]
    source_kind: Option<SourceKind>,
    #[serde(default)]
    product_id: Option<String>,
    #[serde(default)]
    cluster_key: Option<String>,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    observed_from: Option<String>,
    #[serde(default)]
    observed_to: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TransitionInput {
    brand_id: String,
    id: String,
    expected_revision: i64,
}

/// Extension point for core dispatch. Unknown actions return `Ok(None)` so the
/// parent can continue its existing dispatch chain without a fallback error.
pub(super) fn dispatch(
    c: &Connection,
    brand_id: &str,
    action: &str,
    payload: &Value,
) -> Result<Option<Value>> {
    let result = match action {
        "brand.profile.save" => save_profile(c, brand_id, payload)?,
        "research.save" => save_research(c, brand_id, payload)?,
        "research.list" => list_research(c, brand_id, payload)?,
        "research.archive" => transition(c, brand_id, payload, ResearchStatus::Archived)?,
        "research.confirm" => transition(c, brand_id, payload, ResearchStatus::Confirmed)?,
        "research.confirmToQuestion" => confirm_to_question(c, brand_id, payload)?,
        _ => return Ok(None),
    };
    Ok(Some(result))
}

fn ensure_brand(expected: &str, actual: &str) -> Result<()> {
    if expected == actual {
        Ok(())
    } else {
        Err("请求品牌与当前品牌不一致".into())
    }
}

fn validate_text(value: &str, field: &str, max: usize, required: bool) -> Result<String> {
    let trimmed = value.trim();
    if (required && trimmed.is_empty()) || trimmed.chars().count() > max || trimmed.contains('\0') {
        return Err(format!("字段 {field} 为空、过长或含非法字符"));
    }
    Ok(trimmed.to_owned())
}

fn validate_uuid(value: &str, field: &str) -> Result<()> {
    uuid::Uuid::parse_str(value).map_err(|_| format!("请检查对象标识 {field}"))?;
    Ok(())
}

fn validate_revision(value: i64) -> Result<i64> {
    if value > 0 {
        Ok(value)
    } else {
        Err("请检查版本字段 expectedRevision".into())
    }
}

fn save_profile(c: &Connection, brand_id: &str, payload: &Value) -> Result<Value> {
    let input: ProfileSaveInput = serde_json::from_value(payload.clone())
        .map_err(|_| "品牌资料字段缺失、未知或格式错误".to_string())?;
    ensure_brand(brand_id, &input.brand_id)?;
    let mut current = super::brand(c, brand_id)?;
    let mut seen = HashSet::new();
    let mut aliases = Vec::new();
    for alias in input.aliases {
        if alias.trim().is_empty() {
            continue;
        }
        let alias = validate_text(&alias, "aliases", 100, true)?;
        if seen.insert(alias.to_lowercase()) {
            aliases.push(alias);
        }
    }
    if aliases.len() > MAX_ALIASES {
        return Err("品牌别名最多保存20个".into());
    }
    let official_url = validate_text(&input.official_url, "officialUrl", 2000, false)?;
    super::web_url(&official_url, false)?;
    let profile = json!({
        "aliases": aliases,
        "officialUrl": official_url,
        "manufacturer": validate_text(&input.manufacturer,"manufacturer",200,false)?,
        "category": validate_text(&input.category,"category",200,false)?,
        "description": validate_text(&input.description,"description",5000,false)?,
        "updatedAt": super::now(),
    });
    current["profile"] = profile;
    let updated = c
        .execute(
            "UPDATE m_brands SET data=?1 WHERE id=?2",
            params![current.to_string(), brand_id],
        )
        .map_err(super::db_error)?;
    if updated != 1 {
        return Err("品牌不存在或资料保存失败".into());
    }
    Ok(current)
}

fn parse_entry(data: &str) -> Result<ResearchEntry> {
    serde_json::from_str(data).map_err(|_| "问题研究记录格式损坏，请检查备份".into())
}

fn stored_entry(c: &Connection, brand_id: &str, id: &str) -> Result<ResearchEntry> {
    let data = c
        .query_row(
            "SELECT data FROM m_question_research WHERE id=?1 AND brand_id=?2",
            params![id, brand_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(super::db_error)?
        .ok_or("问题研究记录不存在或不属于当前品牌")?;
    parse_entry(&data)
}

/// Deterministic conservative normalization. It folds case, full-width ASCII,
/// whitespace and punctuation, plus common question politeness suffixes; it does
/// not claim semantic equivalence or use a model/network service.
pub(super) fn normalize_question(value: &str) -> String {
    let mut normalized = String::new();
    for character in value.trim().chars().flat_map(char::to_lowercase) {
        let character = match character {
            '！'..='～' => char::from_u32(character as u32 - 0xfee0).unwrap_or(character),
            _ => character,
        };
        if character.is_alphanumeric() {
            normalized.push(character);
        }
    }
    let without_prefix = normalized.strip_prefix("请问").unwrap_or(&normalized);
    let without_suffix = without_prefix
        .strip_suffix('吗')
        .or_else(|| without_prefix.strip_suffix('呢'))
        .unwrap_or(without_prefix);
    without_suffix.to_owned()
}

fn dedup(product_id: Option<&str>, question: &str) -> String {
    let scope = product_id.unwrap_or("");
    let normalized = normalize_question(question);
    hex::encode(Sha256::digest(
        format!("m-geo-question-research-v1\0{scope}\0{normalized}").as_bytes(),
    ))
}

fn validate_product(c: &Connection, brand_id: &str, product_id: Option<&str>) -> Result<()> {
    if let Some(product_id) = product_id {
        validate_uuid(product_id, "productId")?;
        super::load(c, "m_products", brand_id, product_id)?;
    }
    Ok(())
}

fn parse_save(brand_id: &str, payload: &Value) -> Result<ResearchSaveInput> {
    let input: ResearchSaveInput = serde_json::from_value(payload.clone())
        .map_err(|_| "问题研究字段缺失、未知或格式错误".to_string())?;
    ensure_brand(brand_id, &input.brand_id)?;
    validate_text(&input.question, "question", 1000, true)?;
    if normalize_question(&input.question).is_empty() {
        return Err("研究问题必须包含文字或数字".into());
    }
    validate_text(&input.source_label, "sourceLabel", 500, true)?;
    super::web_url(input.source_url.trim(), false)?;
    validate_text(&input.source_url, "sourceUrl", 2000, false)?;
    super::date_time(payload, "observedAt")?;
    validate_text(&input.cluster_key, "clusterKey", 120, false)?;
    if let Some(id) = &input.id {
        validate_uuid(id, "id")?;
        validate_revision(
            input
                .expected_revision
                .ok_or("更新研究记录必须提供 expectedRevision")?,
        )?;
    } else if input.expected_revision.is_some() {
        return Err("新建研究记录不应提供 expectedRevision".into());
    }
    Ok(input)
}

fn save_research(c: &Connection, brand_id: &str, payload: &Value) -> Result<Value> {
    let input = parse_save(brand_id, payload)?;
    validate_product(c, brand_id, input.product_id.as_deref())?;
    let timestamp = super::now();
    let previous = match &input.id {
        Some(id) => Some(stored_entry(c, brand_id, id)?),
        None => None,
    };
    if let Some(previous) = &previous {
        if previous.revision != input.expected_revision.unwrap_or_default() {
            return Err("研究记录已被其他操作修改，请刷新后重试".into());
        }
        if previous.status == ResearchStatus::Archived {
            return Err("已归档研究记录不能直接编辑".into());
        }
    } else {
        let count: i64 = c
            .query_row(
                "SELECT count(*) FROM m_question_research WHERE brand_id=?1",
                [brand_id],
                |row| row.get(0),
            )
            .map_err(super::db_error)?;
        if count >= MAX_RESEARCH_ITEMS as i64 {
            return Err("每个品牌最多保存5000条问题研究记录".into());
        }
    }
    let entry = ResearchEntry {
        id: input.id.clone().unwrap_or_else(super::id),
        brand_id: brand_id.to_owned(),
        product_id: input.product_id,
        question: validate_text(&input.question, "question", 1000, true)?,
        intent: input.intent,
        source_kind: input.source_kind,
        source_label: validate_text(&input.source_label, "sourceLabel", 500, true)?,
        source_url: validate_text(&input.source_url, "sourceUrl", 2000, false)?,
        observed_at: super::date_time(payload, "observedAt")?,
        cluster_key: validate_text(&input.cluster_key, "clusterKey", 120, false)?,
        status: previous
            .as_ref()
            .map(|entry| entry.status)
            .unwrap_or(ResearchStatus::Candidate),
        revision: previous
            .as_ref()
            .map(|entry| entry.revision.checked_add(1).ok_or("研究记录版本已超限"))
            .transpose()?
            .unwrap_or(1),
        created_at: previous
            .as_ref()
            .map(|entry| entry.created_at.clone())
            .unwrap_or_else(|| timestamp.clone()),
        updated_at: timestamp,
    };
    let data = serde_json::to_string(&entry).map_err(|_| "问题研究记录序列化失败")?;
    let key = dedup(entry.product_id.as_deref(), &entry.question);
    let changed = if previous.is_some() {
        c.execute(
            "UPDATE m_question_research SET product_id=?1,dedup=?2,data=?3 WHERE id=?4 AND brand_id=?5",
            params![entry.product_id, key, data, entry.id, brand_id],
        )
    } else {
        c.execute(
            "INSERT INTO m_question_research(id,brand_id,product_id,dedup,data) VALUES(?1,?2,?3,?4,?5)",
            params![entry.id, brand_id, entry.product_id, key, data],
        )
    }
    .map_err(|error| {
        if matches!(error, rusqlite::Error::SqliteFailure(_, _))
            && c.query_row(
                "SELECT 1 FROM m_question_research WHERE brand_id=?1 AND dedup=?2",
                params![brand_id, key],
                |_| Ok(()),
            )
            .optional()
            .ok()
            .flatten()
            .is_some()
        {
            "同一商品范围内已存在相同规范化问题".to_string()
        } else {
            super::db_error(error)
        }
    })?;
    if changed != 1 {
        return Err("研究记录不存在或不属于当前品牌".into());
    }
    serde_json::to_value(entry).map_err(|_| "问题研究记录序列化失败".into())
}

fn optional_date(value: Option<&str>, field: &str) -> Result<Option<NaiveDate>> {
    value
        .map(|value| {
            NaiveDate::parse_from_str(value, "%Y-%m-%d")
                .map_err(|_| format!("字段 {field} 必须为 YYYY-MM-DD"))
        })
        .transpose()
}

fn list_research(c: &Connection, brand_id: &str, payload: &Value) -> Result<Value> {
    let input: ResearchListInput = serde_json::from_value(payload.clone())
        .map_err(|_| "问题研究筛选字段未知或格式错误".to_string())?;
    ensure_brand(brand_id, &input.brand_id)?;
    if let Some(product_id) = &input.product_id {
        validate_uuid(product_id, "productId")?;
    }
    let from = optional_date(input.observed_from.as_deref(), "observedFrom")?;
    let to = optional_date(input.observed_to.as_deref(), "observedTo")?;
    if from.zip(to).is_some_and(|(from, to)| from > to) {
        return Err("观察开始日期不能晚于结束日期".into());
    }
    let limit = input.limit.unwrap_or(DEFAULT_LIST_LIMIT);
    if !(1..=MAX_RESEARCH_ITEMS).contains(&limit) {
        return Err("研究列表数量必须为1至5000".into());
    }
    let query = input.query.as_deref().unwrap_or("").to_lowercase();
    let cluster = input.cluster_key.as_deref().map(str::trim);
    let mut statement = c
        .prepare("SELECT data FROM m_question_research WHERE brand_id=?1 ORDER BY rowid DESC")
        .map_err(super::db_error)?;
    let rows = statement
        .query_map([brand_id], |row| row.get::<_, String>(0))
        .map_err(super::db_error)?;
    let mut items = Vec::new();
    for row in rows {
        let entry = parse_entry(&row.map_err(super::db_error)?)?;
        let observed_date = NaiveDate::parse_from_str(&entry.observed_at[..10], "%Y-%m-%d")
            .map_err(|_| "问题研究观察日期损坏")?;
        if input.status.is_some_and(|status| status != entry.status)
            || input
                .source_kind
                .is_some_and(|kind| kind != entry.source_kind)
            || input
                .product_id
                .as_ref()
                .is_some_and(|id| entry.product_id.as_ref() != Some(id))
            || cluster.is_some_and(|key| entry.cluster_key != key)
            || from.is_some_and(|date| observed_date < date)
            || to.is_some_and(|date| observed_date > date)
            || !query.is_empty()
                && !entry.question.to_lowercase().contains(&query)
                && !entry.source_label.to_lowercase().contains(&query)
        {
            continue;
        }
        items.push(entry);
        if items.len() > limit {
            break;
        }
    }
    let truncated = items.len() > limit;
    items.truncate(limit);
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for entry in &items {
        if !entry.cluster_key.is_empty() {
            groups
                .entry(entry.cluster_key.clone())
                .or_default()
                .push(entry.id.clone());
        }
    }
    let clusters: Vec<Value> = groups
        .into_iter()
        .filter(|(_, ids)| ids.len() > 1)
        .map(|(cluster_key, ids)| {
            json!({"clusterKey":cluster_key,"count":ids.len(),"researchIds":ids})
        })
        .collect();
    Ok(json!({"items":items,"clusters":clusters,"truncated":truncated}))
}

fn transition(
    c: &Connection,
    brand_id: &str,
    payload: &Value,
    target: ResearchStatus,
) -> Result<Value> {
    let input: TransitionInput = serde_json::from_value(payload.clone())
        .map_err(|_| "研究状态操作字段缺失、未知或格式错误".to_string())?;
    ensure_brand(brand_id, &input.brand_id)?;
    validate_uuid(&input.id, "id")?;
    validate_revision(input.expected_revision)?;
    let mut entry = stored_entry(c, brand_id, &input.id)?;
    if entry.revision != input.expected_revision {
        return Err("研究记录已被其他操作修改，请刷新后重试".into());
    }
    match target {
        ResearchStatus::Confirmed if entry.status != ResearchStatus::Candidate => {
            return Err("只有候选研究问题可以确认".into())
        }
        ResearchStatus::Archived if entry.status == ResearchStatus::Archived => {
            return Err("研究问题已经归档".into())
        }
        _ => {}
    }
    entry.status = target;
    entry.revision = entry.revision.checked_add(1).ok_or("研究记录版本已超限")?;
    entry.updated_at = super::now();
    write_entry(c, &entry)?;
    serde_json::to_value(entry).map_err(|_| "问题研究记录序列化失败".into())
}

fn write_entry(c: &Connection, entry: &ResearchEntry) -> Result<()> {
    let updated = c
        .execute(
            "UPDATE m_question_research SET data=?1 WHERE id=?2 AND brand_id=?3",
            params![
                serde_json::to_string(entry).map_err(|_| "问题研究记录序列化失败")?,
                entry.id,
                entry.brand_id
            ],
        )
        .map_err(super::db_error)?;
    if updated != 1 {
        return Err("研究记录不存在或版本已变化".into());
    }
    Ok(())
}

fn confirm_to_question(c: &Connection, brand_id: &str, payload: &Value) -> Result<Value> {
    let input: TransitionInput = serde_json::from_value(payload.clone())
        .map_err(|_| "加入固定问题集字段缺失、未知或格式错误".to_string())?;
    ensure_brand(brand_id, &input.brand_id)?;
    validate_uuid(&input.id, "id")?;
    validate_revision(input.expected_revision)?;
    let research = stored_entry(c, brand_id, &input.id)?;
    if research.revision != input.expected_revision {
        return Err("研究记录已被其他操作修改，请刷新后重试".into());
    }
    if research.status != ResearchStatus::Confirmed {
        return Err("请先确认研究问题，再加入固定问题集".into());
    }
    let product_id = research
        .product_id
        .as_deref()
        .ok_or("研究问题关联商品后才能加入固定问题集")?;
    super::load(c, "m_products", brand_id, product_id)?;
    let brand = super::brand(c, brand_id)?;
    let source = if research.source_url.is_empty() {
        research.source_label.clone()
    } else {
        format!("{}；{}", research.source_label, research.source_url)
    };
    let question = super::save_question(
        c,
        brand_id,
        &json!({
            "productId":product_id,
            "text":research.question.clone(),
            "intent":research.intent,
            "origin":if research.source_kind == SourceKind::Suggested {"suggestion"} else {"customer"},
            "source":source,
            "branded":super::mentions_brand(&research.question,brand["name"].as_str().unwrap_or("")),
            "confirmed":true,
        }),
    )?;
    let research_value = serde_json::to_value(&research).map_err(|_| "问题研究记录序列化失败")?;
    Ok(json!({
        "id":research.id,
        "brandId":research.brand_id,
        "research":research_value,
        "question":question,
    }))
}
