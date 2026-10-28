//! Read-only full-workspace validation for a candidate restore; never applies current eligibility rules.
use super::*;
use imports::{historical_snapshot, keys};

pub(super) fn validate(c: &Connection) -> Result<()> {
    let version: (i64, i64) = c
        .query_row(
            "SELECT count(*),coalesce(min(schema_version),0) FROM m_core_meta WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(db_error)?;
    let total: i64 = c
        .query_row("SELECT count(*) FROM m_core_meta", [], |r| r.get(0))
        .map_err(db_error)?;
    if ![
        (1, 1),
        (1, 2),
        (1, 3),
        (1, 4),
        (1, 5),
        (1, 6),
        (1, 7),
        (1, 8),
        (1, 9),
        (1, 10),
    ]
    .contains(&version)
        || total != 1
    {
        return Err("资料版本不正确".into());
    }
    if c.prepare("PRAGMA foreign_key_check")
        .map_err(db_error)?
        .query([])
        .map_err(db_error)?
        .next()
        .map_err(db_error)?
        .is_some()
    {
        return Err("资料关联不完整".into());
    }
    validate_brands(c)?;
    for table in [
        "m_products",
        "m_questions",
        "m_drafts",
        "m_publications",
        "m_observations",
    ] {
        validate_records(c, table)?;
    }
    if version.1 >= 2 {
        experiments::validate(c)?;
    }
    if version.1 >= 3 {
        attribution::validate_database(c)?;
    }
    if version.1 >= 4 {
        local_jobs::validate_database(c)?;
    }
    if version.1 >= 5 {
        optimization::validate_database(c)?;
    }
    if version.1 >= 6 {
        knowledge::validate_database(c)?;
    }
    if version.1 >= 7 {
        validate_assets(c)?;
    }
    if version.1 >= 8 {
        validate_real_observation_identities(c)?;
        validate_research(c)?;
        validate_media(c)?;
        automation::validate_database(c)?;
    }
    if version.1 >= 9 {
        local_workflow::validate_database(c)?;
    }
    if version.1 >= 10 {
        p0_workflow::validate_database(c)?;
    }
    validate_preparations(c)?;
    validate_migration(c)?;
    validate_audit(c)?;
    Ok(())
}
fn choice(v: &Value, key: &str, allowed: &[&str]) -> Result<String> {
    let selected = super::choice(v, key, allowed)?;
    if v[key].as_str() != Some(selected.as_str()) {
        return Err("存储枚举值不规范".into());
    }
    Ok(selected)
}
fn guid(v: &Value, key: &str) -> Result<String> {
    let selected = super::guid(v, key)?;
    if v[key].as_str() != Some(selected.as_str()) {
        return Err("存储对象标识不规范".into());
    }
    Ok(selected)
}
fn stored(s: String) -> Result<Value> {
    stored_limit(s, 2_100_000)
}
fn stored_limit(s: String, limit: usize) -> Result<Value> {
    if s.len() > limit {
        return Err("单条资料超限".into());
    }
    let v = parse_data(s)?;
    if !v.is_object() {
        return Err("资料必须为对象".into());
    }
    Ok(v)
}
fn string(v: &Value, key: &str, max: usize, nonempty: bool) -> Result<String> {
    if !v.get(key).is_some_and(Value::is_string) {
        return Err(format!("缺少文字字段 {}", key));
    }
    raw(v, key, max, nonempty)
}
fn exact_guid(v: &Value, key: &str, sql: &str) -> Result<()> {
    guid(v, key)?;
    if v[key].as_str() != Some(sql) {
        return Err("资料身份与索引不一致".into());
    }
    Ok(())
}
fn nullable_date(v: &Value, key: &str, required: bool) -> Result<()> {
    match v.get(key) {
        Some(Value::Null) => Ok(()),
        Some(_) => date_time(v, key).map(|_| ()),
        None if !required => Ok(()),
        _ => Err("日期字段缺失".into()),
    }
}
fn kind(v: &Value) -> Result<()> {
    choice(v, "kind", &["description", "faq", "guide", "comparison"])?;
    Ok(())
}
fn record_identity(c: &Connection, v: &Value, sql_id: &str, b: &str) -> Result<()> {
    exact_guid(v, "id", sql_id)?;
    exact_guid(v, "brandId", b)?;
    brand(c, b)?;
    Ok(())
}
fn owned_product(c: &Connection, v: &Value, b: &str) -> Result<Value> {
    let pid = guid(v, "productId")?;
    load(c, "m_products", b, &pid)
}
fn optional_question(c: &Connection, v: &Value, b: &str, pid: &str) -> Result<()> {
    match v.get("questionId") {
        Some(Value::Null) => Ok(()),
        Some(_) => {
            let qid = guid(v, "questionId")?;
            let q = load(c, "m_questions", b, &qid)?;
            if q["productId"] != pid {
                return Err("关联问题不属于商品".into());
            }
            Ok(())
        }
        None => Err("关联问题字段缺失".into()),
    }
}
fn valid_facts(v: &Value, public_only: bool) -> Result<()> {
    facts(v)?;
    if public_only && v["facts"].as_array().is_some_and(Vec::is_empty) {
        return Err("生成快照缺少公开事实".into());
    }
    for f in v["facts"].as_array().ok_or("事实格式不正确")? {
        let legacy = f.get("reviewStatus").is_none();
        let fact_keys: &[&str] = if legacy {
            &["id", "label", "value", "source", "visibility", "validUntil"]
        } else {
            &[
                "id",
                "label",
                "value",
                "source",
                "visibility",
                "validUntil",
                "reviewStatus",
                "reviewedAt",
                "updatedAt",
            ]
        };
        keys(f, fact_keys)?;
        string(f, "id", 100, true)?;
        string(f, "label", 200, true)?;
        string(f, "value", 3000, true)?;
        string(f, "source", 2000, public_only)?;
        choice(f, "visibility", &["public", "private"])?;
        if !legacy {
            choice(f, "reviewStatus", &["pending", "approved", "rejected"])?;
            nullable_date(f, "reviewedAt", true)?;
            date_time(f, "updatedAt")?;
            if f["reviewStatus"] == "approved" && f["reviewedAt"].is_null() {
                return Err("已审核事实缺少审核时间".into());
            }
        }
        if public_only && f["visibility"] != "public" {
            return Err("生成快照不能包含私密事实".into());
        }
    }
    Ok(())
}
fn validate_brand_profile(profile: &Value) -> Result<()> {
    keys(
        profile,
        &[
            "aliases",
            "officialUrl",
            "manufacturer",
            "category",
            "description",
            "updatedAt",
        ],
    )?;
    let aliases = profile["aliases"].as_array().ok_or("品牌别名格式错误")?;
    if aliases.len() > 20 {
        return Err("品牌别名超过上限".into());
    }
    let mut seen = HashSet::new();
    for alias in aliases {
        let wrapped = json!({"alias":alias});
        let normalized = string(&wrapped, "alias", 100, true)?;
        if alias.as_str() != Some(normalized.trim())
            || normalized.trim().is_empty()
            || !seen.insert(normalized.to_lowercase())
        {
            return Err("品牌别名不规范或重复".into());
        }
    }
    let official_url = string(profile, "officialUrl", 2000, false)?;
    web_url(&official_url, false)?;
    for (key, max) in [
        ("manufacturer", 200),
        ("category", 200),
        ("description", 5000),
    ] {
        string(profile, key, max, false)?;
    }
    date_time(profile, "updatedAt")?;
    Ok(())
}
fn validate_brands(c: &Connection) -> Result<()> {
    let mut st = c
        .prepare("SELECT id,name,data FROM m_brands")
        .map_err(db_error)?;
    let rows = st
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(db_error)?;
    for row in rows {
        let (id, name, data) = row.map_err(db_error)?;
        let v = stored(data)?;
        let mut brand_keys = vec!["id", "name", "createdAt"];
        if v.get("profile").is_some() {
            brand_keys.push("profile");
        }
        keys(&v, &brand_keys)?;
        exact_guid(&v, "id", &id)?;
        let vn = string(&v, "name", 100, true)?;
        if vn != name {
            return Err("品牌名称与索引不一致".into());
        }
        date_time(&v, "createdAt")?;
        if let Some(profile) = v.get("profile") {
            validate_brand_profile(profile)?;
        }
    }
    Ok(())
}
fn validate_records(c: &Connection, table: &str) -> Result<()> {
    let extras = match table {
        "m_products" => "sku,NULL,NULL",
        "m_questions" => "product_id,normalized,NULL",
        "m_drafts" => "product_id,question_id,NULL",
        "m_publications" => "draft_id,dedup,NULL",
        "m_observations" => "product_id,question_id,dedup",
        _ => return Err("未知资料类型".into()),
    };
    let mut st = c
        .prepare(&format!("SELECT id,brand_id,data,{extras} FROM {table}"))
        .map_err(db_error)?;
    let rows = st
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, Option<String>>(5)?,
            ))
        })
        .map_err(db_error)?;
    for row in rows {
        let (id, b, data, x, y, z) = row.map_err(db_error)?;
        let v = stored(data)?;
        record_identity(c, &v, &id, &b)?;
        match table {
            "m_products" => {
                keys(
                    &v,
                    &[
                        "id",
                        "brandId",
                        "name",
                        "sku",
                        "category",
                        "officialUrl",
                        "sourceText",
                        "privateNotes",
                        "facts",
                        "revision",
                        "confirmedAt",
                        "updatedAt",
                    ],
                )?;
                string(&v, "name", 200, true)?;
                let sku = string(&v, "sku", 100, true)?;
                if Some(&sku) != x.as_ref() {
                    return Err("商品 SKU 与索引不一致".into());
                }
                string(&v, "category", 200, false)?;
                web_url(&string(&v, "officialUrl", 2000, false)?, false)?;
                string(&v, "sourceText", 100000, false)?;
                string(&v, "privateNotes", 30000, false)?;
                valid_facts(&v, false)?;
                number(&v, "revision")?;
                nullable_date(&v, "confirmedAt", true)?;
                date_time(&v, "updatedAt")?;
            }
            "m_questions" => {
                keys(
                    &v,
                    &[
                        "id",
                        "brandId",
                        "productId",
                        "text",
                        "intent",
                        "origin",
                        "source",
                        "branded",
                        "confirmed",
                    ],
                )?;
                exact_guid(&v, "productId", x.as_deref().ok_or("问题商品索引缺失")?)?;
                owned_product(c, &v, &b)?;
                let txt = string(&v, "text", 1000, true)?;
                if y.as_deref() != Some(normalize(&txt).as_str()) {
                    return Err("问题规范化索引不一致".into());
                }
                choice(
                    &v,
                    "intent",
                    &["selection", "comparison", "usage", "after_sales"],
                )?;
                choice(&v, "origin", &["customer", "suggestion"])?;
                string(&v, "source", 2000, true)?;
                boolean(&v, "branded")?;
                boolean(&v, "confirmed")?;
            }
            "m_drafts" => {
                keys(
                    &v,
                    &[
                        "id",
                        "brandId",
                        "productId",
                        "questionId",
                        "kind",
                        "channel",
                        "channelTemplateVersion",
                        "title",
                        "body",
                        "claims",
                        "assetIds",
                        "assetSnapshots",
                        "sourceRevision",
                        "revision",
                        "generationMode",
                        "reviewedAt",
                        "createdAt",
                    ],
                )?;
                exact_guid(&v, "productId", x.as_deref().ok_or("稿件商品索引缺失")?)?;
                owned_product(c, &v, &b)?;
                if v.get("questionId").and_then(Value::as_str) != y.as_deref() {
                    return Err("稿件问题索引不一致".into());
                }
                optional_question(c, &v, &b, x.as_deref().ok_or("商品标识缺失")?)?;
                kind(&v)?;
                channels::parse(&v, "channel")?;
                if v.get("channel").is_some_and(|value| !value.is_null()) {
                    if v.get("channelTemplateVersion") != Some(&json!(channels::TEMPLATE_VERSION)) {
                        return Err("渠道模板版本不受支持".into());
                    }
                } else if v
                    .get("channelTemplateVersion")
                    .is_some_and(|value| !value.is_null() && *value != json!(0))
                {
                    return Err("通用稿件的渠道模板版本不正确".into());
                }
                string(&v, "title", 300, true)?;
                string(&v, "body", 30000, true)?;
                if v.get("claims").is_some() {
                    claims::validate_stored(&v)?;
                }
                number(&v, "sourceRevision")?;
                number(&v, "revision")?;
                choice(&v, "generationMode", &["local", "ai"])?;
                nullable_date(&v, "reviewedAt", true)?;
                date_time(&v, "createdAt")?;
                draft_assets::validate_stored(c, &b, &v)?;
            }
            "m_publications" => {
                let mut publication_keys = vec![
                    "id",
                    "brandId",
                    "draftId",
                    "draftRevision",
                    "channel",
                    "url",
                    "publishedAt",
                    "note",
                    "contentSnapshot",
                ];
                for key in ["lifecycle", "readbacks", "withdrawal"] {
                    if v.get(key).is_some() {
                        publication_keys.push(key);
                    }
                }
                keys(&v, &publication_keys)?;
                exact_guid(&v, "draftId", x.as_deref().ok_or("发布稿件索引缺失")?)?;
                let d = load(
                    c,
                    "m_drafts",
                    &b,
                    v["draftId"].as_str().ok_or("发布稿件缺失")?,
                )?;
                if number(&v, "draftRevision")? > number(&d, "revision")? {
                    return Err("发布版本超出稿件版本".into());
                }
                string(&v, "channel", 100, true)?;
                web_url(&string(&v, "url", 2000, true)?, true)?;
                date_time(&v, "publishedAt")?;
                string(&v, "note", 5000, false)?;
                string(&v, "contentSnapshot", 31000, true)?;
                let expected = fingerprint(&json!([
                    b,
                    v["draftId"],
                    v["draftRevision"],
                    v["channel"],
                    v["url"],
                    v["publishedAt"]
                ]));
                if y.as_deref() != Some(expected.as_str()) {
                    return Err("发布去重索引不一致".into());
                }
                validate_publication_extensions(&v)?;
            }
            "m_observations" => {
                const LEGACY_KEYS: &[&str] = &[
                    "id",
                    "brandId",
                    "productId",
                    "questionId",
                    "querySnapshot",
                    "branded",
                    "platform",
                    "surface",
                    "phase",
                    "modelLabel",
                    "searchMode",
                    "testedAt",
                    "answer",
                    "mentioned",
                    "accuracy",
                    "citedUrl",
                    "evidenceNote",
                    "assessment",
                    "assessmentRevision",
                    "assessmentHistory",
                ];
                const REAL_KEYS: &[&str] = &[
                    "evidenceMode",
                    "sourceKind",
                    "transport",
                    "responseModel",
                    "providerRequestId",
                    "citations",
                    "preparationHash",
                    "resultIdentity",
                ];
                let real = REAL_KEYS.iter().any(|key| v.get(*key).is_some());
                let observation_keys: Vec<_> = LEGACY_KEYS
                    .iter()
                    .copied()
                    .chain(real.then_some(REAL_KEYS).into_iter().flatten().copied())
                    .collect();
                keys(&v, &observation_keys)?;
                exact_guid(&v, "productId", x.as_deref().ok_or("观察商品索引缺失")?)?;
                exact_guid(&v, "questionId", y.as_deref().ok_or("观察问题索引缺失")?)?;
                owned_product(c, &v, &b)?;
                optional_question(c, &v, &b, x.as_deref().ok_or("商品标识缺失")?)?;
                string(&v, "querySnapshot", 1000, true)?;
                boolean(&v, "branded")?;
                choice(&v, "platform", &["doubao", "yuanbao", "qianwen", "baidu"])?;
                choice(&v, "surface", &["consumer", "api"])?;
                choice(&v, "phase", &["baseline", "followup"])?;
                string(&v, "modelLabel", 200, true)?;
                string(&v, "searchMode", 200, true)?;
                date_time(&v, "testedAt")?;
                string(&v, "answer", 30000, true)?;
                boolean(&v, "mentioned")?;
                choice(&v, "accuracy", &["unknown", "correct", "incorrect"])?;
                web_url(&string(&v, "citedUrl", 2000, false)?, false)?;
                string(&v, "evidenceNote", 5000, false)?;
                if v.get("assessment").is_some()
                    && experiments::assessment(c, &b, &v, true)? != v["assessment"]
                {
                    return Err("样本评估字段不规范".into());
                }
                validate_real_observation(&v, false)?;
                observations::validate_history_with(&v, |sample| {
                    boolean(sample, "mentioned")?;
                    choice(sample, "accuracy", &["unknown", "correct", "incorrect"])?;
                    web_url(&string(sample, "citedUrl", 2000, false)?, false)?;
                    string(sample, "evidenceNote", 5000, false)?;
                    if sample.get("assessment").is_some()
                        && experiments::assessment(c, &b, sample, true)? != sample["assessment"]
                    {
                        return Err("历史样本评估字段不规范".into());
                    }
                    validate_real_observation(sample, true)
                })?;
                let sample = observations::submission(&v);
                if z.as_deref() != Some(fingerprint(&sample).as_str()) {
                    return Err("观察去重索引不一致".into());
                }
            }
            _ => return Err("未知业务类型".into()),
        }
    }
    Ok(())
}
fn lowercase_sha256(value: &Value, key: &str) -> Result<String> {
    let hash = string(value, key, 64, true)?;
    if hash.len() != 64
        || !hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(format!("字段 {key} 必须为 64 位小写 SHA256"));
    }
    Ok(hash)
}
fn validate_publication_extensions(publication: &Value) -> Result<()> {
    let lifecycle = publication.get("lifecycle");
    let readbacks = publication.get("readbacks");
    let withdrawal = publication.get("withdrawal");
    if lifecycle.is_none() && (readbacks.is_some() || withdrawal.is_some()) {
        return Err("发布生命周期字段不完整".into());
    }
    if let Some(value) = lifecycle {
        choice(
            &json!({"lifecycle":value}),
            "lifecycle",
            &[
                "online",
                "content_changed",
                "unreachable",
                "withdrawing",
                "withdrawn",
                "withdrawal_unverified",
            ],
        )?;
    }
    if let Some(value) = readbacks {
        let entries = value.as_array().ok_or("发布回读历史格式错误")?;
        if entries.is_empty() || entries.len() > 12 {
            return Err("发布回读历史为空或超过上限".into());
        }
        for entry in entries {
            keys(
                entry,
                &[
                    "fetchedAt",
                    "httpStatus",
                    "finalUrl",
                    "title",
                    "bodySha256",
                    "draftRevision",
                    "matchedContent",
                    "errorCategory",
                    "transport",
                ],
            )?;
            date_time(entry, "fetchedAt")?;
            let status = match entry.get("httpStatus") {
                Some(Value::Null) => None,
                Some(Value::Number(value)) => {
                    value.as_u64().filter(|status| (100..=599).contains(status))
                }
                _ => None,
            };
            if entry.get("httpStatus") != Some(&Value::Null) && status.is_none() {
                return Err("发布回读 HTTP 状态无效".into());
            }
            let final_url = string(entry, "finalUrl", 2000, true)?;
            web_url(&final_url, true)?;
            string(entry, "title", 300, false)?;
            if number(entry, "draftRevision")? != number(publication, "draftRevision")? {
                return Err("发布回读稿件版本不一致".into());
            }
            let matched = boolean(entry, "matchedContent")?;
            let body_hash = match entry.get("bodySha256") {
                Some(Value::Null) => None,
                Some(_) => Some(lowercase_sha256(entry, "bodySha256")?),
                None => return Err("发布回读正文摘要缺失".into()),
            };
            let error = match entry.get("errorCategory") {
                Some(Value::Null) => None,
                Some(Value::String(value))
                    if [
                        "dns",
                        "timeout",
                        "transport",
                        "http_status",
                        "response_too_large",
                        "invalid_response",
                    ]
                    .contains(&value.as_str()) =>
                {
                    Some(value.as_str())
                }
                _ => return Err("发布回读错误分类无效".into()),
            };
            let success = status.is_some_and(|status| (200..300).contains(&status));
            if success {
                if error.is_some() || body_hash.is_none() {
                    return Err("成功回读缺少正文摘要或含错误分类".into());
                }
            } else if matched
                || error.is_none()
                || status.is_some() != (error == Some("http_status"))
            {
                return Err("失败回读状态、匹配结果或错误分类不一致".into());
            }
            let transport = choice(entry, "transport", &["http", "https"])?;
            if (transport == "https") != final_url.starts_with("https://") {
                return Err("发布回读传输协议与网址不一致".into());
            }
        }
        if withdrawal.is_none() {
            let latest = &entries[0];
            let success = latest["httpStatus"]
                .as_u64()
                .is_some_and(|status| (200..300).contains(&status));
            let expected = if !success {
                "unreachable"
            } else if latest["matchedContent"] == true {
                "online"
            } else {
                "content_changed"
            };
            if lifecycle.and_then(Value::as_str) != Some(expected) {
                return Err("发布生命周期与最近回读不一致".into());
            }
        }
    } else if withdrawal.is_none() && lifecycle.is_some() {
        return Err("发布生命周期缺少回读依据".into());
    }
    if let Some(value) = withdrawal {
        keys(
            value,
            &["status", "reason", "requestedAt", "updatedAt", "history"],
        )?;
        let status = choice(
            value,
            "status",
            &["withdrawing", "withdrawn", "withdrawal_unverified"],
        )?;
        if lifecycle.and_then(Value::as_str) != Some(status.as_str()) {
            return Err("撤回状态与发布生命周期不一致".into());
        }
        string(value, "reason", 2000, true)?;
        let requested = DateTime::parse_from_rfc3339(&date_time(value, "requestedAt")?)
            .map_err(|_| "撤回申请时间无效")?;
        let updated = DateTime::parse_from_rfc3339(&date_time(value, "updatedAt")?)
            .map_err(|_| "撤回更新时间无效")?;
        if updated < requested {
            return Err("撤回时间顺序无效".into());
        }
        let history = value["history"].as_array().ok_or("撤回历史格式错误")?;
        if history.len() > 12 {
            return Err("撤回历史超过上限".into());
        }
        for entry in history {
            keys(entry, &["status", "reason", "recordedAt"])?;
            choice(
                entry,
                "status",
                &["withdrawing", "withdrawn", "withdrawal_unverified"],
            )?;
            string(entry, "reason", 2000, true)?;
            date_time(entry, "recordedAt")?;
        }
    }
    Ok(())
}
fn validate_real_observation(observation: &Value, historical: bool) -> Result<()> {
    if observation.get("sourceKind").is_none() {
        return Ok(());
    }
    for (key, expected) in [
        ("evidenceMode", "recorded"),
        ("sourceKind", "real_api"),
        ("transport", "real_api"),
        ("surface", "api"),
    ] {
        if observation[key] != expected {
            return Err("真实监测来源组合无效".into());
        }
    }
    if (historical || observation.get("assessmentHistory").is_none())
        && observation["accuracy"] != "unknown"
    {
        return Err("真实监测原始复核状态必须为 unknown".into());
    }
    let preparation_hash = lowercase_sha256(observation, "preparationHash")?;
    let response_model = string(observation, "responseModel", 200, true)?;
    if observation["responseModel"] != observation["modelLabel"] {
        return Err("真实监测返回模型与冻结模型不一致".into());
    }
    let provider_request_id = match observation.get("providerRequestId") {
        Some(Value::Null) => Value::Null,
        Some(_) => json!(string(observation, "providerRequestId", 300, true)?),
        None => return Err("真实监测请求标识缺失".into()),
    };
    let citations = observation["citations"]
        .as_array()
        .ok_or("真实监测引用列表缺失")?;
    if citations.len() > 20 {
        return Err("真实监测引用超过上限".into());
    }
    let mut seen = HashSet::new();
    for citation in citations {
        let wrapped = json!({"url":citation});
        let url = string(&wrapped, "url", 2000, true)?;
        web_url(&url, true)?;
        if !seen.insert(url) {
            return Err("真实监测引用重复".into());
        }
    }
    if (historical || observation.get("assessmentHistory").is_none())
        && observation["citedUrl"] != citations.first().cloned().unwrap_or_else(|| json!(""))
    {
        return Err("真实监测主引用与引用列表不一致".into());
    }
    let expected = fingerprint(&json!({
        "preparationHash":preparation_hash,
        "testedAt":observation["testedAt"],
        "model":response_model,
        "providerRequestId":provider_request_id,
        "answer":observation["answer"],
        "citations":observation["citations"]
    }));
    if observation["resultIdentity"] != expected {
        return Err("真实监测结果身份不一致".into());
    }
    lowercase_sha256(observation, "resultIdentity")?;
    Ok(())
}
fn validate_real_observation_identities(c: &Connection) -> Result<()> {
    let mut statement = c
        .prepare("SELECT brand_id,data FROM m_observations")
        .map_err(db_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(db_error)?;
    let mut identities = HashSet::new();
    for row in rows {
        let (brand_id, data) = row.map_err(db_error)?;
        let observation = stored(data)?;
        if observation["sourceKind"] == "real_api"
            && !identities.insert((brand_id, observation["resultIdentity"].clone()))
        {
            return Err("真实监测结果身份重复".into());
        }
    }
    Ok(())
}
fn research_dedup(product_id: Option<&str>, question: &str) -> Result<String> {
    let normalized = brand_research::normalize_question(question);
    if normalized.is_empty() {
        return Err("研究问题必须包含文字或数字".into());
    }
    Ok(hex::encode(Sha256::digest(
        format!(
            "m-geo-question-research-v1\0{}\0{normalized}",
            product_id.unwrap_or("")
        )
        .as_bytes(),
    )))
}
fn validate_research(c: &Connection) -> Result<()> {
    let mut statement = c
        .prepare("SELECT id,brand_id,product_id,dedup,data FROM m_question_research")
        .map_err(db_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })
        .map_err(db_error)?;
    let mut counts = std::collections::BTreeMap::<String, usize>::new();
    for row in rows {
        let (id, brand_id, product_id, dedup, data) = row.map_err(db_error)?;
        let research = stored(data)?;
        keys(
            &research,
            &[
                "id",
                "brandId",
                "productId",
                "question",
                "intent",
                "sourceKind",
                "sourceLabel",
                "sourceUrl",
                "observedAt",
                "clusterKey",
                "status",
                "revision",
                "createdAt",
                "updatedAt",
            ],
        )?;
        record_identity(c, &research, &id, &brand_id)?;
        match (&research["productId"], product_id.as_deref()) {
            (Value::Null, None) => {}
            (Value::String(id), Some(sql_id)) if id == sql_id => {
                owned_product(c, &research, &brand_id)?;
            }
            _ => return Err("研究商品归属或索引不一致".into()),
        }
        let question = string(&research, "question", 1000, true)?;
        choice(
            &research,
            "intent",
            &["selection", "comparison", "usage", "after_sales"],
        )?;
        choice(
            &research,
            "sourceKind",
            &["manual", "imported", "suggested"],
        )?;
        string(&research, "sourceLabel", 500, true)?;
        web_url(&string(&research, "sourceUrl", 2000, false)?, false)?;
        date_time(&research, "observedAt")?;
        string(&research, "clusterKey", 120, false)?;
        choice(&research, "status", &["candidate", "confirmed", "archived"])?;
        number(&research, "revision")?;
        let created = DateTime::parse_from_rfc3339(&date_time(&research, "createdAt")?)
            .map_err(|_| "研究创建时间无效")?;
        let updated = DateTime::parse_from_rfc3339(&date_time(&research, "updatedAt")?)
            .map_err(|_| "研究更新时间无效")?;
        if updated < created {
            return Err("研究记录时间顺序无效".into());
        }
        if research_dedup(product_id.as_deref(), &question)? != dedup {
            return Err("研究记录去重索引不一致".into());
        }
        let count = counts.entry(brand_id).or_default();
        *count += 1;
        if *count > 5000 {
            return Err("品牌研究记录超过上限".into());
        }
    }
    Ok(())
}
fn media_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.len() >= 4
        && bytes.starts_with(&[0xff, 0xd8, 0xff])
        && bytes.ends_with(&[0xff, 0xd9])
    {
        Some("image/jpeg")
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        Some("video/mp4")
    } else if bytes.starts_with(&[0x1a, 0x45, 0xdf, 0xa3])
        && bytes[..bytes.len().min(4096)]
            .windows(4)
            .any(|window| window.eq_ignore_ascii_case(b"webm"))
    {
        Some("video/webm")
    } else {
        None
    }
}
fn media_metadata(value: &Value, key: &str, max: usize) -> Result<String> {
    let text = string(value, key, max, false)?;
    let lower = text.to_lowercase();
    let bytes = text.as_bytes();
    let windows_drive = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\');
    if lower.contains("file://")
        || text.starts_with('/')
        || text.starts_with("~/")
        || text.starts_with("\\\\")
        || windows_drive
    {
        return Err("媒体元数据不能包含本机路径".into());
    }
    Ok(text)
}
fn validate_media(c: &Connection) -> Result<()> {
    let mut statement = c
        .prepare("SELECT id,brand_id,dedup,data,blob FROM m_media")
        .map_err(db_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Vec<u8>>(4)?,
            ))
        })
        .map_err(db_error)?;
    for row in rows {
        let (id, brand_id, dedup, data, blob) = row.map_err(db_error)?;
        if blob.is_empty() || blob.len() > 1_500_000 {
            return Err("媒体 BLOB 为空或超过上限".into());
        }
        let media = stored(data)?;
        keys(
            &media,
            &[
                "id",
                "brandId",
                "filename",
                "mime",
                "size",
                "sha256",
                "alt",
                "caption",
                "source",
                "rightsNote",
                "reviewStatus",
                "archived",
                "revision",
                "createdAt",
                "updatedAt",
                "reviewedAt",
            ],
        )?;
        record_identity(c, &media, &id, &brand_id)?;
        let filename = string(&media, "filename", 240, true)?;
        if filename.trim() != filename
            || filename.contains(['/', '\\'])
            || filename.chars().any(|character| {
                character.is_control()
                    || matches!(character, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
            })
        {
            return Err("媒体文件名称不规范".into());
        }
        let mime = choice(
            &media,
            "mime",
            &[
                "image/png",
                "image/jpeg",
                "image/webp",
                "image/gif",
                "video/mp4",
                "video/webm",
            ],
        )?;
        if media["size"].as_u64() != Some(blob.len() as u64)
            || media_mime(&blob) != Some(mime.as_str())
        {
            return Err("媒体 BLOB 长度或 MIME 与元数据不一致".into());
        }
        let sha256 = lowercase_sha256(&media, "sha256")?;
        if dedup != sha256 || hex::encode(Sha256::digest(&blob)) != sha256 {
            return Err("媒体 BLOB 摘要或去重索引不一致".into());
        }
        media_metadata(&media, "alt", 500)?;
        media_metadata(&media, "caption", 2000)?;
        let source = media_metadata(&media, "source", 2000)?;
        let rights = media_metadata(&media, "rightsNote", 4000)?;
        let status = choice(&media, "reviewStatus", &["pending", "approved", "rejected"])?;
        boolean(&media, "archived")?;
        number(&media, "revision")?;
        let created = DateTime::parse_from_rfc3339(&date_time(&media, "createdAt")?)
            .map_err(|_| "媒体创建时间无效")?;
        let updated = DateTime::parse_from_rfc3339(&date_time(&media, "updatedAt")?)
            .map_err(|_| "媒体更新时间无效")?;
        if updated < created {
            return Err("媒体时间顺序无效".into());
        }
        nullable_date(&media, "reviewedAt", true)?;
        if (status == "pending") != media["reviewedAt"].is_null()
            || (status == "approved" && (source.trim().is_empty() || rights.trim().is_empty()))
        {
            return Err("媒体审核状态、时间或权利资料不一致".into());
        }
    }
    Ok(())
}
fn validate_assets(c: &Connection) -> Result<()> {
    let mut statement = c
        .prepare(
            "SELECT id,brand_id,product_id,request_id,request_hash,created_at,data FROM m_assets",
        )
        .map_err(db_error)?;
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
        .map_err(db_error)?;
    for row in rows {
        let (id, brand_id, product_id, request_id, request_hash, created_at, data) =
            row.map_err(db_error)?;
        let asset = stored(data)?;
        keys(
            &asset,
            &[
                "id",
                "brandId",
                "kind",
                "productId",
                "text",
                "source",
                "reviewStatus",
                "reviewNote",
                "reviewedAt",
                "archivedAt",
                "archiveNote",
                "revision",
                "createdAt",
                "updatedAt",
                "history",
            ],
        )?;
        record_identity(c, &asset, &id, &brand_id)?;
        choice(&asset, "kind", &["keyword", "title"])?;
        match (&asset["productId"], product_id) {
            (Value::Null, None) => {}
            (Value::String(id), Some(sql_id)) if *id == sql_id => {
                load(c, "m_products", &brand_id, id)?;
            }
            _ => return Err("内容资产商品关联与索引不一致".into()),
        }
        string(&asset, "text", 500, true)?;
        string(&asset, "source", 2000, true)?;
        let status = choice(&asset, "reviewStatus", &["pending", "approved", "rejected"])?;
        string(&asset, "reviewNote", 2000, false)?;
        if status == "rejected" && asset["reviewNote"].as_str().is_none_or(str::is_empty) {
            return Err("已驳回内容资产缺少审核理由".into());
        }
        nullable_date(&asset, "reviewedAt", true)?;
        if (status == "pending") != asset["reviewedAt"].is_null() {
            return Err("内容资产审核状态与时间不一致".into());
        }
        if status == "pending" && asset["reviewNote"] != "" {
            return Err("待审核内容资产不能保留审核理由".into());
        }
        if !asset["archivedAt"].is_null() {
            date_time(&asset, "archivedAt")?;
        }
        string(&asset, "archiveNote", 2000, false)?;
        if asset["archivedAt"].is_null() && asset["archiveNote"] != "" {
            return Err("未归档内容资产不能保留归档说明".into());
        }
        let revision = number(&asset, "revision")?;
        date_time(&asset, "createdAt")?;
        date_time(&asset, "updatedAt")?;
        if asset["createdAt"].as_str() != Some(created_at.as_str()) {
            return Err("内容资产创建时间与索引不一致".into());
        }
        guid(&json!({"id":request_id}), "id")?;
        if request_hash.len() != 64 || !request_hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("内容资产创建指纹不规范".into());
        }
        let history = asset["history"].as_array().ok_or("内容资产历史缺失")?;
        if history.len() != usize::try_from(revision).map_err(|_| "内容资产版本超限")? {
            return Err("内容资产历史版本不连续".into());
        }
        let mut history_archived = false;
        for (index, entry) in history.iter().enumerate() {
            keys(
                entry,
                &[
                    "revision",
                    "action",
                    "text",
                    "source",
                    "productId",
                    "reviewStatus",
                    "reviewNote",
                    "reviewedAt",
                    "changedAt",
                ],
            )?;
            if number(entry, "revision")? != i64::try_from(index + 1).unwrap_or(i64::MAX) {
                return Err("内容资产历史版本不连续".into());
            }
            let action = choice(
                entry,
                "action",
                &[
                    "created", "revised", "approved", "rejected", "archived", "restored",
                ],
            )?;
            let entry_status = choice(entry, "reviewStatus", &["pending", "approved", "rejected"])?;
            let valid_action = match action.as_str() {
                "created" => index == 0 && entry_status == "pending" && !history_archived,
                "revised" => index > 0 && entry_status == "pending" && !history_archived,
                "approved" | "rejected" => action == entry_status && !history_archived,
                "archived" if !history_archived => {
                    history_archived = true;
                    index > 0
                }
                "restored" if history_archived => {
                    history_archived = false;
                    index > 0
                }
                _ => false,
            };
            if !valid_action {
                return Err("内容资产历史操作、归档顺序或审核状态不一致".into());
            }
            string(entry, "text", 500, true)?;
            string(entry, "source", 2000, true)?;
            match &entry["productId"] {
                Value::Null => {}
                Value::String(product_id) => {
                    load(c, "m_products", &brand_id, product_id)?;
                }
                _ => return Err("内容资产历史商品关联不规范".into()),
            }
            string(entry, "reviewNote", 2000, false)?;
            if entry_status == "rejected" && entry["reviewNote"].as_str().is_none_or(str::is_empty)
            {
                return Err("历史驳回记录缺少审核理由".into());
            }
            if entry_status == "pending" && entry["reviewNote"] != "" {
                return Err("历史待审核记录不能保留审核理由".into());
            }
            nullable_date(entry, "reviewedAt", true)?;
            if (entry_status == "pending") != entry["reviewedAt"].is_null() {
                return Err("内容资产历史审核状态与时间不一致".into());
            }
            date_time(entry, "changedAt")?;
        }
        let current = history.last().ok_or("内容资产历史缺失")?;
        for key in [
            "revision",
            "text",
            "source",
            "productId",
            "reviewStatus",
            "reviewNote",
            "reviewedAt",
        ] {
            if current[key] != asset[key] {
                return Err("内容资产当前版本与历史不一致".into());
            }
        }
        if current["changedAt"] != asset["updatedAt"] {
            return Err("内容资产更新时间与历史不一致".into());
        }
        if history_archived != !asset["archivedAt"].is_null() {
            return Err("内容资产当前归档状态与历史不一致".into());
        }
    }
    Ok(())
}
fn validate_monitor_preparation(c: &Connection, ctx: &Value, draft_id: Option<&str>) -> Result<()> {
    keys(
        ctx,
        &[
            "monitorVersion",
            "brandId",
            "productId",
            "questionId",
            "kind",
            "campaignId",
            "platform",
            "surface",
            "phase",
            "modelLabel",
            "searchMode",
            "configVersion",
            "sourceRevision",
            "publicSnapshot",
            "prompt",
            "evidenceMode",
            "sourceKind",
            "transport",
            "assessment",
        ],
    )?;
    if draft_id.is_some()
        || number(ctx, "monitorVersion")? != 1
        || number(ctx, "configVersion")? < 1
        || ctx["kind"] != "monitor"
        || ctx["surface"] != "api"
        || ctx["evidenceMode"] != "recorded"
        || ctx["sourceKind"] != "real_api"
        || ctx["transport"] != "real_api"
    {
        return Err("真实监测准备类型或来源无效".into());
    }
    number(ctx, "sourceRevision")?;
    choice(ctx, "phase", &["baseline", "followup"])?;
    choice(ctx, "platform", &["doubao", "yuanbao", "qianwen", "baidu"])?;
    string(ctx, "modelLabel", 200, true)?;
    string(ctx, "searchMode", 200, true)?;
    string(ctx, "prompt", 1_000_000, true)?;
    let b = guid(ctx, "brandId")?;
    brand(c, &b)?;
    let product = owned_product(c, &ctx, &b)?;
    let product_id = product["id"].as_str().ok_or("商品标识缺失")?;
    optional_question(c, ctx, &b, product_id)?;
    let question = load(c, "m_questions", &b, &guid(ctx, "questionId")?)?;
    guid(ctx, "campaignId")?;
    let snapshot = &ctx["publicSnapshot"];
    keys(
        snapshot,
        &[
            "brandName",
            "productName",
            "sku",
            "category",
            "officialUrl",
            "facts",
            "question",
        ],
    )?;
    if snapshot["question"] != Value::Null {
        return Err("真实监测公开快照包含生成问题或随机标识".into());
    }
    string(snapshot, "brandName", 100, true)?;
    string(snapshot, "productName", 200, true)?;
    string(snapshot, "sku", 100, true)?;
    string(snapshot, "category", 200, false)?;
    web_url(&string(snapshot, "officialUrl", 2000, false)?, false)?;
    valid_facts(snapshot, true)?;
    let observation = json!({
        "brandId":ctx["brandId"],"productId":ctx["productId"],"questionId":ctx["questionId"],
        "querySnapshot":question["text"],"branded":false,"platform":ctx["platform"],
        "surface":ctx["surface"],"phase":ctx["phase"],"modelLabel":ctx["modelLabel"],
        "searchMode":ctx["searchMode"],"evidenceMode":ctx["evidenceMode"],
        "sourceKind":ctx["sourceKind"],"transport":ctx["transport"],"mentioned":false,
        "assessment":ctx["assessment"]
    });
    if experiments::assessment(c, &b, &observation, true)? != ctx["assessment"] {
        return Err("真实监测准备评估字段不规范".into());
    }
    Ok(())
}

