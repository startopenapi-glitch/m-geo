//! Local catalog identity checks; no merchant request or database write.
use super::*;
const FIELDS: &[(&str, usize)] = &[
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
    ("titleOrigin", 20),
    ("productClass", 30),
];
const DIMENSIONS: &[&str] = &["color", "size", "material", "pattern"];
fn issue(issues: &mut Vec<Value>, field: &str, code: &str) {
    if !issues
        .iter()
        .any(|i| i["field"] == field && i["code"] == code)
    {
        issues.push(json!({"field":field,"code":code,"message":match code {"missing"=>"缺少字段或有效期","private"=>"字段未公开，已省略","expired"=>"字段已过期，已省略","unsourced"=>"字段缺少依据，已省略","duplicate_combination"=>"同组存在重复变体组合","family_mismatch"=>"同组标题或变体维度不一致","family_invalid"=>"同组成员身份资料不完整","contradictory"=>"独立商品含有变体分组或维度","invalid"=>"字段格式或选项无效，已省略",_=>"请核对字段"}}));
    }
}
fn valid_value(key: &str, s: &str, max: usize) -> bool {
    if s.is_empty() || s.chars().count() > max || s.chars().any(char::is_control) {
        return false;
    }
    match key {
        "kind" => ["standalone", "variant"].contains(&s),
        "condition" => ["new", "refurbished", "used"].contains(&s),
        "titleOrigin" => ["human", "ai"].contains(&s),
        "productClass" => ["physical_non_apparel", "apparel", "digital", "other"].contains(&s),
        "gtin" => {
            if ![8, 12, 13, 14].contains(&s.len()) || !s.bytes().all(|b| b.is_ascii_digit()) {
                return false;
            }
            let sum: u32 = s
                .bytes()
                .rev()
                .enumerate()
                .map(|(i, b)| u32::from(b - b'0') * if i % 2 == 0 { 1 } else { 3 })
                .sum();
            sum.is_multiple_of(10)
        }
        _ => true,
    }
}
fn usable<'a>(
    f: &'a Value,
    field: &str,
    required_expiry: bool,
    today: NaiveDate,
    issues: &mut Vec<Value>,
) -> Option<&'a str> {
    if f["visibility"] != "public" {
        issue(issues, field, "private");
        return None;
    }
    if f["source"].as_str().unwrap_or("").trim().is_empty() {
        issue(issues, field, "unsourced");
        return None;
    }
    match f["validUntil"].as_str() {
        Some(s) => match NaiveDate::parse_from_str(s, "%Y-%m-%d") {
            Ok(d) if s.len() == 10 => {
                if d < today {
                    issue(issues, field, "expired");
                    return None;
                }
            }
            _ => {
                issue(issues, field, "invalid");
                return None;
            }
        },
        None => {
            if required_expiry {
                issue(issues, field, "missing");
                return None;
            }
        }
    }
    let s = f["value"].as_str().unwrap_or("").trim();
    if s.is_empty() {
        issue(issues, field, "missing");
        None
    } else {
        Some(s)
    }
}
pub(super) fn public_brand(facts: &[Value], as_of: NaiveDate) -> Option<&str> {
    let mut brands = facts.iter().filter(|f| f["id"] == "catalog:brand");
    let fact = brands.next()?;
    if brands.next().is_some()
        || !matches!(
            fact.get("validUntil"),
            None | Some(Value::Null) | Some(Value::String(_))
        )
    {
        return None;
    }
    let value = usable(fact, "catalog:brand", false, as_of, &mut vec![])?;
    valid_value("brand", value, 70).then_some(value)
}
fn row(product: &Value, today: NaiveDate) -> Value {
    let mut issues = vec![];
    let mut catalog = serde_json::Map::new();
    let empty = vec![];
    let facts = product["facts"].as_array().unwrap_or(&empty);
    for (key, max) in FIELDS {
        let field = format!("catalog:{key}");
        if let Some(f) = facts.iter().find(|f| f["id"] == field) {
            if let Some(s) = usable(f, &field, false, today, &mut issues) {
                if valid_value(key, s, *max) {
                    catalog.insert((*key).into(), json!(s));
                } else {
                    issue(&mut issues, &field, "invalid");
                }
            }
        }
    }
    for f in facts {
        if f["id"].as_str().is_some_and(|id| {
            id.starts_with("catalog:") && !FIELDS.iter().any(|(k, _)| id == format!("catalog:{k}"))
        }) {
            issue(&mut issues, "catalog", "invalid");
        }
    }
    for key in ["kind", "brand", "condition", "titleOrigin", "productClass"] {
        let field = format!("catalog:{key}");
        if !facts.iter().any(|f| f["id"] == field) {
            issue(&mut issues, &field, "missing");
        }
    }
    let kind = catalog.get("kind").and_then(Value::as_str);
    if kind == Some("variant") {
        for key in ["groupId", "groupTitle"] {
            if !catalog.contains_key(key) {
                issue(&mut issues, &format!("catalog:{key}"), "missing");
            }
        }
        if !DIMENSIONS.iter().any(|k| catalog.contains_key(*k)) {
            issue(&mut issues, "catalog:dimensions", "missing");
        }
    }
    if kind == Some("standalone")
        && [
            "groupId",
            "groupTitle",
            "color",
            "size",
            "material",
            "pattern",
        ]
        .iter()
        .any(|k| {
            facts.iter().any(|f| {
                f["id"] == format!("catalog:{k}")
                    && !f["value"].as_str().unwrap_or("").trim().is_empty()
            })
        })
    {
        issue(&mut issues, "catalog:kind", "contradictory");
    }
    let mut freshness = vec![];
    for (key, _) in FIELDS {
        if catalog.contains_key(*key) {
            let field = format!("catalog:{key}");
            if let Some(date) = facts
                .iter()
                .find(|f| f["id"] == field)
                .and_then(|f| f["validUntil"].as_str())
            {
                let expiry = NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap();
                freshness.push(json!({"field":field,"validUntil":date,"status":if expiry<=today+chrono::Duration::days(7){"expiring"}else{"valid"}}));
            }
        }
    }

    for key in ["price", "availability"] {
        let field = format!("commerce:{key}");
        let f = facts.iter().find(|f| f["id"] == field);
        if let Some(f) = f {
            let usable = usable(f, &field, true, today, &mut issues).is_some();
            let date = f["validUntil"]
                .as_str()
                .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());
            let status = if !usable {
                "blocked"
            } else if date.is_some_and(|d| d <= today + chrono::Duration::days(7)) {
                "expiring"
            } else {
                "valid"
            };
            freshness.push(json!({"field":field,"validUntil":if f["visibility"]=="public"{f["validUntil"].clone()}else{Value::Null},"status":status}));
        } else {
            issue(&mut issues, &field, "missing");
            freshness.push(json!({"field":field,"validUntil":null,"status":"missing"}));
        }
    }
    json!({"productId":product["id"],"sku":product["sku"],"name":product["name"],"revision":product["revision"],"catalog":catalog,"issues":issues,"freshness":freshness})
}
pub(super) fn inspect(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    if p.to_string().len() > 2 * 1024 * 1024 {
        return Err("渠道检查输入超过大小限制".into());
    }
    brand(c, b)?;
    let selected = match p.get("productIds") {
        None | Some(Value::Null) => None,
        Some(Value::Array(ids)) => {
            if ids.is_empty() || ids.len() > 200 {
                return Err("请选择 1 至 200 件商品".into());
            }
            let mut seen = HashSet::new();
            let mut selected = vec![];
            for id in ids {
                let s = id.as_str().ok_or("商品标识格式错误")?;
                Uuid::parse_str(s).map_err(|_| "商品标识格式错误")?;
                if !seen.insert(s.to_owned()) {
                    return Err("商品选择不能重复".into());
                }
                load(c, "m_products", b, s)?;
                selected.push(s.to_owned());
            }
            Some(selected)
        }
        _ => return Err("商品选择格式错误".into()),
    };
    let (products, truncated) = list(c, "m_products", b, 1000)?;
    if truncated {
        return Err("品牌商品超过完整检查上限，请先缩小资料范围".into());
    }
    let today = Utc::now().date_naive();
    let all: Vec<Value> = products
        .as_array()
        .ok_or("商品资料格式错误")?
        .iter()
        .map(|p| row(p, today))
        .collect();
    let mut rows = vec![];
    for r in &all {
        let id = r["productId"].as_str().unwrap_or("");
        if selected
            .as_ref()
            .is_some_and(|ids| !ids.iter().any(|i| i == id))
        {
            continue;
        }
        let mut r = r.clone();
        if r["catalog"]["kind"] == "variant" {
            if let Some(group) = r["catalog"]["groupId"].as_str() {
                // Compare raw group membership only in memory so private or expired siblings
                // cannot disappear from family validation. No raw values enter diagnostics.
                let siblings: Vec<_> = all
                    .iter()
                    .zip(products.as_array().unwrap())
                    .filter(|(_, product)| {
                        product["facts"].as_array().is_some_and(|facts| {
                            facts.iter().any(|f| {
                                f["id"] == "catalog:groupId"
                                    && f["value"]
                                        .as_str()
                                        .is_some_and(|value| value.trim() == group)
                            })
                        })
                    })
                    .map(|(row, _)| row)
                    .collect();
                let dims: Vec<_> = DIMENSIONS
                    .iter()
                    .filter(|d| r["catalog"][**d].is_string())
                    .collect();
                let mut codes = vec![];
                let mut combinations = HashSet::new();
                for sibling in siblings {
                    if sibling["catalog"]["kind"] != "variant"
                        || sibling["issues"].as_array().unwrap().iter().any(|i| {
                            i["field"]
                                .as_str()
                                .is_some_and(|s| s.starts_with("catalog"))
                        })
                    {
                        codes.push("family_invalid");
                    }
                    let sibling_dims: Vec<_> = DIMENSIONS
                        .iter()
                        .filter(|d| sibling["catalog"][**d].is_string())
                        .collect();
                    if sibling_dims != dims
                        || sibling["catalog"]["groupTitle"] != r["catalog"]["groupTitle"]
                    {
                        codes.push("family_mismatch");
                    }
                    // Uniqueness belongs to the complete group, including pairs of
                    // siblings that are both absent from the local selection.
                    let combination: Vec<(String, String)> = sibling_dims
                        .iter()
                        .map(|d| {
                            (
                                (**d).to_owned(),
                                sibling["catalog"][**d].as_str().unwrap().to_owned(),
                            )
                        })
                        .collect();
                    if !combination.is_empty() && !combinations.insert(combination) {
                        codes.push("duplicate_combination");
                    }
                }
                let issues = r["issues"].as_array_mut().unwrap();
                for code in codes {
                    issue(issues, "catalog:groupId", code);
                }
            }
        }
        rows.push(r);
    }
    if let Some(ids) = selected {
        rows.sort_by_key(|r| {
            ids.iter()
                .position(|id| r["productId"] == *id)
                .unwrap_or(usize::MAX)
        });
    }
    let ready = rows
        .iter()
        .filter(|r| r["issues"].as_array().unwrap().is_empty())
        .count();
    let expiring = rows
        .iter()
        .filter(|r| {
            r["freshness"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f["status"] == "expiring")
        })
        .count();
    Ok(
        json!({"summary":{"products":rows.len(),"ready":ready,"blocked":rows.len()-ready,"expiring":expiring},"rows":rows}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Connection, String) {
        let mut c = Connection::open_in_memory().unwrap();
        initialize(&mut c).unwrap();
        let b = handle(&mut c, "brand.create", json!({"name":"Fictional catalog"})).unwrap();
        (c, b["id"].as_str().unwrap().into())
    }
    fn fact(key: &str, value: &str) -> Value {
        json!({"id":key,"label":key,"value":value,"source":"Fictional test source","visibility":"public","validUntil":null})
    }
    fn product(c: &mut Connection, b: &str, sku: &str, fs: Vec<Value>) -> Value {
        handle(c,"product.save",json!({"brandId":b,"name":"Fictional product","sku":sku,"category":"Test","officialUrl":"https://example.com/item","sourceText":"RAW_SECRET","privateNotes":"NOTE_SECRET","facts":fs})).unwrap()
    }
    fn identity(kind: &str) -> Vec<Value> {
        vec![
            fact("catalog:kind", kind),
            fact("catalog:brand", "Fictional"),
            fact("catalog:condition", "new"),
            fact("catalog:titleOrigin", "human"),
            fact("catalog:productClass", "physical_non_apparel"),
        ]
    }
    fn issue(row: &Value, field: &str, code: &str) -> bool {
        row["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["field"] == field && i["code"] == code)
    }
    #[test]
    fn public_brand_accepts_only_unique_public_sourced_valid_identity() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 24).unwrap();
        let valid = fact("catalog:brand", "  Fictional Maker  ");
        assert_eq!(
            public_brand(std::slice::from_ref(&valid), today),
            Some("Fictional Maker")
        );
        let mut absent_expiry = valid.clone();
        absent_expiry.as_object_mut().unwrap().remove("validUntil");
        assert_eq!(
            public_brand(&[absent_expiry], today),
            Some("Fictional Maker")
        );
        let max = "杯".repeat(70);
        assert_eq!(
            public_brand(&[fact("catalog:brand", &max)], today),
            Some(max.as_str())
        );
        assert_eq!(public_brand(&[], today), None);
        assert_eq!(public_brand(&[valid.clone(), valid.clone()], today), None);
        let mut private = valid.clone();
        private["visibility"] = json!("private");
        assert_eq!(public_brand(std::slice::from_ref(&private), today), None);
        assert_eq!(public_brand(&[valid.clone(), private], today), None);
        for value in ["", "   ", "Fictional\nMaker", &"杯".repeat(71)] {
            assert_eq!(public_brand(&[fact("catalog:brand", value)], today), None);
        }
        for (field, value) in [
            ("source", json!("  ")),
            ("source", json!(3)),
            ("visibility", json!(null)),
            ("value", json!(9)),
        ] {
            let mut invalid = valid.clone();
            invalid[field] = value;
            assert_eq!(public_brand(&[invalid], today), None);
        }
    }
    #[test]
    fn public_brand_checks_expiry_type_format_and_inclusive_calendar_boundary() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 24).unwrap();
        for (expiry, expected) in [
            (json!(null), Some("Fictional Maker")),
            (json!("2026-10-24"), Some("Fictional Maker")),
            (json!("2026-10-25"), Some("Fictional Maker")),
            (json!("2026-10-23"), None),
            (json!("2026-02-30"), None),
            (json!("2026-1-24"), None),
            (json!("2026-10-24T00:00:00Z"), None),
            (json!(true), None),
            (json!(123), None),
            (json!([]), None),
            (json!({}), None),
        ] {
            let mut f = fact("catalog:brand", "Fictional Maker");
            f["validUntil"] = expiry.clone();
            assert_eq!(public_brand(&[f], today), expected, "expiry: {expiry}");
        }
    }
    #[test]
    fn catalog_family_checks_unselected_members() {
        let (mut c, b) = fixture();
        let mut fs = identity("variant");
        fs.extend([
            fact("catalog:groupId", "group"),
            fact("catalog:groupTitle", "Fictional cup"),
            fact("catalog:color", "red"),
        ]);
        let a = product(&mut c, &b, "A", fs.clone());
        product(&mut c, &b, "B", fs);
        let r = inspect(&c, &b, &json!({"productIds":[a["id"]]})).unwrap();
        assert_eq!(r["rows"].as_array().unwrap().len(), 1);
        assert!(issue(
            &r["rows"][0],
            "catalog:groupId",
            "duplicate_combination"
        ));
    }
    #[test]
    fn catalog_private_expired_never_exposes_value() {
        let (mut c, b) = fixture();
        let mut fs = identity("standalone");
        fs[1]["visibility"] = json!("private");
        fs[1]["value"] = json!("PRIVATE_SECRET");
        let mut expired = fact("catalog:mpn", "EXPIRED_SECRET");
        expired["validUntil"] = json!("2000-01-01");
        fs.push(expired);
        let p = product(&mut c, &b, "A", fs);
        let r = inspect(&c, &b, &json!({"productIds":[p["id"]]})).unwrap();
        let row = &r["rows"][0];
        assert!(issue(row, "catalog:brand", "private"));
        assert!(issue(row, "catalog:mpn", "expired"));
        for secret in [
            "PRIVATE_SECRET",
            "EXPIRED_SECRET",
            "RAW_SECRET",
            "NOTE_SECRET",
            "Fictional test source",
        ] {
            assert!(!r.to_string().contains(secret));
        }
    }
    #[test]
    fn catalog_missing_kind_is_unknown_and_seven_day_expiry() {
        let (mut c, b) = fixture();
        let today = Utc::now().date_naive();
        let mut fs = vec![
            fact("commerce:price", "12.00"),
            fact("commerce:availability", "InStock"),
        ];
        fs[0]["validUntil"] = json!(today.format("%Y-%m-%d").to_string());
        fs[1]["validUntil"] = json!((today + chrono::Duration::days(7))
            .format("%Y-%m-%d")
            .to_string());
        let p = product(&mut c, &b, "A", fs);
        let r = inspect(&c, &b, &json!({"productIds":[p["id"]]})).unwrap();
        let row = &r["rows"][0];
        assert!(row["catalog"]["kind"].is_null());
        assert!(issue(row, "catalog:kind", "missing"));
        assert_eq!(row["freshness"][0]["status"], "expiring");
        assert_eq!(row["freshness"][1]["status"], "expiring");
        assert_eq!(r["summary"]["expiring"], 1);
    }
    #[test]
    fn catalog_gtin_checksum() {
        let (mut c, b) = fixture();
        for (sku, gtin, valid) in [
            ("A", "4006381333931", true),
            ("B", "4006381333932", false),
            ("C", "96385074", true),
            ("D", "1234567", false),
        ] {
            let mut fs = identity("standalone");
            fs.push(fact("catalog:gtin", gtin));
            let p = product(&mut c, &b, sku, fs);
            let r = inspect(&c, &b, &json!({"productIds":[p["id"]]})).unwrap();
            assert_eq!(r["rows"][0]["catalog"]["gtin"].as_str().is_some(), valid);
            if !valid {
                assert!(issue(&r["rows"][0], "catalog:gtin", "invalid"));
            }
        }
    }
    #[test]
    fn catalog_private_group_still_blocks_selected_family_without_exposure() {
        let (mut c, b) = fixture();
        let mut fs = identity("variant");
        fs.extend([
            fact("catalog:groupId", "PRIVATE_GROUP"),
            fact("catalog:groupTitle", "Cup"),
            fact("catalog:color", "red"),
        ]);
        let p = product(&mut c, &b, "A", fs.clone());
        fs[5]["visibility"] = json!("private");
        fs[7]["value"] = json!("blue");
        product(&mut c, &b, "B", fs);
        let r = inspect(&c, &b, &json!({"productIds":[p["id"]]})).unwrap();
        assert!(issue(&r["rows"][0], "catalog:groupId", "family_invalid"));
    }
    #[test]
    fn catalog_tracks_identity_expiring_within_seven_days() {
        let (mut c, b) = fixture();
        let mut fs = identity("standalone");
        fs[1]["validUntil"] = json!((Utc::now().date_naive() + chrono::Duration::days(7))
            .format("%Y-%m-%d")
            .to_string());
        let p = product(&mut c, &b, "A", fs);
        let r = inspect(&c, &b, &json!({"productIds":[p["id"]]})).unwrap();
        assert_eq!(r["summary"]["expiring"], 1);
        assert!(r["rows"][0]["freshness"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["field"] == "catalog:brand" && f["status"] == "expiring"));
    }
    #[test]
    fn catalog_duplicate_between_unselected_members_blocks_entire_family() {
        let (mut c, b) = fixture();
        let mut ids = vec![];
        for (sku, color) in [("A", "red"), ("B", "blue"), ("C", "blue")] {
            let mut facts = identity("variant");
            facts.extend([
                fact("catalog:groupId", "family"),
                fact("catalog:groupTitle", "Fictional cup"),
                fact("catalog:color", color),
            ]);
            for (key, value) in [
                ("commerce:price", "12.00"),
                ("commerce:availability", "InStock"),
            ] {
                let mut f = fact(key, value);
                f["validUntil"] = json!("2099-01-01");
                facts.push(f);
            }
            ids.push(product(&mut c, &b, sku, facts)["id"].clone());
        }
        let selected = inspect(&c, &b, &json!({"productIds":[ids[0]]})).unwrap();
        assert!(issue(
            &selected["rows"][0],
            "catalog:groupId",
            "duplicate_combination"
        ));
        assert_eq!(selected["summary"]["ready"], 0);
        assert_eq!(selected["summary"]["blocked"], 1);
        let complete = inspect(&c, &b, &json!({"productIds":ids})).unwrap();
        assert_eq!(complete["summary"]["ready"], 0);
        assert_eq!(complete["summary"]["blocked"], 3);
        assert!(complete["rows"].as_array().unwrap().iter().all(|row| issue(
            row,
            "catalog:groupId",
            "duplicate_combination"
        )));
    }
}
