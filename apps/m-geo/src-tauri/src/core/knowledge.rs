//! Knowledge documents, deterministic chunks, review history, and brand-scoped persistence.
use chrono::{DateTime, NaiveDate, SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use uuid::Uuid;

const MAX_CONTENT_CHARS: usize = 200_000;
const MAX_CHUNK_CHARS: usize = 1_200;
const MAX_CHUNKS: usize = 200;
const MAX_TITLE_CHARS: usize = 300;
const MAX_SOURCE_CHARS: usize = 2_000;
const MAX_NOTE_CHARS: usize = 2_000;
const MAX_DOCUMENTS_PER_BRAND: i64 = 100;

type Result<T> = std::result::Result<T, String>;

pub(super) const TABLE_SQL: &str = "
CREATE TABLE IF NOT EXISTS m_knowledge_documents(
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
CREATE INDEX IF NOT EXISTS m_knowledge_documents_brand_created
    ON m_knowledge_documents(brand_id,created_at DESC,id DESC);
CREATE TABLE IF NOT EXISTS m_knowledge_reviews(
    id TEXT PRIMARY KEY,
    brand_id TEXT NOT NULL,
    document_id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    data TEXT NOT NULL,
    UNIQUE(document_id,revision),
    FOREIGN KEY(document_id,brand_id) REFERENCES m_knowledge_documents(id,brand_id),
    FOREIGN KEY(brand_id) REFERENCES m_brands(id)
);
CREATE INDEX IF NOT EXISTS m_knowledge_reviews_document
    ON m_knowledge_reviews(document_id,revision);
";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum KnowledgeFormat {
    Markdown,
    Text,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum KnowledgeRisk {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum KnowledgeReviewStatus {
    Pending,
    Approved,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct KnowledgeChunk {
    pub id: String,
    pub ordinal: usize,
    pub text: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct KnowledgeReview {
    pub id: String,
    pub document_id: String,
    pub revision: u64,
    pub decision: KnowledgeReviewStatus,
    pub note: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct KnowledgeDocument {
    pub id: String,
    pub brand_id: String,
    pub product_id: Option<String>,
    pub title: String,
    pub source: String,
    pub format: KnowledgeFormat,
    pub effective_at: Option<String>,
    pub risk: KnowledgeRisk,
    pub content: String,
    pub review_status: KnowledgeReviewStatus,
    pub reviewed_at: Option<String>,
    pub revision: u64,
    pub created_at: String,
    pub updated_at: String,
    pub chunks: Vec<KnowledgeChunk>,
    pub reviews: Vec<KnowledgeReview>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct KnowledgeDocumentSummary {
    pub id: String,
    pub brand_id: String,
    pub product_id: Option<String>,
    pub title: String,
    pub source: String,
    pub format: KnowledgeFormat,
    pub effective_at: Option<String>,
    pub risk: KnowledgeRisk,
    pub review_status: KnowledgeReviewStatus,
    pub reviewed_at: Option<String>,
    pub revision: u64,
    pub created_at: String,
    pub updated_at: String,
    pub chunk_count: usize,
}

impl From<&KnowledgeDocument> for KnowledgeDocumentSummary {
    fn from(document: &KnowledgeDocument) -> Self {
        Self {
            id: document.id.clone(),
            brand_id: document.brand_id.clone(),
            product_id: document.product_id.clone(),
            title: document.title.clone(),
            source: document.source.clone(),
            format: document.format,
            effective_at: document.effective_at.clone(),
            risk: document.risk,
            review_status: document.review_status,
            reviewed_at: document.reviewed_at.clone(),
            revision: document.revision,
            created_at: document.created_at.clone(),
            updated_at: document.updated_at.clone(),
            chunk_count: document.chunks.len(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CreateInput {
    brand_id: String,
    #[serde(default)]
    product_id: Option<String>,
    title: String,
    source: String,
    format: KnowledgeFormat,
    #[serde(default)]
    effective_at: Option<String>,
    risk: KnowledgeRisk,
    content: String,
    request_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReviseInput {
    brand_id: String,
    id: String,
    expected_revision: u64,
    #[serde(default)]
    product_id: Option<String>,
    title: String,
    source: String,
    format: KnowledgeFormat,
    #[serde(default)]
    effective_at: Option<String>,
    risk: KnowledgeRisk,
    content: String,
    request_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReviewInput {
    brand_id: String,
    id: String,
    expected_revision: u64,
    decision: KnowledgeReviewStatus,
    #[serde(default)]
    note: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListInput {
    brand_id: String,
    #[serde(default)]
    document_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RecallInput {
    brand_id: String,
    query: String,
    #[serde(default)]
    product_id: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RecallHit {
    document_id: String,
    document_revision: u64,
    document_title: String,
    source: String,
    product_id: Option<String>,
    risk: KnowledgeRisk,
    effective_at: Option<String>,
    reviewed_at: String,
    chunk_id: String,
    chunk_ordinal: usize,
    text: String,
    sha256: String,
    score: u32,
}

/// Searches approved chunks locally. This is deterministic keyword matching,
/// not semantic retrieval, generation, or an external platform observation.
pub(super) fn recall(c: &Connection, brand_id: &str, payload: &Value) -> Result<Value> {
    const MAX_QUERY_CHARS: usize = 200;
    const MAX_RESULTS: usize = 20;
    let input: RecallInput = serde_json::from_value(payload.clone())
        .map_err(|_| "知识召回字段缺失、未知或格式错误".to_string())?;
    if input.brand_id != brand_id {
        return Err("知识召回品牌与当前品牌不一致".into());
    }
    validate_required_text(&input.query, "query", MAX_QUERY_CHARS, true)?;
    if let Some(product_id) = &input.product_id {
        validate_uuid(product_id, "productId")?;
        super::load(c, "m_products", brand_id, product_id)?;
    }
    let limit = input.limit.unwrap_or(MAX_RESULTS);
    if !(1..=MAX_RESULTS).contains(&limit) {
        return Err("知识召回数量必须为1至20".into());
    }
    let normalized = input.query.trim().to_lowercase();
    let phrase = normalized.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut terms: Vec<String> = Vec::new();
    for term in normalized.split_whitespace().map(str::to_owned) {
        if !terms.contains(&term) {
            terms.push(term);
        }
    }
    if terms.is_empty() {
        return Err("字段 query 不能为空".into());
    }
    let matches = |text: &str, term: &str| {
        if term.is_ascii() {
            text.split_whitespace().any(|candidate| {
                candidate.trim_matches(|character: char| !character.is_ascii_alphanumeric()) == term
            })
        } else {
            text.contains(term)
        }
    };

    let mut statement = c
        .prepare("SELECT data FROM m_knowledge_documents WHERE brand_id=?1 ORDER BY id")
        .map_err(super::db_error)?;
    let rows = statement
        .query_map([brand_id], |row| row.get::<_, String>(0))
        .map_err(super::db_error)?;
    let mut hits = Vec::new();
    for row in rows {
        let document = parse_document(&row.map_err(super::db_error)?)?;
        if document.review_status != KnowledgeReviewStatus::Approved
            || document.reviewed_at.is_none()
            || document.product_id.is_some() && document.product_id != input.product_id
        {
            continue;
        }
        let title = document.title.to_lowercase();
        for chunk in &document.chunks {
            let text = chunk.text.to_lowercase();
            let chunk_terms = terms.iter().filter(|term| matches(&text, term)).count();
            let title_terms = terms.iter().filter(|term| matches(&title, term)).count();
            if !terms
                .iter()
                .all(|term| matches(&text, term) || matches(&title, term))
            {
                continue;
            }
            let score = u32::try_from(chunk_terms * 10 + title_terms * 6).unwrap_or(u32::MAX)
                + if chunk_terms == terms.len() { 20 } else { 0 }
                + if title_terms == terms.len() { 12 } else { 0 }
                + if text.contains(&phrase) { 8 } else { 0 };
            hits.push(RecallHit {
                document_id: document.id.clone(),
                document_revision: document.revision,
                document_title: document.title.clone(),
                source: document.source.clone(),
                product_id: document.product_id.clone(),
                risk: document.risk,
                effective_at: document.effective_at.clone(),
                reviewed_at: document.reviewed_at.clone().ok_or("知识审核时间缺失")?,
                chunk_id: chunk.id.clone(),
                chunk_ordinal: chunk.ordinal,
                text: chunk.text.clone(),
                sha256: chunk.sha256.clone(),
                score,
            });
        }
    }
    hits.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.document_id.cmp(&right.document_id))
            .then_with(|| left.chunk_ordinal.cmp(&right.chunk_ordinal))
            .then_with(|| left.chunk_id.cmp(&right.chunk_id))
    });
    hits.truncate(limit);
    serde_json::to_value(hits).map_err(|_| "知识召回结果序列化失败".into())
}

pub(super) fn list(c: &Connection, brand_id: &str, payload: &Value) -> Result<Value> {
    let input: ListInput = serde_json::from_value(payload.clone())
        .map_err(|_| "知识查询字段缺失、未知或格式错误".to_string())?;
    if input.brand_id != brand_id {
        return Err("知识查询品牌与当前品牌不一致".into());
    }
    if let Some(document_id) = input.document_id {
        validate_uuid(&document_id, "documentId")?;
        return detail(c, brand_id, &document_id);
    }
    let mut statement = c
        .prepare("SELECT data FROM m_knowledge_documents WHERE brand_id=?1 ORDER BY created_at DESC,id DESC")
        .map_err(super::db_error)?;
    let rows = statement
        .query_map([brand_id], |row| row.get::<_, String>(0))
        .map_err(super::db_error)?;
    let mut summaries = Vec::new();
    for row in rows {
        let document = parse_document(&row.map_err(super::db_error)?)?;
        summaries.push(KnowledgeDocumentSummary::from(&document));
    }
    serde_json::to_value(summaries).map_err(|_| "知识文档摘要序列化失败".into())
}

pub(super) fn import_is_replay(c: &Connection, payload: &Value) -> Result<bool> {
    let brand_id = super::guid(payload, "brandId")?;
    let input = parse_create_input(&brand_id, payload)?;
    let request_hash = create_request_hash(&input)?;
    Ok(existing_document(c, &brand_id, &input, &request_hash)?.is_some())
}

fn existing_document(
    c: &Connection,
    brand_id: &str,
    input: &CreateInput,
    request_hash: &str,
) -> Result<Option<String>> {
    let existing = c
        .query_row(
            "SELECT request_hash,id FROM m_knowledge_documents WHERE brand_id=?1 AND request_id=?2",
            params![brand_id, input.request_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(super::db_error)?;
    match existing {
        Some((existing_hash, document_id)) if existing_hash == request_hash => {
            Ok(Some(document_id))
        }
        Some(_) => Err("知识导入请求标识已用于不同载荷".into()),
        None => Ok(None),
    }
}

pub(super) fn import(c: &Connection, brand_id: &str, payload: &Value) -> Result<Value> {
    let input = parse_create_input(brand_id, payload)?;
    let request_hash = create_request_hash(&input)?;
    if let Some(document_id) = existing_document(c, brand_id, &input, &request_hash)? {
        return detail(c, brand_id, &document_id);
    }
    if let Some(product_id) = &input.product_id {
        super::load(c, "m_products", brand_id, product_id)?;
    }
    let count: i64 = c
        .query_row(
            "SELECT count(*) FROM m_knowledge_documents WHERE brand_id=?1",
            [brand_id],
            |row| row.get(0),
        )
        .map_err(super::db_error)?;
    if count >= MAX_DOCUMENTS_PER_BRAND {
        return Err("每个品牌最多保存100篇知识文档".into());
    }
    let document = create_from_input(brand_id, input)?;
    let data = document_json(&document)?;
    c.execute(
        "INSERT INTO m_knowledge_documents(id,brand_id,product_id,request_id,request_hash,created_at,data) VALUES(?1,?2,?3,?4,?5,?6,?7)",
        params![
            document.id,
            brand_id,
            document.product_id,
            document_request_id(payload)?,
            request_hash,
            document.created_at,
            data
        ],
    )
    .map_err(super::db_error)?;
    detail(c, brand_id, &document.id)
}

pub(super) fn revise_is_replay(c: &Connection, brand_id: &str, payload: &Value) -> Result<bool> {
    let input = parse_revise_input(brand_id, payload)?;
    let request_hash = revise_request_hash(&input)?;
    Ok(existing_revision(c, brand_id, &input, &request_hash)?.is_some())
}

fn existing_revision(
    c: &Connection,
    brand_id: &str,
    input: &ReviseInput,
    request_hash: &str,
) -> Result<Option<String>> {
    let existing = c
        .query_row(
            "SELECT request_hash,id FROM m_knowledge_documents WHERE brand_id=?1 AND request_id=?2",
            params![brand_id, input.request_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(super::db_error)?;
    match existing {
        Some((existing_hash, document_id)) if existing_hash == request_hash => {
            Ok(Some(document_id))
        }
        Some(_) => Err("知识修订请求标识已用于不同载荷".into()),
        None => Ok(None),
    }
}

pub(super) fn revise(c: &Connection, brand_id: &str, payload: &Value) -> Result<Value> {
    let input = parse_revise_input(brand_id, payload)?;
    let request_hash = revise_request_hash(&input)?;
    if let Some(document_id) = existing_revision(c, brand_id, &input, &request_hash)? {
        return detail(c, brand_id, &document_id);
    }
    let document = stored_document(c, brand_id, &input.id)?;
    if input.expected_revision != document.revision {
        return Err("知识文档已被其他操作修改，请刷新后重试".into());
    }
    if let Some(product_id) = &input.product_id {
        super::load(c, "m_products", brand_id, product_id)?;
    }
    let request_id = input.request_id.clone();
    let revised = revise_from_input(document, input)?;
    let data = document_json(&revised)?;
    let updated = c
        .execute(
            "UPDATE m_knowledge_documents SET product_id=?1,request_id=?2,request_hash=?3,data=?4 WHERE id=?5 AND brand_id=?6",
            params![
                revised.product_id,
                request_id,
                request_hash,
                data,
                revised.id,
                brand_id
            ],
        )
        .map_err(super::db_error)?;
    if updated != 1 {
        return Err("知识文档不存在或不属于当前品牌".into());
    }
    detail(c, brand_id, &revised.id)
}

pub(super) fn review(c: &Connection, brand_id: &str, payload: &Value) -> Result<Value> {
    let document_id = review_document_id(payload)?;
    let document = stored_document(c, brand_id, &document_id)?;
    let reviewed = review_document(&document, payload)?;
    let history = review_from_document(&reviewed, payload)?;
    let data = document_json(&reviewed)?;
    c.execute(
        "UPDATE m_knowledge_documents SET data=?1 WHERE id=?2 AND brand_id=?3",
        params![data, document_id, brand_id],
    )
    .map_err(super::db_error)?;
    c.execute(
        "INSERT INTO m_knowledge_reviews(id,brand_id,document_id,revision,data) VALUES(?1,?2,?3,?4,?5)",
        params![
            history.id,
            brand_id,
            document_id,
            history.revision,
            serde_json::to_string(&history).map_err(|_| "知识审核记录序列化失败")?
        ],
    )
    .map_err(super::db_error)?;
    detail(c, brand_id, &document_id)
}

fn detail(c: &Connection, brand_id: &str, document_id: &str) -> Result<Value> {
    let mut document = stored_document(c, brand_id, document_id)?;
    let mut statement = c
        .prepare("SELECT data FROM m_knowledge_reviews WHERE brand_id=?1 AND document_id=?2 ORDER BY revision")
        .map_err(super::db_error)?;
    let rows = statement
        .query_map(params![brand_id, document_id], |row| {
            row.get::<_, String>(0)
        })
        .map_err(super::db_error)?;
    for row in rows {
        document
            .reviews
            .push(parse_review(&row.map_err(super::db_error)?)?);
    }
    serde_json::to_value(document).map_err(|_| "知识文档序列化失败".into())
}

fn stored_document(c: &Connection, brand_id: &str, document_id: &str) -> Result<KnowledgeDocument> {
    let data = c
        .query_row(
            "SELECT data FROM m_knowledge_documents WHERE id=?1 AND brand_id=?2",
            params![document_id, brand_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(super::db_error)?
        .ok_or("知识文档不存在或不属于当前品牌")?;
    parse_document(&data)
}

fn document_json(document: &KnowledgeDocument) -> Result<String> {
    if !document.reviews.is_empty() {
        return Err("知识文档存储记录不能包含审核历史".into());
    }
    serde_json::to_string(document).map_err(|_| "知识文档序列化失败".into())
}

fn parse_document(data: &str) -> Result<KnowledgeDocument> {
    serde_json::from_str(data).map_err(|_| "知识文档存储格式损坏".into())
}

fn parse_review(data: &str) -> Result<KnowledgeReview> {
    serde_json::from_str(data).map_err(|_| "知识审核存储格式损坏".into())
}

#[cfg(test)]
fn create_document(brand_id: &str, payload: &Value) -> Result<KnowledgeDocument> {
    create_from_input(brand_id, parse_create_input(brand_id, payload)?)
}

fn create_from_input(brand_id: &str, input: CreateInput) -> Result<KnowledgeDocument> {
    let id = super::id();
    let chunks = chunk_content(&id, &input.content)?;
    let timestamp = super::now();
    Ok(KnowledgeDocument {
        id,
        brand_id: brand_id.to_owned(),
        product_id: input.product_id,
        title: input.title.trim().to_owned(),
        source: input.source.trim().to_owned(),
        format: input.format,
        effective_at: input.effective_at,
        risk: input.risk,
        content: input.content,
        review_status: KnowledgeReviewStatus::Pending,
        reviewed_at: None,
        revision: 1,
        created_at: timestamp.clone(),
        updated_at: timestamp,
        chunks,
        reviews: Vec::new(),
    })
}

fn revise_from_input(document: KnowledgeDocument, input: ReviseInput) -> Result<KnowledgeDocument> {
    let chunks = chunk_content(&document.id, &input.content)?;
    let revision = document
        .revision
        .checked_add(1)
        .ok_or("知识文档版本已超限")?;
    Ok(KnowledgeDocument {
        id: document.id,
        brand_id: document.brand_id,
        product_id: input.product_id,
        title: input.title.trim().to_owned(),
        source: input.source.trim().to_owned(),
        format: input.format,
        effective_at: input.effective_at,
        risk: input.risk,
        content: input.content,
        review_status: KnowledgeReviewStatus::Pending,
        reviewed_at: None,
        revision,
        created_at: document.created_at,
        updated_at: super::now(),
        chunks,
        reviews: Vec::new(),
    })
}

fn parse_create_input(brand_id: &str, payload: &Value) -> Result<CreateInput> {
    if brand_id.contains('\0') {
        return Err("字段 brandId 含非法字符".into());
    }
    let input: CreateInput = serde_json::from_value(payload.clone())
        .map_err(|_| "知识文档字段缺失、未知或格式错误".to_string())?;
    if input.brand_id != brand_id {
        return Err("知识文档品牌与当前品牌不一致".into());
    }
    validate_required_text(&input.title, "title", MAX_TITLE_CHARS, true)?;
    validate_required_text(&input.source, "source", MAX_SOURCE_CHARS, true)?;
    validate_uuid(&input.request_id, "requestId")?;
    if let Some(product_id) = &input.product_id {
        validate_uuid(product_id, "productId")?;
    }
    if let Some(date) = &input.effective_at {
        validate_date(date)?;
    }
    validate_content(&input.content)?;
    Ok(input)
}

fn parse_revise_input(brand_id: &str, payload: &Value) -> Result<ReviseInput> {
    if brand_id.contains('\0') {
        return Err("字段 brandId 含非法字符".into());
    }
    let input: ReviseInput = serde_json::from_value(payload.clone())
        .map_err(|_| "知识修订字段缺失、未知或格式错误".to_string())?;
    if input.brand_id != brand_id {
        return Err("知识修订品牌与当前品牌不一致".into());
    }
    validate_uuid(&input.id, "id")?;
    validate_required_text(&input.title, "title", MAX_TITLE_CHARS, true)?;
    validate_required_text(&input.source, "source", MAX_SOURCE_CHARS, true)?;
    validate_uuid(&input.request_id, "requestId")?;
    if let Some(product_id) = &input.product_id {
        validate_uuid(product_id, "productId")?;
    }
    if let Some(date) = &input.effective_at {
        validate_date(date)?;
    }
    validate_content(&input.content)?;
    Ok(input)
}

fn create_request_hash(input: &CreateInput) -> Result<String> {
    let encoded = serde_json::to_vec(input).map_err(|_| "知识导入请求序列化失败")?;
    Ok(hex::encode(Sha256::digest(encoded)))
}

fn revise_request_hash(input: &ReviseInput) -> Result<String> {
    let encoded = serde_json::to_vec(input).map_err(|_| "知识修订请求序列化失败")?;
    Ok(hex::encode(Sha256::digest(encoded)))
}

fn document_request_id(payload: &Value) -> Result<String> {
    payload
        .get("requestId")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| "知识导入请求标识缺失".into())
}

/// Deterministically chunks original text without a model, network, or storage.
pub(super) fn chunk_content(document_id: &str, content: &str) -> Result<Vec<KnowledgeChunk>> {
    validate_uuid(document_id, "documentId")?;
    validate_content(content)?;
    let normalized = content.replace("\r\n", "\n").replace('\r', "\n");
    let mut blocks = Vec::new();
    for paragraph in normalized.split("\n\n") {
        let paragraph = paragraph.trim();
        if paragraph.is_empty() {
            continue;
        }
        if paragraph.chars().count() <= MAX_CHUNK_CHARS {
            blocks.push(paragraph.to_owned());
        } else {
            blocks.extend(split_oversized(paragraph));
        }
    }

    let mut texts: Vec<String> = Vec::new();
    let mut buffer = String::new();
    for block in blocks {
        let separator_chars = usize::from(!buffer.is_empty()) * 2;
        let candidate_chars = buffer.chars().count() + separator_chars + block.chars().count();
        if !buffer.is_empty() && candidate_chars > MAX_CHUNK_CHARS {
            texts.push(std::mem::take(&mut buffer));
        }
        if !buffer.is_empty() {
            buffer.push_str("\n\n");
        }
        buffer.push_str(&block);
    }
    if !buffer.trim().is_empty() {
        texts.push(buffer);
    }
    if texts.is_empty() || texts.len() > MAX_CHUNKS {
        return Err("知识文档切片为空或超过200片".into());
    }

    Ok(texts
        .into_iter()
        .enumerate()
        .map(|(ordinal, text)| {
            let sha256 = hex::encode(Sha256::digest(text.as_bytes()));
            let id = hex::encode(Sha256::digest(
                format!("m-geo-knowledge-chunk-v1\0{document_id}\0{ordinal}\0{sha256}").as_bytes(),
            ));
            KnowledgeChunk {
                id,
                ordinal,
                text,
                sha256,
            }
        })
        .collect())
}

fn parse_review_input(payload: &Value) -> Result<ReviewInput> {
    let input: ReviewInput = serde_json::from_value(payload.clone())
        .map_err(|_| "知识审核字段缺失、未知或格式错误".to_string())?;
    validate_uuid(&input.id, "id")?;
    if input.decision == KnowledgeReviewStatus::Pending {
        return Err("知识审核只能选择 approved 或 rejected".into());
    }
    let note = input.note.as_deref().unwrap_or("");
    validate_required_text(note, "note", MAX_NOTE_CHARS, false).or_else(|error| {
        if note.is_empty() {
            Ok(())
        } else {
            Err(error)
        }
    })?;
    if input.decision == KnowledgeReviewStatus::Rejected && note.trim().is_empty() {
        return Err("驳回知识文档必须填写审核理由".into());
    }
    Ok(input)
}

fn review_document_id(payload: &Value) -> Result<String> {
    Ok(parse_review_input(payload)?.id)
}

/// Applies an explicit human decision; stale reviews fail without mutation.
pub(super) fn review_document(
    document: &KnowledgeDocument,
    payload: &Value,
) -> Result<KnowledgeDocument> {
    let input = parse_review_input(payload)?;
    if input.id != document.id || input.brand_id != document.brand_id {
        return Err("知识审核对象或品牌不一致".into());
    }
    if input.expected_revision != document.revision {
        return Err("知识文档已被其他操作修改，请刷新后重试".into());
    }
    let mut reviewed = document.clone();
    reviewed.review_status = input.decision;
    let timestamp = super::now();
    reviewed.reviewed_at = Some(timestamp.clone());
    reviewed.updated_at = timestamp;
    reviewed.revision = reviewed
        .revision
        .checked_add(1)
        .ok_or("知识文档版本已超限")?;
    Ok(reviewed)
}

fn review_from_document(document: &KnowledgeDocument, payload: &Value) -> Result<KnowledgeReview> {
    let input = parse_review_input(payload)?;
    if input.id != document.id || document.reviewed_at.is_none() {
        return Err("知识审核文档状态无效".into());
    }
    Ok(KnowledgeReview {
        id: super::id(),
        document_id: document.id.clone(),
        revision: document.revision,
        decision: input.decision,
        note: input.note.unwrap_or_default(),
        created_at: document.reviewed_at.clone().ok_or("知识审核时间缺失")?,
    })
}

#[cfg(test)]
pub(super) fn is_approved(document: &KnowledgeDocument) -> bool {
    document.review_status == KnowledgeReviewStatus::Approved && document.reviewed_at.is_some()
}

/// Read-only semantic validation used before backup replacement.
pub(super) fn validate_database(c: &Connection) -> Result<()> {
    let over_limit: Option<String> = c
        .query_row(
            "SELECT brand_id FROM m_knowledge_documents GROUP BY brand_id HAVING count(*)>100 LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(super::db_error)?;
    if over_limit.is_some() {
        return Err("品牌知识文档数量超限".into());
    }
    let mut documents = HashMap::new();
    let mut statement = c
        .prepare("SELECT id,brand_id,product_id,request_id,request_hash,created_at,data FROM m_knowledge_documents")
        .map_err(super::db_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
            ))
        })
        .map_err(super::db_error)?;
    for row in rows {
        let (id, brand_id, product_id, request_id, request_hash, created_at, data) =
            row.map_err(super::db_error)?;
        let document = parse_document(&data)?;
        if document.id != id
            || document.brand_id != brand_id
            || document.product_id != product_id
            || document.created_at != created_at
            || !document.reviews.is_empty()
        {
            return Err("知识文档身份或索引不一致".into());
        }
        validate_uuid(&id, "id")?;
        validate_uuid(&brand_id, "brandId")?;
        validate_uuid(&request_id, "requestId")?;
        if !is_lower_sha256(&request_hash) {
            return Err("知识导入请求哈希无效".into());
        }
        if let Some(product_id) = &document.product_id {
            validate_uuid(product_id, "productId")?;
            super::load(c, "m_products", &brand_id, product_id)?;
        }
        validate_stored_document(&document)?;
        documents.insert((brand_id, id), document);
    }

    let mut histories: HashMap<(String, String), Vec<KnowledgeReview>> = HashMap::new();
    let mut statement = c
        .prepare("SELECT id,brand_id,document_id,revision,data FROM m_knowledge_reviews ORDER BY document_id,revision")
        .map_err(super::db_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
            ))
        })
        .map_err(super::db_error)?;
    for row in rows {
        let (id, brand_id, document_id, revision, data) = row.map_err(super::db_error)?;
        let review = parse_review(&data)?;
        let key = (brand_id, document_id);
        let document = documents.get(&key).ok_or("知识审核记录与文档归属不一致")?;
        if review.id != id
            || review.document_id != key.1
            || review.revision != u64::try_from(revision).map_err(|_| "知识审核版本无效")?
            || review.revision < 2
            || review.revision > document.revision
        {
            return Err("知识审核身份或版本关联不一致".into());
        }
        validate_uuid(&review.id, "id")?;
        validate_timestamp(&review.created_at)?;
        validate_review_decision(review.decision, &review.note)?;
        histories.entry(key).or_default().push(review);
    }

    for (key, document) in documents {
        let history = histories.remove(&key).unwrap_or_default();
        match document.review_status {
            KnowledgeReviewStatus::Pending => {
                if document.reviewed_at.is_some()
                    || history
                        .last()
                        .is_some_and(|latest| latest.revision >= document.revision)
                {
                    return Err("待审核知识文档状态不一致".into());
                }
            }
            KnowledgeReviewStatus::Approved | KnowledgeReviewStatus::Rejected => {
                let latest = history.last().ok_or("已审核知识文档缺少历史")?;
                if latest.revision != document.revision
                    || latest.decision != document.review_status
                    || document.reviewed_at.as_deref() != Some(latest.created_at.as_str())
                    || document.updated_at != latest.created_at
                {
                    return Err("知识文档当前审核状态与历史不一致".into());
                }
            }
        }
    }
    if !histories.is_empty() {
        return Err("知识审核记录存在孤立归属".into());
    }
    Ok(())
}

fn validate_stored_document(document: &KnowledgeDocument) -> Result<()> {
    if document.title.trim() != document.title
        || document.source.trim() != document.source
        || document.revision == 0
    {
        return Err("知识文档存储文本或版本不规范".into());
    }
    validate_required_text(&document.title, "title", MAX_TITLE_CHARS, true)?;
    validate_required_text(&document.source, "source", MAX_SOURCE_CHARS, true)?;
    if let Some(date) = &document.effective_at {
        validate_date(date)?;
    }
    validate_timestamp(&document.created_at)?;
    validate_timestamp(&document.updated_at)?;
    if document.updated_at < document.created_at {
        return Err("知识文档时间顺序无效".into());
    }
    if chunk_content(&document.id, &document.content)? != document.chunks {
        return Err("知识文档切片或哈希不一致".into());
    }
    Ok(())
}

fn validate_review_decision(decision: KnowledgeReviewStatus, note: &str) -> Result<()> {
    if decision == KnowledgeReviewStatus::Pending
        || note.contains('\0')
        || note.chars().count() > MAX_NOTE_CHARS
        || decision == KnowledgeReviewStatus::Rejected && note.trim().is_empty()
    {
        return Err("知识审核决定或理由无效".into());
    }
    Ok(())
}

fn validate_timestamp(value: &str) -> Result<()> {
    let parsed = DateTime::parse_from_rfc3339(value).map_err(|_| "知识文档时间格式无效")?;
    if parsed
        .with_timezone(&Utc)
        .to_rfc3339_opts(SecondsFormat::Secs, true)
        != value
    {
        return Err("知识文档时间格式不规范".into());
    }
    Ok(())
}

fn is_lower_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn validate_required_text(value: &str, field: &str, max: usize, trim: bool) -> Result<()> {
    if value.contains('\0') || value.chars().count() > max {
        return Err(format!("字段 {field} 为空、过长或含非法字符"));
    }
    let value = if trim { value.trim() } else { value };
    if value.is_empty() {
        return Err(format!("字段 {field} 不能为空"));
    }
    Ok(())
}

fn validate_content(content: &str) -> Result<()> {
    if content.trim().is_empty() {
        return Err("字段 content 不能为空".into());
    }
    validate_required_text(content, "content", MAX_CONTENT_CHARS, false)
}

fn validate_uuid(value: &str, field: &str) -> Result<()> {
    let parsed = Uuid::parse_str(value).map_err(|_| format!("字段 {field} 必须为合法 UUID"))?;
    if value.len() != 36 || !parsed.to_string().eq_ignore_ascii_case(value) {
        return Err(format!("字段 {field} 必须为合法 UUID"));
    }
    Ok(())
}

fn validate_date(value: &str) -> Result<()> {
    if value.len() != 10 || NaiveDate::parse_from_str(value, "%Y-%m-%d").is_err() {
        return Err("字段 effectiveAt 必须为合法 YYYY-MM-DD 日期".into());
    }
    Ok(())
}

fn split_oversized(text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::with_capacity(MAX_CHUNK_CHARS);
    let mut count = 0;
    for character in text.chars() {
        if count == MAX_CHUNK_CHARS {
            parts.push(std::mem::take(&mut current));
            count = 0;
        }
        current.push(character);
        count += 1;
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const BRAND: &str = "10000000-0000-4000-8000-000000000001";
    const PRODUCT: &str = "20000000-0000-4000-8000-000000000002";
    const REQUEST: &str = "30000000-0000-4000-8000-000000000003";
    const DOCUMENT: &str = "40000000-0000-4000-8000-000000000004";

    fn input(content: &str) -> Value {
        json!({
            "brandId": BRAND,
            "productId": PRODUCT,
            "title": " 虚构保温杯保养手册 ",
            "source": " 虚构实验室内部说明 ",
            "format": "markdown",
            "effectiveAt": "2028-02-29",
            "risk": "medium",
            "content": content,
            "requestId": REQUEST
        })
    }

    fn document(content: &str) -> KnowledgeDocument {
        create_document(BRAND, &input(content)).unwrap()
    }

    fn review_payload(document: &KnowledgeDocument, decision: &str, note: Value) -> Value {
        json!({"brandId":BRAND,"id":document.id,"expectedRevision":document.revision,"decision":decision,"note":note})
    }

    #[test]
    fn creates_serializable_pending_document_with_chinese_content() {
        let doc = document("第一段：容量为虚构的 500 毫升。\n\n第二段：仅用于隔离测试。");
        assert_eq!(doc.brand_id, BRAND);
        assert_eq!(doc.product_id.as_deref(), Some(PRODUCT));
        assert_eq!(doc.title, "虚构保温杯保养手册");
        assert_eq!(doc.source, "虚构实验室内部说明");
        assert_eq!(doc.review_status, KnowledgeReviewStatus::Pending);
        assert_eq!(doc.reviewed_at, None);
        assert_eq!(doc.revision, 1);
        assert_eq!(doc.created_at, doc.updated_at);
        assert_eq!(doc.chunks.len(), 1);
        assert!(doc.reviews.is_empty());
        assert!(!is_approved(&doc));
        let encoded = serde_json::to_value(&doc).unwrap();
        assert_eq!(encoded["format"], "markdown");
        assert_eq!(encoded["risk"], "medium");
        assert_eq!(encoded["reviewStatus"], "pending");
        assert_eq!(encoded["reviewedAt"], Value::Null);
        assert_eq!(encoded["productId"], PRODUCT);
        assert_eq!(encoded["reviews"], json!([]));
        let decoded: KnowledgeDocument = serde_json::from_value(encoded).unwrap();
        assert_eq!(decoded, doc);
    }

    #[test]
    fn nullable_fields_accept_null_and_omission() {
        let mut value = input("虚构正文");
        value["productId"] = Value::Null;
        value["effectiveAt"] = Value::Null;
        let doc = create_document(BRAND, &value).unwrap();
        assert_eq!(doc.product_id, None);
        assert_eq!(doc.effective_at, None);
        let mut value = input("虚构正文");
        value.as_object_mut().unwrap().remove("productId");
        value.as_object_mut().unwrap().remove("effectiveAt");
        let doc = create_document(BRAND, &value).unwrap();
        assert_eq!(doc.product_id, None);
        assert_eq!(doc.effective_at, None);
    }

    #[test]
    fn rejects_unknown_fields_enums_dates_uuids_and_nul() {
        let invalid = [
            ("format", json!("html")),
            ("risk", json!("critical")),
            ("effectiveAt", json!("2027-02-29")),
            ("effectiveAt", json!("2028-2-29")),
            ("effectiveAt", json!("2028-02-29T00:00:00Z")),
            ("requestId", json!("not-a-uuid")),
            ("requestId", json!("30000000000040008000000000000003")),
            ("productId", json!("bad")),
            ("unexpected", json!(true)),
            ("title", json!("bad\0title")),
            ("source", json!("bad\0source")),
            ("content", json!("bad\0content")),
        ];
        for (field, replacement) in invalid {
            let mut value = input("有效虚构正文");
            value[field] = replacement;
            assert!(create_document(BRAND, &value).is_err(), "accepted {field}");
        }
    }

    #[test]
    fn rejects_missing_required_wrong_types_empty_and_brand_mismatch() {
        for field in [
            "brandId",
            "title",
            "source",
            "format",
            "risk",
            "content",
            "requestId",
        ] {
            let mut value = input("有效虚构正文");
            value.as_object_mut().unwrap().remove(field);
            assert!(
                create_document(BRAND, &value).is_err(),
                "accepted missing {field}"
            );
        }
        for field in ["title", "source", "content"] {
            let mut value = input("有效虚构正文");
            value[field] = json!(" \n\t ");
            assert!(create_document(BRAND, &value).is_err());
            value[field] = json!(42);
            assert!(create_document(BRAND, &value).is_err());
        }
        assert!(create_document(PRODUCT, &input("有效虚构正文")).is_err());
        assert!(create_document("bad\0brand", &input("有效虚构正文")).is_err());
    }

    #[test]
    fn chunks_are_deterministic_ordered_hashed_and_unicode_safe() {
        let content = "甲段🙂保留原文。\n\n乙段包含中文与 emoji 🚀。\n\n第三段结束。";
        let first = chunk_content(DOCUMENT, content).unwrap();
        let second = chunk_content(DOCUMENT, content).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].ordinal, 0);
        assert_eq!(first[0].text, content);
        assert_eq!(
            first[0].sha256,
            hex::encode(Sha256::digest(content.as_bytes()))
        );
        assert_eq!(first[0].id.len(), 64);
        assert_ne!(first[0].id, chunk_content(REQUEST, content).unwrap()[0].id);
    }

    #[test]
    fn paragraph_priority_and_oversized_unicode_are_bounded() {
        let short_a = "甲".repeat(700);
        let short_b = "乙".repeat(700);
        let chunks = chunk_content(DOCUMENT, &format!("{short_a}\n\n{short_b}")).unwrap();
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].text, short_a);
        assert_eq!(chunks[1].text, short_b);
        let long = "中🙂".repeat(1_001);
        let chunks = chunk_content(DOCUMENT, &long).unwrap();
        assert_eq!(
            chunks.iter().map(|c| c.text.chars().count()).sum::<usize>(),
            2_002
        );
        assert!(chunks
            .iter()
            .all(|c| !c.text.is_empty() && c.text.chars().count() <= MAX_CHUNK_CHARS));
        assert_eq!(
            chunks.iter().map(|c| c.text.as_str()).collect::<String>(),
            long
        );
        assert!(chunks.windows(2).all(|w| w[0].ordinal + 1 == w[1].ordinal));
    }

    #[test]
    fn exact_boundary_and_crlf_blank_lines_never_create_empty_chunks() {
        let exact = "界".repeat(MAX_CHUNK_CHARS);
        assert_eq!(chunk_content(DOCUMENT, &exact).unwrap().len(), 1);
        let over = "界".repeat(MAX_CHUNK_CHARS + 1);
        let chunks = chunk_content(DOCUMENT, &over).unwrap();
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[1].text.chars().count(), 1);
        let chunks = chunk_content(DOCUMENT, "\r\n第一段\r\n\r\n\r\n\r\n第二段\r\n").unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].text, "第一段\n\n第二段");
        assert!(chunks.iter().all(|chunk| !chunk.text.trim().is_empty()));
    }

    #[test]
    fn content_and_chunk_count_limits_are_enforced() {
        let maximum = "字".repeat(MAX_CONTENT_CHARS);
        let chunks = chunk_content(DOCUMENT, &maximum).unwrap();
        assert!(chunks.len() <= MAX_CHUNKS);
        assert_eq!(chunks.len(), 167);
        assert!(chunk_content(DOCUMENT, &"字".repeat(MAX_CONTENT_CHARS + 1)).is_err());
        let paragraphs = vec!["段".repeat(900); MAX_CHUNKS];
        let chunks = chunk_content(DOCUMENT, &paragraphs.join("\n\n")).unwrap();
        assert_eq!(chunks.len(), MAX_CHUNKS);
    }

    #[test]
    fn explicit_approval_and_rejection_advance_revision() {
        let original = document("仅供虚构审核测试");
        let approved = review_document(
            &original,
            &review_payload(&original, "approved", json!(null)),
        )
        .unwrap();
        assert_eq!(approved.review_status, KnowledgeReviewStatus::Approved);
        assert!(approved.reviewed_at.is_some());
        assert_eq!(approved.revision, 2);
        assert!(is_approved(&approved));
        assert_eq!(original.revision, 1);
        assert_eq!(original.review_status, KnowledgeReviewStatus::Pending);
        let history = review_from_document(
            &approved,
            &review_payload(&original, "approved", json!(null)),
        )
        .unwrap();
        assert_eq!(history.revision, 2);
        assert_eq!(history.document_id, original.id);
        assert_eq!(history.note, "");
        let rejected = review_document(
            &approved,
            &review_payload(&approved, "rejected", json!("虚构依据不足")),
        )
        .unwrap();
        assert_eq!(rejected.review_status, KnowledgeReviewStatus::Rejected);
        assert_eq!(rejected.revision, 3);
        assert!(rejected.reviewed_at.is_some());
        assert!(!is_approved(&rejected));
    }

    #[test]
    fn stale_reviews_conflict_without_mutating_document() {
        let doc = document("重复审核冲突夹具");
        let reviewed =
            review_document(&doc, &review_payload(&doc, "approved", json!("ok"))).unwrap();
        let before = reviewed.clone();
        for revision in [0, 1, 3] {
            let mut payload = review_payload(&reviewed, "rejected", json!("重新驳回"));
            payload["expectedRevision"] = json!(revision);
            assert!(review_document(&reviewed, &payload).is_err());
            assert_eq!(reviewed, before);
        }
    }

    #[test]
    fn review_rejects_pending_unknown_status_bad_notes_and_invalid_revisions() {
        let doc = document("审核输入夹具");
        for payload in [
            review_payload(&doc, "pending", json!("bad")),
            review_payload(&doc, "approved ", json!("bad")),
            review_payload(&doc, "unknown", json!("bad")),
            review_payload(&doc, "rejected", json!(" ")),
            review_payload(&doc, "rejected", json!(null)),
            review_payload(&doc, "approved", json!("bad\0note")),
            review_payload(&doc, "approved", json!("n".repeat(MAX_NOTE_CHARS + 1))),
            json!({"brandId":BRAND,"id":doc.id,"expectedRevision":"1","decision":"approved","note":""}),
            json!({"brandId":BRAND,"id":doc.id,"expectedRevision":1,"decision":"approved","note":"","extra":true}),
        ] {
            assert!(
                review_document(&doc, &payload).is_err(),
                "accepted {payload}"
            );
            assert_eq!(doc.revision, 1);
            assert_eq!(doc.reviewed_at, None);
        }
    }

    #[test]
    fn revision_overflow_cannot_wrap() {
        let mut doc = document("版本边界夹具");
        doc.revision = u64::MAX;
        assert!(review_document(&doc, &review_payload(&doc, "approved", json!(""))).is_err());
        assert_eq!(doc.revision, u64::MAX);
    }
}