fn validate_preparations(c: &Connection) -> Result<()> {
    let mut st = c
        .prepare("SELECT hash,context,draft_id FROM m_ai_preparations")
        .map_err(db_error)?;
    let rows = st
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(db_error)?;
    for row in rows {
        let (hash, data, draft_id) = row.map_err(db_error)?;
        let ctx = stored_limit(data, 8_400_000)?;
        if fingerprint(&ctx) != hash {
            return Err("生成上下文校验不一致".into());
        }
        if ctx["kind"] == "monitor" {
            validate_monitor_preparation(c, &ctx, draft_id.as_deref())?;
            continue;
        }
        keys(
            &ctx,
            &[
                "brandId",
                "productId",
                "kind",
                "channel",
                "channelTemplateVersion",
                "questionId",
                "sourceRevision",
                "sourceSnapshot",
                "title",
                "prompt",
            ],
        )?;
        let b = guid(&ctx, "brandId")?;
        brand(c, &b)?;
        let p = owned_product(c, &ctx, &b)?;
        let pid = p["id"].as_str().ok_or("商品标识缺失")?;
        optional_question(c, &ctx, &b, pid)?;
        kind(&ctx)?;
        channels::parse(&ctx, "channel")?;
        if ctx.get("channel").is_some_and(|value| !value.is_null()) {
            if ctx.get("channelTemplateVersion") != Some(&json!(channels::TEMPLATE_VERSION)) {
                return Err("生成上下文的渠道模板版本不受支持".into());
            }
        } else if ctx
            .get("channelTemplateVersion")
            .is_some_and(|value| !value.is_null() && *value != json!(0))
        {
            return Err("通用生成上下文的渠道模板版本不正确".into());
        }
        number(&ctx, "sourceRevision")?;
        string(&ctx, "title", 300, true)?;
        string(&ctx, "prompt", 1_000_000, true)?;
        let snapshot = ctx.get("sourceSnapshot").ok_or("缺少生成资料快照")?;
        keys(
            snapshot,
            &[
                "brandName",
                "productName",
                "sku",
                "category",
                "officialUrl",
                "facts",
                "question",
                "preparationId",
            ],
        )?;
        string(snapshot, "brandName", 100, true)?;
        string(snapshot, "productName", 200, true)?;
        string(snapshot, "sku", 100, true)?;
        string(snapshot, "category", 200, false)?;
        web_url(&string(snapshot, "officialUrl", 2000, false)?, false)?;
        valid_facts(snapshot, true)?;
        if snapshot.get("preparationId").is_some() {
            guid(snapshot, "preparationId")?;
        }
        match (&ctx["questionId"], snapshot.get("question")) {
            (Value::Null, Some(Value::Null)) => (),
            (Value::String(qid), Some(q)) => {
                keys(q, &["id", "text", "branded"])?;
                exact_guid(q, "id", qid)?;
                string(q, "text", 1000, true)?;
                boolean(q, "branded")?;
            }
            _ => return Err("生成问题快照不一致".into()),
        }
        if let Some(did) = draft_id {
            let d = load(c, "m_drafts", &b, &did)?;
            if d["productId"] != ctx["productId"]
                || d["sourceRevision"] != ctx["sourceRevision"]
                || d["kind"] != ctx["kind"]
                || d["questionId"] != ctx["questionId"]
                || d["generationMode"] != "ai"
            {
                return Err("生成记录与稿件关联不一致".into());
            }
        }
    }
    Ok(())
}
fn validate_migration(c: &Connection) -> Result<()> {
    let mut maps = c
        .prepare("SELECT source,kind,old_id,brand_id,new_id FROM m_migration_map")
        .map_err(db_error)?;
    let rows = maps
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        })
        .map_err(db_error)?;
    for row in rows {
        let (src, kind, old, b, new) = row.map_err(db_error)?;
        migration_id(&src, &kind, &old)?;
        let table = match kind.as_str() {
            "product" => "m_products",
            "question" => "m_questions",
            "draft" => "m_drafts",
            "publication" => "m_publications",
            "observation" => "m_observations",
            _ => return Err("迁移类型错误".into()),
        };
        load(c, table, &b, &new)?;
    }
    let mut skips = c
        .prepare("SELECT source,kind,old_id,brand_id FROM m_migration_skips")
        .map_err(db_error)?;
    let rows = skips
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })
        .map_err(db_error)?;
    for row in rows {
        let (src, kind, old, b) = row.map_err(db_error)?;
        migration_id(&src, &kind, &old)?;
        brand(c, &b)?;
        let mapped = c
            .query_row(
                "SELECT brand_id FROM m_migration_map WHERE source=?1 AND kind=?2 AND old_id=?3",
                params![src, kind, old],
                |r| r.get::<_, String>(0),
            )
            .optional()
            .map_err(db_error)?;
        if mapped.is_some_and(|other| other != b) {
            return Err("迁移跳过记录归属不一致".into());
        }
    }
    let mut history = c
        .prepare("SELECT source,kind,old_id,data FROM m_migration_history")
        .map_err(db_error)?;
    let rows = history
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })
        .map_err(db_error)?;
    for row in rows {
        let (src, kind, old, data) = row.map_err(db_error)?;
        migration_id(&src, &kind, &old)?;
        let v = stored(data)?;
        keys(
            &v,
            &[
                "ownerLabel",
                "reviewerLabel",
                "confirmedAt",
                "reviewedAt",
                "sourceSnapshot",
                "reviewSnapshot",
                "confirmationSnapshot",
                "revalidationStatus",
            ],
        )?;
        text(&v, "ownerLabel", 200, false)?;
        text(&v, "reviewerLabel", 200, false)?;
        nullable_date(&v, "confirmedAt", false)?;
        nullable_date(&v, "reviewedAt", false)?;
        for key in ["sourceSnapshot", "reviewSnapshot", "confirmationSnapshot"] {
            historical_snapshot(&v, key)?;
        }
        if v.get("revalidationStatus").is_some() {
            choice(&v, "revalidationStatus", &["pending"])?;
        }
    }
    Ok(())
}
fn migration_id(src: &str, kind: &str, old: &str) -> Result<()> {
    let v = json!({"source":src,"kind":kind,"old":old});
    string(&v, "source", 200, true)?;
    string(&v, "old", 200, true)?;
    choice(
        &v,
        "kind",
        &["product", "question", "draft", "publication", "observation"],
    )?;
    Ok(())
}
fn validate_audit(c: &Connection) -> Result<()> {
    let mut st = c
        .prepare("SELECT id,occurred_at,action,brand_id,object_id FROM m_audit")
        .map_err(db_error)?;
    let rows = st
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<String>>(4)?,
            ))
        })
        .map_err(db_error)?;
    for row in rows {
        let (id, time, action, b, object) = row.map_err(db_error)?;
        if id <= 0 {
            return Err("审计标识错误".into());
        }
        date_time(&json!({"time":time}), "time")?;
        if ![
            "brand.create",
            "product.save",
            "product.confirm",
            "question.save",
            "question.confirm",
            "question.suggest",
            "draft.generateLocal",
            "draft.generateChannels",
            "draft.save",
            "draft.review",
            "publication.add",
            "observation.add",
            "observation.assess",
            "campaign.create",
            "attribution.apply",
            "optimization.create",
            "optimization.update",
            "optimization.check",
            "p0Workflow.create",
            "p0Workflow.resume",
            "p0Workflow.pause",
            "p0Workflow.cancel",
            "p0Workflow.retry",
            "p0Workflow.step",
            "p0Workflow.approvePublish",
            "p0Workflow.reconcile",
            "p0Workflow.rollback",
            "p0Workflow.policyUpdate",
            "p0Workflow.haltAll",
            "offlineWorkflow.create",
            "offlineWorkflow.resume",
            "offlineWorkflow.pause",
            "offlineWorkflow.step",
            "offlineWorkflow.retry",
            "offlineWorkflow.cancel",
            "localJobs.create",
            "localJobs.resume",
            "localJobs.pause",
            "localJobs.step",
            "localJobs.retry",
            "localJobs.cancel",
            "commerce.applyBatch",
            "pilot.createQuestions",
            "collection.apply",
            "import.csv",
            "import.cnvp",
            "knowledge.import",
            "knowledge.revise",
            "knowledge.review",
            "asset.create",
            "asset.revise",
            "asset.review",
            "asset.setArchived",
            "brand.profile.save",
            "research.save",
            "research.archive",
            "research.confirm",
            "research.confirmToQuestion",
            "publication.recordReadback",
            "publication.withdraw",
            "media.import",
            "media.review",
            "media.revise",
            "media.setArchived",
            "automation.create",
            "automation.pause",
            "automation.resume",
            "automation.runDue",
            "automation.cancel",
            "internal.ai.commit",
            "internal.monitor.commit",
        ]
        .contains(&action.as_str())
        {
            return Err("审计操作类型错误".into());
        }
        let b = b.ok_or("审计品牌缺失")?;
        brand(c, &b)?;
        let table = match action.as_str() {
            "campaign.create" => Some("m_campaigns"),
            "optimization.create" | "optimization.update" | "optimization.check" => {
                Some("m_optimization_plans")
            }
            "attribution.apply" => Some("m_attribution_datasets"),
            "p0Workflow.create"
            | "p0Workflow.resume"
            | "p0Workflow.pause"
            | "p0Workflow.cancel"
            | "p0Workflow.retry"
            | "p0Workflow.step"
            | "p0Workflow.approvePublish"
            | "p0Workflow.reconcile"
            | "p0Workflow.rollback" => Some("m_p0_workflows"),
            "offlineWorkflow.create"
            | "offlineWorkflow.resume"
            | "offlineWorkflow.pause"
            | "offlineWorkflow.step"
            | "offlineWorkflow.retry"
            | "offlineWorkflow.cancel" => Some("m_offline_workflows"),
            "localJobs.create" | "localJobs.resume" | "localJobs.pause" | "localJobs.step"
            | "localJobs.retry" | "localJobs.cancel" => Some("m_local_jobs"),
            "product.save" | "product.confirm" => Some("m_products"),
            "question.save" | "question.confirm" => Some("m_questions"),
            "draft.generateLocal" | "draft.save" | "draft.review" | "internal.ai.commit" => {
                Some("m_drafts")
            }
            "research.save"
            | "research.archive"
            | "research.confirm"
            | "research.confirmToQuestion" => Some("m_question_research"),
            "publication.add" | "publication.recordReadback" | "publication.withdraw" => {
                Some("m_publications")
            }
            "observation.add" | "observation.assess" | "internal.monitor.commit" => {
                Some("m_observations")
            }
            "media.import" | "media.review" | "media.revise" | "media.setArchived" => {
                Some("m_media")
            }
            "automation.create" | "automation.pause" | "automation.resume"
            | "automation.cancel" => Some("m_automation_tasks"),
            "knowledge.import" | "knowledge.revise" | "knowledge.review" => {
                Some("m_knowledge_documents")
            }
            "asset.create" | "asset.revise" | "asset.review" | "asset.setArchived" => {
                Some("m_assets")
            }
            _ => None,
        };
        if let Some(object) = object {
            guid(&json!({"id":object}), "id")?;
            if action == "brand.create" || action == "brand.profile.save" {
                if object != b {
                    return Err("审计品牌对象不一致".into());
                }
            } else if let Some(table) = table {
                load(c, table, &b, &object)?;
            } else {
                return Err("审计对象类型不一致".into());
            }
        } else if table.is_some() || action == "brand.create" || action == "brand.profile.save" {
            return Err("审计对象缺失".into());
        }
    }
    Ok(())
}
