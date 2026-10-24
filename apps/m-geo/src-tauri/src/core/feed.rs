//! Deterministic local feed preparation. Hashes are integrity checks, not signatures.
use super::*;
const MAX_INPUT: usize = 2 * 1024 * 1024;
const PROFILE: &[&str] = &["mgeo-catalog-v1", "google-merchant-basic-v1"];
const CATALOG: &[(&str, usize)] = &[
    ("brand", 70),
    ("kind", 20),
    ("groupId", 50),
    ("groupTitle", 150),
    ("color", 100),
    ("size", 100),
    ("material", 200),
    ("pattern", 100),
    ("gtin", 14),
    ("mpn", 70),
    ("condition", 20),
    ("titleOrigin", 10),
    ("productClass", 30),
];
// Exported JSON is imported as a string inside another JSON request. Budget
// the pretty operation-export form (which also bounds the compact feed form),
// plus the maximum supported selection, rather than only the decoded file.
// guid() bounds identifiers to 80 characters, number() uses positive i64,
// selection is capped at 200, and profile comes from the fixed PROFILE list.
// Include the native export operation member; no global input limit is raised.
pub(super) fn validate_export_roundtrip(manifest: &Value) -> Result<()> {
    let content = serde_json::to_string_pretty(manifest).map_err(|_| "无法生成本地清单")?;
    let identifier = "0".repeat(80);
    let item = json!({"productId":identifier,"expectedRevision":i64::MAX,"draftId":identifier,"expectedDraftRevision":i64::MAX});
    let envelope = json!({
        "brandId":identifier,
        "profile":PROFILE.iter().max_by_key(|profile| profile.len()).unwrap(),
        "items":vec![item; 200],
        "baseline":content,
        "operation":"compare"
    });
    if envelope.to_string().len() > MAX_INPUT {
        return Err("Feed 清单重新导入请求超过2MiB，请减少商品数量后分批导出".into());
    }
    Ok(())
}
fn issue(field: &str, code: &str, message: &str) -> Value {
    json!({"field":field,"code":code,"message":message})
}
fn file(path: &str, content: String) -> Value {
    json!({"path":path,"content":content})
}
fn xml_text(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn xml_legal(s: &str) -> bool {
    s.chars().all(|c| {
        matches!(c, '\u{9}' | '\u{a}' | '\u{d}')
            || ('\u{20}'..='\u{d7ff}').contains(&c)
            || ('\u{e000}'..='\u{fffd}').contains(&c)
            || ('\u{10000}'..='\u{10ffff}').contains(&c)
    })
}
fn exact(v: &Value, keys: &[&str]) -> Result<()> {
    let o = v.as_object().ok_or("Feed 格式无效")?;
    if o.len() != keys.len() || keys.iter().any(|k| !o.contains_key(*k)) {
        return Err("Feed 字段无效".into());
    }
    Ok(())
}
fn bounded(v: &Value, k: &str, max: usize, required: bool) -> Result<String> {
    if !v[k].is_string() {
        return Err("Feed 字段类型无效".into());
    }
    let s = raw(v, k, max, required)?;
    if !xml_legal(&s) {
        return Err("Feed 含不支持的字符".into());
    }
    Ok(s)
}
fn check_catalog(v: &Value) -> Result<()> {
    let o = v.as_object().ok_or("商品身份格式无效")?;
    for (k, _) in o {
        let max = CATALOG
            .iter()
            .find(|(n, _)| *n == k)
            .ok_or("不支持的身份字段")?
            .1;
        bounded(v, k, max, true)?;
    }
    for (k, allowed) in [
        ("kind", &["standalone", "variant"][..]),
        ("condition", &["new", "refurbished", "used"][..]),
        ("titleOrigin", &["human", "ai"][..]),
        (
            "productClass",
            &["physical_non_apparel", "apparel", "digital", "other"][..],
        ),
    ] {
        if let Some(s) = v.get(k) {
            if !allowed.contains(&s.as_str().unwrap_or("")) {
                return Err("商品身份枚举无效".into());
            }
        }
    }
    if let Some(s) = v["gtin"].as_str() {
        if ![8, 12, 13, 14].contains(&s.len()) || !s.bytes().all(|b| b.is_ascii_digit()) {
            return Err("商品标识无效".into());
        }
        let digits: Vec<u32> = s.bytes().map(|b| (b - b'0') as u32).collect();
        let sum: u32 = digits[..digits.len() - 1]
            .iter()
            .rev()
            .enumerate()
            .map(|(i, d)| d * if i % 2 == 0 { 3 } else { 1 })
            .sum();
        if (10 - sum % 10) % 10 != digits[digits.len() - 1] {
            return Err("商品标识无效".into());
        }
    }
    Ok(())
}
fn check_data(d: &Value, sku: &str, google: bool) -> Result<()> {
    exact(
        d,
        &[
            "id",
            "title",
            "description",
            "link",
            "imageLink",
            "price",
            "currency",
            "availability",
            "catalog",
        ],
    )?;
    if bounded(d, "id", if google { 50 } else { 100 }, true)? != sku {
        return Err("商品标识不匹配".into());
    }
    bounded(d, "title", if google { 150 } else { 200 }, true)?;
    bounded(d, "description", if google { 5000 } else { 30000 }, true)?;
    for k in ["link", "imageLink"] {
        web_url(&bounded(d, k, 3000, true)?, true)?;
    }
    for (k, id) in [
        ("price", "price"),
        ("currency", "currency"),
        ("availability", "availability"),
    ] {
        let s = bounded(d, k, 30, true)?;
        commerce::value(&json!({"id":format!("commerce:{id}"),"value":s}))?;
    }
    if d["price"]
        .as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .is_none_or(|n| n <= 0.0)
    {
        return Err("价格必须为正数".into());
    }
    if d["availability"] == "Unknown" {
        return Err("库存未知".into());
    }
    check_catalog(&d["catalog"])?;
    let cat = &d["catalog"];
    for key in ["brand", "kind", "condition", "titleOrigin", "productClass"] {
        bounded(cat, key, 200, true)?;
    }
    match cat["kind"].as_str() {
        Some("standalone") => {
            if [
                "groupId",
                "groupTitle",
                "color",
                "size",
                "material",
                "pattern",
            ]
            .iter()
            .any(|k| cat.get(*k).is_some())
            {
                return Err("单规格身份矛盾".into());
            }
        }
        Some("variant") => {
            bounded(cat, "groupId", 50, true)?;
            bounded(cat, "groupTitle", 150, true)?;
            if !["color", "size", "material", "pattern"]
                .iter()
                .any(|k| cat.get(*k).is_some())
            {
                return Err("变体维度缺失".into());
            }
        }
        _ => return Err("商品身份未知".into()),
    };
    if google {
        for (k, want) in [
            ("kind", "standalone"),
            ("condition", "new"),
            ("productClass", "physical_non_apparel"),
        ] {
            if cat[k] != want {
                return Err("超出 Google basic 支持范围".into());
            }
        }
        bounded(cat, "brand", 70, true)?;
        if !["human", "ai"].contains(&cat["titleOrigin"].as_str().unwrap_or(""))
            || (!cat["gtin"].is_string() && !cat["mpn"].is_string())
            || !["InStock", "OutOfStock"].contains(&d["availability"].as_str().unwrap_or(""))
        {
            return Err("Google basic 资料不完整".into());
        }
    }
    Ok(())
}
fn fact_usable(f: &Value) -> bool {
    f["visibility"] == "public"
        && !f["source"].as_str().unwrap_or("").trim().is_empty()
        && f["validUntil"]
            .as_str()
            .map(|s| {
                NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok_and(|d| d >= Utc::now().date_naive())
            })
            .unwrap_or(f["validUntil"].is_null())
        && commerce::freshness(f).is_ok()
}
fn xml_field(k: &str, v: &str, ai: bool) -> String {
    if ai {
        format!("<g:structured_{k}><g:digital_source_type>trained_algorithmic_media</g:digital_source_type><g:content>{}</g:content></g:structured_{k}>",xml_text(v))
    } else {
        format!("<g:{k}>{}</g:{k}>", xml_text(v))
    }
}
pub(super) fn prepare(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    if p.to_string().len() > MAX_INPUT {
        return Err("Feed 请求超过2MB".into());
    }
    exact(p, &["brandId", "profile", "items"])?;
    if guid(p, "brandId")? != b {
        return Err("Feed 品牌不匹配".into());
    }
    brand(c, b)?;
    let profile = choice(p, "profile", PROFILE)?;
    let google = profile == PROFILE[1];
    let selected = p["items"].as_array().ok_or("请选择商品和已审核介绍")?;
    if selected.is_empty() || selected.len() > 200 {
        return Err("请选择1至200件商品".into());
    }
    let mut ids = HashSet::new();
    let mut loaded = vec![];
    for s in selected {
        exact(
            s,
            &[
                "productId",
                "expectedRevision",
                "draftId",
                "expectedDraftRevision",
            ],
        )?;
        let pid = guid(s, "productId")?;
        if !ids.insert(pid.clone()) {
            return Err("不能重复选择商品".into());
        }
        let prod = load(c, "m_products", b, &pid)?;
        revision(&prod, s)?;
        let did = guid(s, "draftId")?;
        let d = load(c, "m_drafts", b, &did)?;
        if d["productId"] != pid
            || d["brandId"] != b
            || prod["brandId"] != b
            || d["sourceRevision"] != prod["revision"]
            || number(s, "expectedDraftRevision")? != number(&d, "revision")?
        {
            return Err("稿件归属或版本已变化，请刷新后重选".into());
        }
        if d["kind"] != "description" {
            return Err("Feed 需要商品介绍稿件".into());
        }
        if let Some(qid) = d["questionId"].as_str() {
            checked_question(c, b, &pid, qid)?;
        }
        loaded.push((pid, prod, d));
    }
    loaded.sort_by(|a, b| a.0.cmp(&b.0));
    let inspection = catalog::inspect(
        c,
        b,
        &json!({"productIds":loaded.iter().map(|(pid,_,_)|pid).collect::<Vec<_>>()}),
    )?;
    let mut manifest_rows = vec![];
    let mut report = vec![];
    let mut xml_items = String::new();
    let mut skus = HashSet::new();
    for (pid, prod, d) in loaded {
        let mut issues = vec![];
        let inspected = inspection["rows"]
            .as_array()
            .and_then(|rows| rows.iter().find(|r| r["productId"] == pid))
            .ok_or("商品身份检查不完整")?;
        if let Some(arr) = inspected["issues"].as_array() {
            issues.extend(arr.iter().map(|i| {
                let field = i["field"]
                    .as_str()
                    .filter(|s| s.len() < 100)
                    .unwrap_or("catalog");
                issue(field, "catalog_incomplete", "商品身份或有效期资料待核对")
            }));
        } else {
            return Err("商品身份检查不完整".into());
        }
        let sku = text(&prod, "sku", 100, false)?;
        if sku.is_empty() || !skus.insert(sku.clone()) {
            issues.push(issue("sku", "missing_or_duplicate", "商品 SKU 缺失或重复"));
        }
        if prod["confirmedAt"].as_str().is_none() {
            issues.push(issue("confirmation", "unconfirmed", "请确认当前商品资料"));
        }
        if d["reviewedAt"].as_str().is_none() {
            issues.push(issue("description", "unreviewed", "请审核当前商品介绍"));
        }
        if !["local", "ai"].contains(&d["generationMode"].as_str().unwrap_or("")) {
            issues.push(issue("description", "unknown_origin", "介绍来源未知"));
        }
        let facts = prod["facts"].as_array().ok_or("商品事实格式错误")?;
        if check_public(&prod).is_err() {
            issues.push(issue(
                "facts",
                "not_current",
                "公开事实缺少依据或有效期，请更新并重新确认",
            ));
        }
        let mut validity = vec![];
        let mut commerce_values = serde_json::Map::new();
        for f in facts {
            if fact_usable(f) {
                if let Some(k) = f["id"].as_str().and_then(|s| s.strip_prefix("commerce:")) {
                    if ["purchase", "image", "price", "currency", "availability"].contains(&k) {
                        commerce_values.insert(k.to_owned(), f["value"].clone());
                    }
                }
                if let Some(expiry) = f["validUntil"].as_str() {
                    validity.push(json!({"field":f["id"],"validUntil":expiry}));
                }
            }
        }
        validity.sort_by(|a, b| a["field"].as_str().cmp(&b["field"].as_str()));
        let description = d["body"].as_str().unwrap_or("");
        let title = prod["name"].as_str().unwrap_or("");
        // Explicitly reviewed draft body is the public prose boundary; raw source/private metadata is never added.
        let cv = Value::Object(commerce_values);
        let data = json!({"id":sku,"title":title,"description":description,"link":cv["purchase"],"imageLink":cv["image"],"price":cv["price"],"currency":cv["currency"],"availability":cv["availability"],"catalog":inspected["catalog"]});
        for (field, key) in [
            ("purchase", "link"),
            ("image", "imageLink"),
            ("price", "price"),
            ("currency", "currency"),
            ("availability", "availability"),
        ] {
            if !data[key].is_string() {
                issues.push(issue(
                    field,
                    "missing_public_value",
                    "缺少当前有依据的公开购买资料",
                ));
            }
        }
        if check_data(&data, &sku, google).is_err() {
            issues.push(issue("profile","unsupported_or_invalid","字段、身份、字符或长度不满足所选本地格式；请核对规格、成色、类别、来源、标识符与库存"));
        }
        let ready = issues.is_empty();
        let mut row = json!({"productId":pid,"sku":sku,"productRevision":prod["revision"],"draftId":d["id"],"draftRevision":d["revision"],"localStatus":if ready{"ready"}else{"blocked"},"data":if ready{data.clone()}else{json!({})},"validity":validity});
        row["rowHash"] = json!(fingerprint(&row));
        if ready && google {
            let cat = &data["catalog"];
            xml_items.push_str("<item>");
            for (k, v) in [
                ("id", sku.as_str()),
                ("link", data["link"].as_str().unwrap()),
                ("image_link", data["imageLink"].as_str().unwrap()),
                (
                    "price",
                    &format!(
                        "{} {}",
                        data["price"].as_str().unwrap(),
                        data["currency"].as_str().unwrap()
                    ),
                ),
                (
                    "availability",
                    if data["availability"] == "InStock" {
                        "in_stock"
                    } else {
                        "out_of_stock"
                    },
                ),
                ("condition", "new"),
                ("brand", cat["brand"].as_str().unwrap()),
            ] {
                xml_items.push_str(&xml_field(k, v, false));
            }
            xml_items.push_str(&xml_field("title", title, cat["titleOrigin"] == "ai"));
            // generationMode is immutable in supported draft actions; ID/revision binds it. Never render XML from imported manifests.
            xml_items.push_str(&xml_field(
                "description",
                description,
                d["generationMode"] == "ai",
            ));
            for k in ["gtin", "mpn"] {
                if let Some(v) = cat[k].as_str() {
                    xml_items.push_str(&xml_field(k, v, false));
                }
            }
            xml_items.push_str("</item>");
        }
        manifest_rows.push(row);
        report.push(json!({"productId":pid,"sku":sku,"localStatus":if ready{"ready"}else{"blocked"},"issues":issues}));
    }
    let ready = report
        .iter()
        .filter(|r| r["localStatus"] == "ready")
        .count();
    let summary = json!({"selected":report.len(),"ready":ready,"blocked":report.len()-ready});
    let mut manifest = json!({"format":"m-geo-feed-v1","schemaVersion":1,"profile":profile,"brandId":b,"mode":"local_draft","submissionStatus":"not_submitted","selectionScope":"selected_products_not_full_merchant_catalog","items":manifest_rows});
    let package_id = fingerprint(&manifest);
    manifest["packageId"] = json!(package_id);
    validate_export_roundtrip(&manifest)?;
    let external_checks = json!([
        "商家账号与目标地区、品类资格尚未验证",
        "商品真实标识符分配、品牌与内容图片权利尚未验证",
        "购买页、域名、图片可访问性与实时价格库存尚未验证",
        "运输、税费、退货及地区品类附加字段尚未验证",
        "仅为选中商品的本地草稿，不能作为完整商家目录替换或删除授权",
        "未上传、未提交；本地检查不代表接纳、收录或推荐"
    ]);
    let mut files=vec![file("feed-manifest.json",manifest.to_string()),file("feed-report.json",json!({"mode":"local_draft","submissionStatus":"not_submitted","summary":summary,"rows":report,"externalChecks":external_checks}).to_string()),file("field-mapping.txt","SKU → id；商品名称 → title / structured_title；当前已审核介绍 → description / structured_description；公开 purchase → link；image → image_link；price + currency → price；InStock / OutOfStock → in_stock / out_of_stock。AI 字段仅使用 structured_*。商品身份仅保留白名单字段。有效期为本地检查依据，不自动映射平台下架日期。\n".into()),file("official-sources.txt","specReviewedAt=2026-10-23\nhttps://support.google.com/merchants/answer/7052112?hl=en\nhttps://support.google.com/merchants/answer/14987622?hl=en\nhttps://support.google.com/merchants/answer/6324415?hl=en\nhttps://support.google.com/merchants/answer/6324468?hl=en\nGoogle basic 仅支持明确单规格、新实物非服装商品；官方规则会变化，本地映射不代表完整合规。\n".into()),file("external-requirements.json",external_checks.to_string())];
    if ready == report.len() {
        if google {
            let link = manifest["items"][0]["data"]["link"].as_str().unwrap();
            files.push(file("google-merchant-basic-draft.xml",format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><rss version=\"2.0\" xmlns:g=\"http://base.google.com/ns/1.0\"><channel><title>M-GEO local draft</title><link>{}</link><description>Selected products only; not submitted; not a full merchant catalog.</description>{xml_items}</channel></rss>",xml_text(link))));
        } else {
            files.push(file("mgeo-catalog-draft.json",json!({"format":"mgeo-catalog-v1","mode":"local_draft","submissionStatus":"not_submitted","products":manifest["items"].as_array().unwrap().iter().map(|r|&r["data"]).collect::<Vec<_>>()}).to_string()));
        }
    }
    if files
        .iter()
        .map(|f| f["content"].as_str().unwrap().len())
        .sum::<usize>()
        > 16 * 1024 * 1024
    {
        return Err("Feed 文件包超过16MB".into());
    }
    Ok(
        json!({"manifest":manifest,"packageId":package_id,"summary":summary,"rows":report,"externalChecks":external_checks,"files":files}),
    )
}
pub(super) fn validate_manifest(v: &Value, b: &str) -> Result<()> {
    validate_inner(v, b).map_err(|_| "Feed 清单格式、范围或完整性校验失败".into())
}
fn validate_inner(v: &Value, b: &str) -> Result<()> {
    if v.to_string().len() > MAX_INPUT {
        return Err("大小".into());
    }
    exact(
        v,
        &[
            "format",
            "schemaVersion",
            "profile",
            "brandId",
            "mode",
            "submissionStatus",
            "selectionScope",
            "items",
            "packageId",
        ],
    )?;
    if v["format"] != "m-geo-feed-v1"
        || v["schemaVersion"] != 1
        || v["mode"] != "local_draft"
        || v["submissionStatus"] != "not_submitted"
        || v["selectionScope"] != "selected_products_not_full_merchant_catalog"
        || guid(v, "brandId")? != b
    {
        return Err("信封".into());
    }
    let profile = choice(v, "profile", PROFILE)?;
    let rows = v["items"].as_array().ok_or("商品列表")?;
    if rows.is_empty() || rows.len() > 200 {
        return Err("数量".into());
    }
    let mut ids = HashSet::new();
    let mut draft_ids = HashSet::new();
    let mut skus = HashSet::new();
    let mut last = String::new();
    for row in rows {
        exact(
            row,
            &[
                "productId",
                "sku",
                "productRevision",
                "draftId",
                "draftRevision",
                "localStatus",
                "data",
                "validity",
                "rowHash",
            ],
        )?;
        let pid = guid(row, "productId")?;
        if !draft_ids.insert(guid(row, "draftId")?) {
            return Err("重复稿件".into());
        }
        number(row, "productRevision")?;
        number(row, "draftRevision")?;
        if !ids.insert(pid.clone()) || pid <= last {
            return Err("标识顺序".into());
        }
        last = pid;
        let sku = bounded(row, "sku", 100, false)?;
        if !skus.insert(sku.clone()) {
            return Err("重复SKU".into());
        }
        let status = choice(row, "localStatus", &["ready", "blocked"])?;
        if status == "ready" {
            check_data(&row["data"], &sku, profile == PROFILE[1])?;
        } else {
            exact(&row["data"], &[])?;
        }
        let validity = row["validity"].as_array().ok_or("有效期")?;
        if validity.len() > 100 {
            return Err("有效期数量".into());
        }
        let mut fields = HashSet::new();
        let mut prior = String::new();
        for f in validity {
            exact(f, &["field", "validUntil"])?;
            let field = bounded(f, "field", 100, true)?;
            if !fields.insert(field.clone()) || field <= prior {
                return Err("有效期字段".into());
            }
            prior = field;
            let date = bounded(f, "validUntil", 10, true)?;
            if date.len() != 10 || NaiveDate::parse_from_str(&date, "%Y-%m-%d").is_err() {
                return Err("有效期格式".into());
            }
        }
        if status == "ready"
            && !["commerce:price", "commerce:availability"]
                .iter()
                .all(|f| fields.contains(*f))
        {
            return Err("动态字段缺少有效期".into());
        }
        let hash = bounded(row, "rowHash", 64, true)?;
        let mut unsigned = row.clone();
        unsigned.as_object_mut().unwrap().remove("rowHash");
        if hash != fingerprint(&unsigned) {
            return Err("行摘要".into());
        }
    }
    let hash = bounded(v, "packageId", 64, true)?;
    let mut unsigned = v.clone();
    unsigned.as_object_mut().unwrap().remove("packageId");
    if hash != fingerprint(&unsigned) {
        return Err("包摘要".into());
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Connection, String, Value) {
        let mut c = Connection::open_in_memory().unwrap();
        initialize(&mut c).unwrap();
        let b = handle(&mut c, "brand.create", json!({"name":"虚构品牌"})).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let facts:Vec<Value>=[("catalog:kind","standalone"),("catalog:condition","new"),("catalog:productClass","physical_non_apparel"),("catalog:titleOrigin","human"),("catalog:brand","虚构制造品牌"),("catalog:mpn","FICTIONAL-1"),("commerce:price","29.90"),("commerce:currency","CNY"),("commerce:availability","InStock"),("commerce:purchase","https://example.com/item"),("commerce:image","https://example.com/image.png")].iter().map(|(k,v)|json!({"id":k,"label":k,"value":v,"source":"SOURCE_SECRET","visibility":"public","validUntil":"2099-01-01"})).collect();
        let prod=handle(&mut c,"product.save",json!({"brandId":b,"name":"虚构 & <台灯>","sku":"FICTIONAL-1","category":"灯","officialUrl":"","sourceText":"RAW_SECRET","privateNotes":"NOTE_SECRET","facts":facts})).unwrap();
        handle(
            &mut c,
            "product.confirm",
            json!({"brandId":b,"id":prod["id"],"expectedRevision":1}),
        )
        .unwrap();
        let d = handle(
            &mut c,
            "draft.generateLocal",
            json!({"brandId":b,"productId":prod["id"],"kind":"description"}),
        )
        .unwrap();
        let mut d = load(&c, "m_drafts", &b, d["id"].as_str().unwrap()).unwrap();
        d["body"] = json!("虚构说明 & <文本>");
        update(&c, "m_drafts", &d).unwrap();
        let d = handle(
            &mut c,
            "draft.review",
            json!({"brandId":b,"id":d["id"],"expectedRevision":1}),
        )
        .unwrap();
        let req = json!({"brandId":b,"profile":"google-merchant-basic-v1","items":[{"productId":prod["id"],"expectedRevision":1,"draftId":d["id"],"expectedDraftRevision":d["revision"]}]});
        (c, b, req)
    }
    #[test]
    fn feed_current_review_and_scope_gate() {
        let (c, b, p) = fixture();
        let r = prepare(&c, &b, &p).unwrap();
        assert_eq!(r["summary"]["ready"], 1);
        let mut bad = p.clone();
        bad["items"][0]["expectedRevision"] = json!(9);
        assert!(prepare(&c, &b, &bad).is_err());
        assert!(prepare(&c, &id(), &p).is_err());
        let mut d = load(
            &c,
            "m_drafts",
            &b,
            p["items"][0]["draftId"].as_str().unwrap(),
        )
        .unwrap();
        d["reviewedAt"] = Value::Null;
        update(&c, "m_drafts", &d).unwrap();
        assert_eq!(prepare(&c, &b, &p).unwrap()["summary"]["blocked"], 1);
    }
    #[test]
    fn feed_private_and_expired_block_without_leak() {
        let (c, b, p) = fixture();
        let mut prod = load(
            &c,
            "m_products",
            &b,
            p["items"][0]["productId"].as_str().unwrap(),
        )
        .unwrap();
        prod["facts"][5]["visibility"] = json!("private");
        prod["facts"][5]["value"] = json!("PRIVATE_SECRET");
        prod["facts"][6]["validUntil"] = json!("2000-01-01");
        update(&c, "m_products", &prod).unwrap();
        let r = prepare(&c, &b, &p).unwrap();
        assert_eq!(r["summary"]["blocked"], 1);
        assert_eq!(r["manifest"]["items"][0]["data"], json!({}));
        for secret in [
            "PRIVATE_SECRET",
            "RAW_SECRET",
            "NOTE_SECRET",
            "SOURCE_SECRET",
        ] {
            assert!(!r.to_string().contains(secret));
        }
        assert!(!r["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["path"] == "google-merchant-basic-draft.xml"));
    }
    #[test]
    fn feed_xml_ai_escape_and_control() {
        let (c, b, p) = fixture();
        let mut prod = load(
            &c,
            "m_products",
            &b,
            p["items"][0]["productId"].as_str().unwrap(),
        )
        .unwrap();
        prod["facts"][3]["value"] = json!("ai");
        update(&c, "m_products", &prod).unwrap();
        let mut d = load(
            &c,
            "m_drafts",
            &b,
            p["items"][0]["draftId"].as_str().unwrap(),
        )
        .unwrap();
        d["generationMode"] = json!("ai");
        update(&c, "m_drafts", &d).unwrap();
        let r = prepare(&c, &b, &p).unwrap();
        let xml = r["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["path"] == "google-merchant-basic-draft.xml")
            .unwrap()["content"]
            .as_str()
            .unwrap();
        assert!(xml.contains("<g:structured_title>"));
        assert!(xml.contains("<g:structured_description>"));
        assert!(xml.contains("trained_algorithmic_media"));
        assert!(xml.contains("&amp; &lt;"));
        assert!(!xml.contains("<g:title>"));
        assert!(!xml.contains("<g:description>"));
        d["body"] = json!("invalid\u{0001}");
        update(&c, "m_drafts", &d).unwrap();
        assert_eq!(prepare(&c, &b, &p).unwrap()["summary"]["blocked"], 1);
    }
    #[test]
    fn feed_deterministic_and_tampered_manifest_rejected() {
        let (c, b, p) = fixture();
        let r = prepare(&c, &b, &p).unwrap();
        assert_eq!(r, prepare(&c, &b, &p).unwrap());
        validate_manifest(&r["manifest"], &b).unwrap();
        let mut v = r["manifest"].clone();
        v["items"][0]["data"]["price"] = json!("1.00");
        assert!(validate_manifest(&v, &b).is_err());
        let mut v = r["manifest"].clone();
        v["secret"] = json!("RAW_SECRET");
        assert!(validate_manifest(&v, &b).is_err());
    }
    #[test]
    fn feed_manifest_rejects_resigned_missing_identity_and_wrong_types() {
        let (c, b, p) = fixture();
        let good = prepare(&c, &b, &p).unwrap()["manifest"].clone();
        for field in ["brand", "condition", "titleOrigin", "productClass"] {
            let mut v = good.clone();
            v["profile"] = json!("mgeo-catalog-v1");
            v["items"][0]["data"]["catalog"]
                .as_object_mut()
                .unwrap()
                .remove(field);
            resign(&mut v);
            assert!(validate_manifest(&v, &b).is_err(), "missing {field}");
        }
        let mut v = good.clone();
        v["items"][0]["localStatus"] = json!("blocked");
        v["items"][0]["data"] = json!({});
        v["items"][0]["sku"] = Value::Null;
        resign(&mut v);
        assert!(validate_manifest(&v, &b).is_err());
    }
    fn resign(v: &mut Value) {
        for r in v["items"].as_array_mut().unwrap() {
            r.as_object_mut().unwrap().remove("rowHash");
            r["rowHash"] = json!(fingerprint(r));
        }
        v.as_object_mut().unwrap().remove("packageId");
        v["packageId"] = json!(fingerprint(v));
    }
    #[test]
    fn feed_historical_expiry_is_integrity_only_and_profile_limits_block() {
        let (c, b, p) = fixture();
        let mut v = prepare(&c, &b, &p).unwrap()["manifest"].clone();
        for f in v["items"][0]["validity"].as_array_mut().unwrap() {
            f["validUntil"] = json!("2000-01-01");
        }
        resign(&mut v);
        validate_manifest(&v, &b).unwrap();
        for (field, value) in [
            ("catalog:kind", "variant"),
            ("catalog:condition", "used"),
            ("catalog:productClass", "apparel"),
            ("catalog:titleOrigin", "unknown"),
            ("commerce:availability", "PreOrder"),
        ] {
            let mut prod = load(
                &c,
                "m_products",
                &b,
                p["items"][0]["productId"].as_str().unwrap(),
            )
            .unwrap();
            let original = prod.clone();
            prod["facts"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|f| f["id"] == field)
                .unwrap()["value"] = json!(value);
            update(&c, "m_products", &prod).unwrap();
            let r = prepare(&c, &b, &p).unwrap();
            assert_eq!(r["summary"]["blocked"], 1);
            assert!(!r["files"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f["path"] == "google-merchant-basic-draft.xml"));
            update(&c, "m_products", &original).unwrap();
        }
    }
    #[test]
    fn feed_rejects_invalid_question_scope() {
        let (c, b, p) = fixture();
        let mut d = load(
            &c,
            "m_drafts",
            &b,
            p["items"][0]["draftId"].as_str().unwrap(),
        )
        .unwrap();
        d["questionId"] = json!(id());
        update(&c, "m_drafts", &d).unwrap();
        assert!(prepare(&c, &b, &p).is_err());
    }
}
