//! Bounded imports. The caller owns a transaction, so malformed late rows roll back early writes.
use super::*;
const MAX_BYTES: usize = 2 * 1024 * 1024;
fn consent(p: &Value) -> Result<()> {
    if p["confirm"] != true {
        return Err("请先预览并确认导入".into());
    }
    Ok(())
}
fn content(p: &Value) -> Result<&str> {
    let s = p["content"].as_str().ok_or("导入内容必须为文本")?;
    if s.is_empty() || s.len() > MAX_BYTES {
        return Err("导入文件为空或超过2 MB".into());
    }
    Ok(s)
}
pub(super) fn csv(c: &Connection, b: &str, br: &Value, p: &Value) -> Result<Value> {
    consent(p)?;
    let input = content(p)?.trim_start_matches('\u{feff}');
    let mut reader = ::csv::ReaderBuilder::new()
        .trim(::csv::Trim::All)
        .from_reader(input.as_bytes());
    let header = reader.headers().map_err(|_| "CSV 表头无法解析")?.clone();
    if header.iter().collect::<Vec<_>>() != ["品牌", "商品名称", "SKU", "类目", "商品链接"]
    {
        return Err("CSV 必须依次包含：品牌、商品名称、SKU、类目、商品链接".into());
    }
    let mut created = 0;
    let mut skipped = 0;
    let mut count = 0;
    for (index, row) in reader.records().enumerate() {
        count += 1;
        if count > 200 {
            return Err("每次最多导入200行商品".into());
        }
        let line = index + 2;
        let row = row.map_err(|_| format!("CSV 第{}行字段或引号不正确", line))?;
        if row.get(0) != br["name"].as_str() {
            return Err(format!("CSV 第{}行品牌与当前品牌不一致，整批未导入", line));
        }
        let data = json!({"name":row.get(1).unwrap_or(""),"sku":row.get(2).unwrap_or(""),"category":row.get(3).unwrap_or(""),"officialUrl":row.get(4).unwrap_or(""),"sourceText":"","privateNotes":"","facts":[]});
        // Validate even duplicate rows: malformed duplicates cannot mask invalid input.
        text(&data, "name", 200, true).map_err(|e| format!("第{}行：{}", line, e))?;
        let sku = text(&data, "sku", 100, true).map_err(|e| format!("第{}行：{}", line, e))?;
        text(&data, "category", 200, false)?;
        let u = text(&data, "officialUrl", 2000, false)?;
        web_url(&u, false).map_err(|e| format!("第{}行：{}", line, e))?;
        let exists: bool = c
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM m_products WHERE brand_id=?1 AND sku=?2)",
                params![b, sku],
                |r| r.get(0),
            )
            .map_err(db_error)?;
        if exists {
            skipped += 1
        } else {
            save_product(c, b, &data).map_err(|e| format!("第{}行：{}", line, e))?;
            created += 1;
        }
    }
    if count == 0 {
        return Err("CSV 没有商品行".into());
    }
    Ok(json!({"created":created,"skipped":skipped}))
}
pub(super) fn keys(v: &Value, allowed: &[&str]) -> Result<()> {
    let obj = v.as_object().ok_or("导入记录必须为对象")?;
    for key in obj.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(format!(
                "导入包含未支持字段 {}；请使用只含商品业务数据的导出格式",
                key
            ));
        }
    }
    Ok(())
}
fn rows<'a>(v: &'a Value, k: &str) -> Result<&'a Vec<Value>> {
    let rows = v[k]
        .as_array()
        .ok_or_else(|| format!("导入缺少 {} 列表", k))?;
    if rows.len() > 5000 {
        return Err("单类迁移记录超过5000条".into());
    }
    Ok(rows)
}
fn old_id(v: &Value, k: &str) -> Result<String> {
    text(v, k, 200, true)
}
fn find_mapping(
    c: &Connection,
    src: &str,
    kind: &str,
    old: &str,
    b: &str,
) -> Result<Option<String>> {
    let row = c
        .query_row(
            "SELECT brand_id,new_id FROM m_migration_map WHERE source=?1 AND kind=?2 AND old_id=?3",
            params![src, kind, old],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(db_error)?;
    match row {
        None => Ok(None),
        Some((brand, new)) if brand == b => Ok(Some(new)),
        _ => Err("来源记录已归属其他品牌，禁止重新归属".into()),
    }
}
fn map_required(c: &Connection, src: &str, kind: &str, old: &str, b: &str) -> Result<String> {
    find_mapping(c, src, kind, old, b)?
        .ok_or_else(|| format!("导入关联缺失：{} / {}，整批未导入", kind, old))
}
pub(super) fn historical_snapshot(row: &Value, key: &str) -> Result<Value> {
    fn check(v: &Value, depth: usize) -> Result<()> {
        if depth > 20 {
            return Err("历史快照层级过深".into());
        }
        match v {
            Value::Object(map) => {
                for (key, value) in map {
                    let normalized = key.to_lowercase().replace(['_', '-'], "");
                    if [
                        "apikey",
                        "password",
                        "passwd",
                        "secret",
                        "token",
                        "accesstoken",
                        "refreshtoken",
                        "authorization",
                        "cookie",
                        "session",
                        "credentials",
                    ]
                    .contains(&normalized.as_str())
                    {
                        return Err("历史快照包含凭据字段，整批未导入".into());
                    }
                    check(value, depth + 1)?
                }
            }
            Value::Array(values) => {
                for value in values {
                    check(value, depth + 1)?
                }
            }
            Value::String(s) if s.contains('\0') => return Err("历史快照含非法字符".into()),
            _ => (),
        }
        Ok(())
    }
    let value = row.get(key).cloned().unwrap_or(Value::Null);
    if value.to_string().len() > 300000 {
        return Err("单个历史快照超过300 KB".into());
    }
    check(&value, 0)?;
    Ok(value)
}
fn add_mapping(
    c: &Connection,
    src: &str,
    kind: &str,
    old: &str,
    b: &str,
    new: &str,
    row: &Value,
) -> Result<()> {
    c.execute(
        "INSERT INTO m_migration_map(source,kind,old_id,brand_id,new_id)VALUES(?1,?2,?3,?4,?5)",
        params![src, kind, old, b, new],
    )
    .map_err(db_error)?;
    // Only explicitly allowed, bounded historical labels/status are retained; no account or credentials.
    let meta = json!({"ownerLabel":text(row,"ownerLabel",200,false)?,"reviewerLabel":text(row,"reviewerLabel",200,false)?,"confirmedAt":row.get("confirmedAt").cloned().unwrap_or(Value::Null),"reviewedAt":row.get("reviewedAt").cloned().unwrap_or(Value::Null),"sourceSnapshot":historical_snapshot(row,"sourceSnapshot")?,"reviewSnapshot":historical_snapshot(row,"reviewSnapshot")?,"confirmationSnapshot":historical_snapshot(row,"confirmationSnapshot")?,"revalidationStatus":"pending"});
    c.execute(
        "INSERT INTO m_migration_history(source,kind,old_id,data)VALUES(?1,?2,?3,?4)",
        params![src, kind, old, meta.to_string()],
    )
    .map_err(db_error)?;
    Ok(())
}
fn ownership(row: &Value, brand_name: &Value) -> Result<()> {
    if let Some(n) = row.get("brandName") {
        if n != brand_name {
            return Err("迁移记录包含其他品牌，整批未导入".into());
        }
    }
    Ok(())
}
fn legacy_date(row: &Value, key: &str) -> Result<()> {
    if row.get(key).is_some_and(|v| !v.is_null()) {
        date_time(row, key)?;
    }
    Ok(())
}
fn was_skipped(c: &Connection, src: &str, kind: &str, old: &str, b: &str) -> Result<bool> {
    let brand = c
        .query_row(
            "SELECT brand_id FROM m_migration_skips WHERE source=?1 AND kind=?2 AND old_id=?3",
            params![src, kind, old],
            |r| r.get::<_, String>(0),
        )
        .optional()
        .map_err(db_error)?;
    if brand.as_ref().is_some_and(|other| other != b) {
        return Err("跳过的来源记录已归属其他品牌".into());
    }
    Ok(brand.is_some())
}
fn mark_skip(c: &Connection, src: &str, kind: &str, old: &str, b: &str) -> Result<()> {
    was_skipped(c, src, kind, old, b)?;
    c.execute(
        "INSERT OR IGNORE INTO m_migration_skips(source,kind,old_id,brand_id)VALUES(?1,?2,?3,?4)",
        params![src, kind, old, b],
    )
    .map_err(db_error)?;
    Ok(())
}
fn validate_legacy(kind: &str, r: &Value) -> Result<()> {
    old_id(r, "id")?;
    text(r, "ownerLabel", 200, false)?;
    text(r, "reviewerLabel", 200, false)?;
    for key in ["sourceSnapshot", "reviewSnapshot", "confirmationSnapshot"] {
        historical_snapshot(r, key)?;
    }
    match kind {
        "product" => {
            text(r, "name", 200, true)?;
            text(r, "sku", 100, true)?;
            text(r, "category", 200, false)?;
            web_url(&text(r, "officialUrl", 2000, false)?, false)?;
            raw(r, "sourceText", 100000, false)?;
            raw(r, "privateNotes", 30000, false)?;
            facts(r)?;
            number(r, "revision")?;
            legacy_date(r, "confirmedAt")?;
            legacy_date(r, "updatedAt")?;
        }
        "question" => {
            old_id(r, "productId")?;
            text(r, "text", 1000, true)?;
            choice(
                r,
                "intent",
                &["selection", "comparison", "usage", "after_sales"],
            )?;
            choice(r, "origin", &["customer", "suggestion"])?;
            text(r, "source", 2000, true)?;
            boolean(r, "branded")?;
            boolean(r, "confirmed")?;
        }
        "draft" => {
            old_id(r, "productId")?;
            if r.get("questionId").is_some_and(|v| !v.is_null()) {
                old_id(r, "questionId")?;
            }
            choice(r, "kind", &["description", "faq", "guide", "comparison"])?;
            text(r, "title", 300, true)?;
            raw(r, "body", 30000, true)?;
            number(r, "sourceRevision")?;
            number(r, "revision")?;
            choice(r, "generationMode", &["local", "ai"])?;
            date_time(r, "createdAt")?;
            legacy_date(r, "reviewedAt")?;
        }
        "publication" => {
            old_id(r, "draftId")?;
            number(r, "draftRevision")?;
            text(r, "channel", 100, true)?;
            web_url(&text(r, "url", 2000, true)?, true)?;
            date_time(r, "publishedAt")?;
            raw(r, "note", 5000, false)?;
            let snap = r["contentSnapshot"]
                .as_str()
                .filter(|s| !s.trim().is_empty() && s.chars().count() <= 31000 && !s.contains('\0'))
                .ok_or("发布快照为空或超限")?;
            if let Some(hash) = r.get("contentSnapshotSha256") {
                if hash.as_str() != Some(hex::encode(Sha256::digest(snap.as_bytes())).as_str()) {
                    return Err("发布快照校验不一致，整批未导入".into());
                }
            }
        }
        "observation" => {
            old_id(r, "productId")?;
            old_id(r, "questionId")?;
            text(r, "querySnapshot", 1000, true)?;
            boolean(r, "branded")?;
            choice(r, "platform", &["doubao", "yuanbao", "qianwen", "baidu"])?;
            choice(r, "surface", &["consumer", "api"])?;
            choice(r, "phase", &["baseline", "followup"])?;
            text(r, "modelLabel", 200, true)?;
            text(r, "searchMode", 200, true)?;
            date_time(r, "testedAt")?;
            raw(r, "answer", 30000, true)?;
            boolean(r, "mentioned")?;
            choice(r, "accuracy", &["unknown", "correct", "incorrect"])?;
            web_url(&text(r, "citedUrl", 2000, false)?, false)?;
            raw(r, "evidenceNote", 5000, false)?;
        }
        _ => return Err("不支持的迁移类型".into()),
    };
    Ok(())
}
pub(super) fn cnvp(c: &Connection, b: &str, br: &Value, p: &Value) -> Result<Value> {
    consent(p)?;
    let src = text(p, "sourceInstance", 200, true)?;
    let input = content(p)?;
    let export: Value = serde_json::from_str(input).map_err(|_| "CNVP 文件不是有效 JSON")?;
    keys(
        &export,
        &[
            "schemaVersion",
            "brandName",
            "products",
            "questions",
            "drafts",
            "publications",
            "observations",
        ],
    )?;
    if export["schemaVersion"] != 1 {
        return Err("不支持的 CNVP 导出版本".into());
    }
    if export["brandName"] != br["name"] {
        return Err("迁移文件品牌与当前品牌不一致".into());
    }
    let products = rows(&export, "products")?;
    if products.len() > 1000 {
        return Err("一次最多迁移1000个商品".into());
    }
    let questions = rows(&export, "questions")?;
    let drafts = rows(&export, "drafts")?;
    let publications = rows(&export, "publications")?;
    let observations = rows(&export, "observations")?;
    let mut seen = HashSet::new();
    for (kind, rs) in [
        ("product", products),
        ("question", questions),
        ("draft", drafts),
        ("publication", publications),
        ("observation", observations),
    ] {
        for row in rs {
            let old = old_id(row, "id")?;
            if !seen.insert((kind, old)) {
                return Err("迁移文件存在重复旧标识，整批未导入".into());
            }
            ownership(row, &br["name"])?;
            validate_legacy(kind, row)?;
        }
    }
    let mut created = 0;
    let mut skipped = 0;
    for row in products {
        keys(
            row,
            &[
                "id",
                "brandName",
                "name",
                "sku",
                "category",
                "officialUrl",
                "sourceText",
                "privateNotes",
                "facts",
                "revision",
                "confirmedAt",
                "confirmationSnapshot",
                "updatedAt",
                "ownerLabel",
                "reviewerLabel",
            ],
        )?;
        let old = old_id(row, "id")?;
        let rev = number(row, "revision")?;
        legacy_date(row, "confirmedAt")?;
        legacy_date(row, "updatedAt")?;
        let mut data = row.clone();
        data.as_object_mut().ok_or("迁移商品格式错误")?.remove("id");
        // Reuse normal field validators even when this record will be skipped.
        text(&data, "name", 200, true)?;
        let sku = text(&data, "sku", 100, true)?;
        text(&data, "category", 200, false)?;
        let u = text(&data, "officialUrl", 2000, false)?;
        web_url(&u, false)?;
        raw(&data, "sourceText", 100000, false)?;
        raw(&data, "privateNotes", 30000, false)?;
        facts(&data)?;
        for f in data["facts"].as_array().ok_or("商品事实格式错误")? {
            keys(
                f,
                &["id", "label", "value", "source", "visibility", "validUntil"],
            )?;
        }
        if find_mapping(c, &src, "product", &old, b)?.is_some() {
            skipped += 1;
            continue;
        }
        let collision = c
            .query_row(
                "SELECT id FROM m_products WHERE brand_id=?1 AND sku=?2",
                params![b, sku],
                |r| r.get::<_, String>(0),
            )
            .optional()
            .map_err(db_error)?;
        // A SKU collision skips the entire historical subtree. The existing product is never overwritten.
        if let Some(existing) = collision {
            mark_skip(c, &src, "product", &old, b)?;
            add_mapping(c, &src, "product", &old, b, &existing, row)?;
            skipped += 1;
            continue;
        }
        let mut saved = save_product(c, b, &data)?;
        saved["revision"] = json!(rev);
        saved["confirmedAt"] = Value::Null;
        if row.get("updatedAt").is_some_and(|v| !v.is_null()) {
            saved["updatedAt"] = json!(date_time(row, "updatedAt")?);
        }
        update(c, "m_products", &saved)?;
        add_mapping(
            c,
            &src,
            "product",
            &old,
            b,
            saved["id"].as_str().ok_or("商品标识错误")?,
            row,
        )?;
        created += 1;
    }
    for row in questions {
        keys(
            row,
            &[
                "id",
                "brandName",
                "productId",
                "text",
                "intent",
                "origin",
                "source",
                "branded",
                "confirmed",
                "ownerLabel",
                "reviewerLabel",
            ],
        )?;
        let old = old_id(row, "id")?;
        if was_skipped(c, &src, "product", &old_id(row, "productId")?, b)? {
            mark_skip(c, &src, "question", &old, b)?;
            skipped += 1;
            continue;
        }
        let pid = map_required(c, &src, "product", &old_id(row, "productId")?, b)?;
        let mut data = row.clone();
        data["productId"] = json!(pid); // checks must execute for duplicates too
        text(&data, "text", 1000, true)?;
        choice(
            &data,
            "intent",
            &["selection", "comparison", "usage", "after_sales"],
        )?;
        choice(&data, "origin", &["customer", "suggestion"])?;
        text(&data, "source", 2000, true)?;
        boolean(&data, "branded")?;
        boolean(&data, "confirmed")?;
        if find_mapping(c, &src, "question", &old, b)?.is_some() {
            skipped += 1;
            continue;
        }
        let count: i64 = c
            .query_row(
                "SELECT count(*) FROM m_questions WHERE product_id=?1",
                [&pid],
                |r| r.get(0),
            )
            .map_err(db_error)?;
        let saved = save_question(c, b, &data)?;
        add_mapping(
            c,
            &src,
            "question",
            &old,
            b,
            saved["id"].as_str().ok_or("问题标识错误")?,
            row,
        )?;
        let after: i64 = c
            .query_row(
                "SELECT count(*) FROM m_questions WHERE product_id=?1",
                [&pid],
                |r| r.get(0),
            )
            .map_err(db_error)?;
        if after > count {
            created += 1
        } else {
            skipped += 1
        }
    }
    for row in drafts {
        keys(
            row,
            &[
                "id",
                "brandName",
                "productId",
                "questionId",
                "kind",
                "title",
                "body",
                "sourceRevision",
                "sourceSnapshot",
                "reviewSnapshot",
                "revision",
                "generationMode",
                "reviewedAt",
                "createdAt",
                "ownerLabel",
                "reviewerLabel",
            ],
        )?;
        let old = old_id(row, "id")?;
        if was_skipped(c, &src, "product", &old_id(row, "productId")?, b)?
            || (row.get("questionId").is_some_and(|v| !v.is_null())
                && was_skipped(c, &src, "question", &old_id(row, "questionId")?, b)?)
        {
            mark_skip(c, &src, "draft", &old, b)?;
            skipped += 1;
            continue;
        }
        let pid = map_required(c, &src, "product", &old_id(row, "productId")?, b)?;
        let qid = if row.get("questionId").is_none_or(Value::is_null) {
            None
        } else {
            Some(map_required(
                c,
                &src,
                "question",
                &old_id(row, "questionId")?,
                b,
            )?)
        };
        if let Some(q) = &qid {
            let question = load(c, "m_questions", b, q)?;
            if question["productId"] != pid {
                return Err("迁移稿件的问题不属于同一商品".into());
            }
        }
        let kind = choice(row, "kind", &["description", "faq", "guide", "comparison"])?;
        let title = text(row, "title", 300, true)?;
        let body = raw(row, "body", 30000, true)?;
        let source_rev = number(row, "sourceRevision")?;
        let rev = number(row, "revision")?;
        let mode = choice(row, "generationMode", &["local", "ai"])?;
        let created_at = date_time(row, "createdAt")?;
        legacy_date(row, "reviewedAt")?;
        if find_mapping(c, &src, "draft", &old, b)?.is_some() {
            skipped += 1;
            continue;
        }
        let ctx = json!({"brandId":b,"productId":pid,"questionId":qid,"kind":kind,"title":title,"sourceRevision":source_rev});
        let mut saved = insert_draft(c, &ctx, &body, &mode)?;
        saved["revision"] = json!(rev);
        saved["createdAt"] = json!(created_at);
        saved["reviewedAt"] = Value::Null;
        update(c, "m_drafts", &saved)?;
        add_mapping(
            c,
            &src,
            "draft",
            &old,
            b,
            saved["id"].as_str().ok_or("稿件标识错误")?,
            row,
        )?;
        created += 1;
    }
    for row in publications {
        keys(
            row,
            &[
                "id",
                "brandName",
                "draftId",
                "draftRevision",
                "channel",
                "url",
                "publishedAt",
                "note",
                "contentSnapshot",
                "contentSnapshotSha256",
                "ownerLabel",
                "reviewerLabel",
            ],
        )?;
        let old = old_id(row, "id")?;
        if was_skipped(c, &src, "draft", &old_id(row, "draftId")?, b)? {
            mark_skip(c, &src, "publication", &old, b)?;
            skipped += 1;
            continue;
        }
        let did = map_required(c, &src, "draft", &old_id(row, "draftId")?, b)?;
        let rev = number(row, "draftRevision")?;
        let draft = load(c, "m_drafts", b, &did)?;
        if rev > number(&draft, "revision")? {
            return Err("发布版本大于关联稿件版本".into());
        }
        let channel = text(row, "channel", 100, true)?;
        let url = text(row, "url", 2000, true)?;
        web_url(&url, true)?;
        let time = date_time(row, "publishedAt")?;
        let note = raw(row, "note", 5000, false)?;
        let snapshot = row["contentSnapshot"]
            .as_str()
            .filter(|s| !s.trim().is_empty() && s.chars().count() <= 31000 && !s.contains('\0'))
            .ok_or("发布快照为空或超限")?;
        if let Some(hash) = row.get("contentSnapshotSha256") {
            if hash.as_str() != Some(hex::encode(Sha256::digest(snapshot.as_bytes())).as_str()) {
                return Err("发布快照校验不一致，整批未导入".into());
            }
        }
        if find_mapping(c, &src, "publication", &old, b)?.is_some() {
            skipped += 1;
            continue;
        }
        let key = fingerprint(&json!([b, did, rev, channel, url, time]));
        let existing = c
            .query_row(
                "SELECT id FROM m_publications WHERE dedup=?1",
                [&key],
                |r| r.get::<_, String>(0),
            )
            .optional()
            .map_err(db_error)?;
        if let Some(existing) = existing {
            let existing_data = load(c, "m_publications", b, &existing)?;
            if existing_data["contentSnapshot"] != snapshot {
                return Err("重复发布记录的快照不一致，整批未导入".into());
            }
            add_mapping(c, &src, "publication", &old, b, &existing, row)?;
            skipped += 1;
            continue;
        }
        let saved = json!({"id":id(),"brandId":b,"draftId":did,"draftRevision":rev,"channel":channel,"url":url,"publishedAt":time,"note":note,"contentSnapshot":snapshot});
        c.execute(
            "INSERT INTO m_publications(id,brand_id,draft_id,dedup,data)VALUES(?1,?2,?3,?4,?5)",
            params![saved["id"].as_str(), b, did, key, saved.to_string()],
        )
        .map_err(db_error)?;
        add_mapping(
            c,
            &src,
            "publication",
            &old,
            b,
            saved["id"].as_str().ok_or("发布标识错误")?,
            row,
        )?;
        created += 1;
    }
    for row in observations {
        keys(
            row,
            &[
                "id",
                "brandName",
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
                "ownerLabel",
                "reviewerLabel",
            ],
        )?;
        let old = old_id(row, "id")?;
        if was_skipped(c, &src, "product", &old_id(row, "productId")?, b)?
            || was_skipped(c, &src, "question", &old_id(row, "questionId")?, b)?
        {
            mark_skip(c, &src, "observation", &old, b)?;
            skipped += 1;
            continue;
        }
        let pid = map_required(c, &src, "product", &old_id(row, "productId")?, b)?;
        let qid = map_required(c, &src, "question", &old_id(row, "questionId")?, b)?;
        let q = load(c, "m_questions", b, &qid)?;
        if q["productId"] != pid {
            return Err("迁移观察的问题不属于同一商品".into());
        }
        let mut data = row.clone();
        data["productId"] = json!(pid);
        data["questionId"] = json!(qid);
        // An archived observation retains its historical query and branded cohort, even if a question changed later.
        let snapshot = text(row, "querySnapshot", 1000, true)?;
        let branded = boolean(row, "branded")?;
        // Validation is independent of present question confirmation for historical observations.
        let mut saved = observation_value_with_history(c, b, &data, true)?;
        saved["querySnapshot"] = json!(snapshot);
        saved["branded"] = json!(branded);
        if find_mapping(c, &src, "observation", &old, b)?.is_some() {
            skipped += 1;
            continue;
        }
        let hash = fingerprint(&saved);
        let existing = c
            .query_row(
                "SELECT id FROM m_observations WHERE dedup=?1",
                [&hash],
                |r| r.get::<_, String>(0),
            )
            .optional()
            .map_err(db_error)?;
        if let Some(existing) = existing {
            add_mapping(c, &src, "observation", &old, b, &existing, row)?;
            skipped += 1;
            continue;
        }
        saved["id"] = json!(id());
        c.execute("INSERT INTO m_observations(id,brand_id,product_id,question_id,dedup,data)VALUES(?1,?2,?3,?4,?5,?6)",params![saved["id"].as_str(),b,pid,qid,hash,saved.to_string()]).map_err(db_error)?;
        add_mapping(
            c,
            &src,
            "observation",
            &old,
            b,
            saved["id"].as_str().ok_or("观察标识错误")?,
            row,
        )?;
        created += 1;
    }
    Ok(json!({"created":created,"skipped":skipped}))
}
