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
    if ![(1, 1), (1, 2), (1, 3), (1, 4), (1, 5)].contains(&version) || total != 1 {
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
        keys(
            f,
            &["id", "label", "value", "source", "visibility", "validUntil"],
        )?;
        string(f, "id", 100, true)?;
        string(f, "label", 200, true)?;
        string(f, "value", 3000, true)?;
        string(f, "source", 2000, public_only)?;
        choice(f, "visibility", &["public", "private"])?;
        if public_only && f["visibility"] != "public" {
            return Err("生成快照不能包含私密事实".into());
        }
    }
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
        keys(&v, &["id", "name", "createdAt"])?;
        exact_guid(&v, "id", &id)?;
        let vn = string(&v, "name", 100, true)?;
        if vn != name {
            return Err("品牌名称与索引不一致".into());
        }
        date_time(&v, "createdAt")?;
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
                        "title",
                        "body",
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
                string(&v, "title", 300, true)?;
                string(&v, "body", 30000, true)?;
                number(&v, "sourceRevision")?;
                number(&v, "revision")?;
                choice(&v, "generationMode", &["local", "ai"])?;
                nullable_date(&v, "reviewedAt", true)?;
                date_time(&v, "createdAt")?;
            }
            "m_publications" => {
                keys(
                    &v,
                    &[
                        "id",
                        "brandId",
                        "draftId",
                        "draftRevision",
                        "channel",
                        "url",
                        "publishedAt",
                        "note",
                        "contentSnapshot",
                    ],
                )?;
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
            }
            "m_observations" => {
                keys(
                    &v,
                    &[
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
                    ],
                )?;
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
                observations::validate_history(c, &b, &v)?;
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
        keys(
            &ctx,
            &[
                "brandId",
                "productId",
                "kind",
                "questionId",
                "sourceRevision",
                "sourceSnapshot",
                "title",
                "prompt",
            ],
        )?;
        if fingerprint(&ctx) != hash {
            return Err("生成上下文校验不一致".into());
        }
        let b = guid(&ctx, "brandId")?;
        brand(c, &b)?;
        let p = owned_product(c, &ctx, &b)?;
        let pid = p["id"].as_str().ok_or("商品标识缺失")?;
        optional_question(c, &ctx, &b, pid)?;
        kind(&ctx)?;
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
            "internal.ai.commit",
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
            "localJobs.create" | "localJobs.resume" | "localJobs.pause" | "localJobs.step"
            | "localJobs.retry" | "localJobs.cancel" => Some("m_local_jobs"),
            "product.save" | "product.confirm" => Some("m_products"),
            "question.save" | "question.confirm" => Some("m_questions"),
            "draft.generateLocal" | "draft.save" | "draft.review" | "internal.ai.commit" => {
                Some("m_drafts")
            }
            "publication.add" => Some("m_publications"),
            "observation.add" | "observation.assess" => Some("m_observations"),
            _ => None,
        };
        if let Some(object) = object {
            guid(&json!({"id":object}), "id")?;
            if action == "brand.create" {
                if object != b {
                    return Err("审计品牌对象不一致".into());
                }
            } else if let Some(table) = table {
                load(c, table, &b, &object)?;
            } else {
                return Err("审计对象类型不一致".into());
            }
        } else if table.is_some() || action == "brand.create" {
            return Err("审计对象缺失".into());
        }
    }
    Ok(())
}
