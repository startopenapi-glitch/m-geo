//! Immutable, locally supplied commerce evidence. No identity inference or network access.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

#[path = "attribution_diagnostics.rs"]
mod diagnostics;

pub(super) fn diagnose_exchange(
    c: &Connection,
    b: &str,
    input: &Value,
    name: &Value,
) -> Result<Vec<Value>> {
    diagnostics::diagnose(c, b, input, name)
}

pub(super) const TABLE_SQL: &str = "CREATE TABLE IF NOT EXISTS m_attribution_datasets(id TEXT PRIMARY KEY,brand_id TEXT NOT NULL,content_hash TEXT NOT NULL,data TEXT NOT NULL,UNIQUE(brand_id,content_hash),FOREIGN KEY(brand_id) REFERENCES m_brands(id));";
const MAX_BYTES: usize = 512 * 1024;
const INVALID: &str = "归因资料格式无效，请检查字段、时间、金额与引用";

// Reuse the shared recursive duplicate-key parser for import and frozen evidence.
fn parse(s: &str) -> Result<Value> {
    strict_json::parse(s).map_err(|_| INVALID.into())
}
fn exact(v: &Value, keys: &[&str]) -> Result<()> {
    let o = v.as_object().ok_or(INVALID)?;
    if o.len() != keys.len() || keys.iter().any(|k| !o.contains_key(*k)) {
        return Err(INVALID.into());
    }
    Ok(())
}
fn plain(v: &Value, k: &str, max: usize) -> Result<String> {
    let s = v[k].as_str().ok_or(INVALID)?;
    if s.trim().is_empty() || s.chars().count() > max || s.chars().any(char::is_control) {
        return Err(INVALID.into());
    }
    Ok(s.to_owned())
}
fn token(v: &Value, k: &str) -> Result<String> {
    let s = plain(v, k, 80)?;
    if !s
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b))
    {
        return Err(INVALID.into());
    }
    Ok(s)
}
fn selection(v: &Value, k: &str, options: &[&str]) -> Result<String> {
    let s = v[k].as_str().ok_or(INVALID)?;
    if !options.contains(&s) {
        return Err(INVALID.into());
    }
    Ok(s.to_owned())
}
fn time(v: &Value, k: &str) -> Result<(String, i64)> {
    let s = v[k].as_str().ok_or(INVALID)?;
    if s.len() > 80 || s.contains('.') {
        return Err(INVALID.into());
    }
    let t = DateTime::parse_from_rfc3339(s).map_err(|_| INVALID)?;
    if t.timestamp_subsec_nanos() != 0 {
        return Err(INVALID.into());
    }
    Ok((
        t.with_timezone(&Utc)
            .to_rfc3339_opts(SecondsFormat::Secs, true),
        t.timestamp(),
    ))
}
fn money(v: &Value) -> Result<i64> {
    v["amountMinor"]
        .as_i64()
        .filter(|v| (1..=1_000_000_000).contains(v))
        .ok_or_else(|| INVALID.into())
}
fn normalize(input: &Value) -> Result<(Value, usize)> {
    exact(
        input,
        &[
            "format",
            "evidenceMode",
            "sourceLabel",
            "coverageStart",
            "coverageEnd",
            "currency",
            "visits",
            "orders",
            "refunds",
        ],
    )?;
    selection(input, "format", &["m-geo-attribution-v1"])?;
    let mode = selection(input, "evidenceMode", &["simulated", "recorded"])?;
    let label = plain(input, "sourceLabel", 100)?;
    let currency = selection(input, "currency", &["CNY", "USD", "EUR"])?;
    let (start, st) = time(input, "coverageStart")?;
    let (end, en) = time(input, "coverageEnd")?;
    if en <= st || en - st > 366 * 86400 {
        return Err(INVALID.into());
    }
    let mut raw_rows = 0usize;
    for key in ["visits", "orders", "refunds"] {
        raw_rows = raw_rows
            .checked_add(input[key].as_array().ok_or(INVALID)?.len())
            .ok_or(INVALID)?;
    }
    if raw_rows > 1000
        || (input["visits"].as_array().unwrap().is_empty()
            && input["orders"].as_array().unwrap().is_empty())
    {
        return Err(INVALID.into());
    }
    let mut duplicates = 0;
    let mut output = BTreeMap::new();
    for kind in ["visits", "orders", "refunds"] {
        let mut rows = BTreeMap::<String, Value>::new();
        for v in input[kind].as_array().unwrap() {
            exact(
                v,
                match kind {
                    "visits" => &[
                        "id",
                        "sessionId",
                        "sku",
                        "occurredAt",
                        "source",
                        "sourceEvidence",
                    ],
                    "orders" => &[
                        "id",
                        "sessionId",
                        "sku",
                        "occurredAt",
                        "status",
                        "amountMinor",
                    ],
                    _ => &["id", "orderId", "occurredAt", "amountMinor"],
                },
            )?;
            let rid = token(v, "id")?;
            let (at, ts) = time(v, "occurredAt")?;
            if ts < st || ts >= en {
                return Err(INVALID.into());
            }
            let mut row = json!({"id":rid,"occurredAt":at});
            if kind == "refunds" {
                row["orderId"] = json!(token(v, "orderId")?);
                row["amountMinor"] = json!(money(v)?);
            } else {
                // Use the catalog's SKU rule; accepted internal newlines/tabs are identity.
                row["sku"] = json!(text(v, "sku", 100, true).map_err(|_| INVALID)?);
                row["sessionId"] = if kind == "orders" && v["sessionId"].is_null() {
                    Value::Null
                } else {
                    json!(token(v, "sessionId")?)
                };
                if kind == "visits" {
                    let source = selection(
                        v,
                        "source",
                        &["doubao", "yuanbao", "qianwen", "baidu", "other", "unknown"],
                    )?;
                    let evidence =
                        selection(v, "sourceEvidence", &["tagged", "referrer", "unknown"])?;
                    if (source == "unknown") != (evidence == "unknown") {
                        return Err(INVALID.into());
                    }
                    row["source"] = json!(source);
                    row["sourceEvidence"] = json!(evidence);
                } else {
                    row["status"] = json!(selection(v, "status", &["paid", "cancelled"])?);
                    row["amountMinor"] = json!(money(v)?);
                }
            }
            if let Some(old) = rows.get(&rid) {
                if old != &row {
                    return Err(INVALID.into());
                }
                duplicates += 1;
            } else {
                rows.insert(rid, row);
            }
        }
        output.insert(kind, rows);
    }
    let mut refunds = BTreeMap::<String, i64>::new();
    for refund in output["refunds"].values() {
        let oid = refund["orderId"].as_str().unwrap();
        let order = output["orders"].get(oid).ok_or(INVALID)?;
        if order["status"] != "paid" || time(refund, "occurredAt")?.1 < time(order, "occurredAt")?.1
        {
            return Err(INVALID.into());
        }
        let sum = refunds.entry(oid.to_owned()).or_default();
        *sum = sum.checked_add(money(refund)?).ok_or(INVALID)?;
        if *sum > money(order)? {
            return Err(INVALID.into());
        }
    }
    let result = json!({"format":"m-geo-attribution-v1","evidenceMode":mode,"sourceLabel":label,"coverageStart":start,"coverageEnd":end,"currency":currency,"visits":output["visits"].values().collect::<Vec<_>>(),"orders":output["orders"].values().collect::<Vec<_>>(),"refunds":output["refunds"].values().collect::<Vec<_>>()});
    // UTC normalization/JSON escaping can expand compact source bytes. Keep exports reimportable.
    if result.to_string().len() > MAX_BYTES {
        return Err(INVALID.into());
    }
    Ok((result, duplicates))
}
fn skus(input: &Value) -> BTreeSet<String> {
    ["visits", "orders"]
        .iter()
        .flat_map(|k| input[*k].as_array().unwrap())
        .map(|v| v["sku"].as_str().unwrap().to_owned())
        .collect()
}
fn snapshot(c: &Connection, b: &str, input: &Value) -> Result<Value> {
    let mut out = vec![];
    for sku in skus(input) {
        let mut q = c
            .prepare("SELECT id,data FROM m_products WHERE brand_id=?1 AND sku=?2")
            .map_err(db_error)?;
        let rows = q
            .query_map(params![b, sku], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(db_error)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(db_error)?;
        if rows.len() != 1 {
            return Err(INVALID.into());
        }
        let (pid, data) = &rows[0];
        let p = parse(data)?;
        if p["id"] != *pid || p["brandId"] != b || p["sku"] != sku {
            return Err(INVALID.into());
        }
        Uuid::parse_str(pid).map_err(|_| INVALID)?;
        // Snapshot names share product.save validation, including accepted newlines/tabs.
        let name = text(&p, "name", 200, true).map_err(|_| INVALID)?;
        let rev = p["revision"].as_i64().filter(|v| *v > 0).ok_or(INVALID)?;
        out.push(json!({"id":pid,"sku":sku,"name":name,"revision":rev}));
    }
    Ok(json!(out))
}
fn prepare(
    c: &Connection,
    b: &str,
    p: &Value,
    applying: bool,
) -> Result<(Value, Value, String, usize, String)> {
    exact(
        p,
        if applying {
            &["brandId", "name", "content", "expectedHash"]
        } else {
            &["brandId", "name", "content"]
        },
    )?;
    if p["brandId"] != b {
        return Err(INVALID.into());
    }
    brand(c, b)?;
    let name = plain(p, "name", 100)?;
    let content = p["content"].as_str().ok_or(INVALID)?;
    if content.len() > MAX_BYTES {
        return Err(INVALID.into());
    }
    let (input, duplicates) = normalize(&parse(content)?)?;
    let products = snapshot(c, b, &input)?;
    let hash = fingerprint(&json!({"input":input,"products":products}));
    Ok((input, products, hash, duplicates, name))
}
fn counts(input: &Value) -> Value {
    json!({"visits":input["visits"].as_array().unwrap().len(),"orders":input["orders"].as_array().unwrap().len(),"refunds":input["refunds"].as_array().unwrap().len()})
}
// Portable validation has no database dependency; live ownership stays in stored().
pub(super) fn validate_portable_dataset(sql_brand: &str, v: &Value) -> Result<()> {
    exact(
        v,
        &[
            "id",
            "brandId",
            "name",
            "createdAt",
            "contentHash",
            "input",
            "products",
        ],
    )?;
    if v["brandId"] != sql_brand || v.to_string().len() > 2 * 1024 * 1024 {
        return Err(INVALID.into());
    }
    Uuid::parse_str(v["id"].as_str().ok_or(INVALID)?).map_err(|_| INVALID)?;
    Uuid::parse_str(sql_brand).map_err(|_| INVALID)?;
    plain(v, "name", 100)?;
    if v["createdAt"] != time(v, "createdAt")?.0 {
        return Err(INVALID.into());
    }
    let (input, duplicates) = normalize(&v["input"])?;
    if duplicates != 0 || input != v["input"] {
        return Err(INVALID.into());
    }
    let products = v["products"].as_array().ok_or(INVALID)?;
    let mut seen_ids = BTreeSet::new();
    let mut seen_skus = BTreeSet::new();
    let mut previous = None::<String>;
    for product in products {
        exact(product, &["id", "sku", "name", "revision"])?;
        let pid = product["id"].as_str().ok_or(INVALID)?;
        let product_uuid = Uuid::parse_str(pid).map_err(|_| INVALID)?;
        let sku = text(product, "sku", 100, true).map_err(|_| INVALID)?;
        let name = text(product, "name", 200, true).map_err(|_| INVALID)?;
        if product["sku"] != sku || product["name"] != name {
            return Err(INVALID.into());
        }
        if product["revision"].as_i64().filter(|v| *v > 0).is_none()
            || !seen_ids.insert(product_uuid)
            || !seen_skus.insert(sku.clone())
            || previous.as_ref().is_some_and(|v| v >= &sku)
        {
            return Err(INVALID.into());
        }
        previous = Some(sku);
    }
    if seen_skus != skus(&input)
        || fingerprint(&json!({"input":input,"products":products}))
            != v["contentHash"].as_str().ok_or(INVALID)?
    {
        return Err(INVALID.into());
    }
    Ok(())
}
fn stored(
    c: &Connection,
    sql_id: &str,
    sql_brand: &str,
    sql_hash: &str,
    data: &str,
) -> Result<Value> {
    let v = parse(data)?;
    if v["id"] != sql_id || v["brandId"] != sql_brand || v["contentHash"] != sql_hash {
        return Err(INVALID.into());
    }
    validate_portable_dataset(sql_brand, &v)?;
    brand(c, sql_brand)?;
    for product in v["products"].as_array().ok_or(INVALID)? {
        // Frozen labels may differ, but live callers must still own every product.
        let owns: bool = c
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM m_products WHERE id=?1 AND brand_id=?2)",
                params![product["id"].as_str().ok_or(INVALID)?, sql_brand],
                |r| r.get(0),
            )
            .map_err(db_error)?;
        if !owns {
            return Err(INVALID.into());
        }
    }
    Ok(v)
}
fn find_hash(c: &Connection, b: &str, hash: &str) -> Result<Option<Value>> {
    let row: Option<(String, String)> = c
        .query_row(
            "SELECT id,data FROM m_attribution_datasets WHERE brand_id=?1 AND content_hash=?2",
            params![b, hash],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(db_error)?;
    row.map(|(id, data)| stored(c, &id, b, hash, &data))
        .transpose()
}
pub(super) fn normalize_exchange(content: &str) -> Result<Value> {
    if content.len() > MAX_BYTES {
        return Err(INVALID.into());
    }
    normalize(&parse(content)?).map(|(input, _)| input)
}
// Validate a prospective frozen snapshot without requiring its ID to be persisted.
pub(super) fn validate_frozen_dataset(c: &Connection, b: &str, v: &Value) -> Result<()> {
    let key = v["id"].as_str().ok_or(INVALID)?;
    let hash = v["contentHash"].as_str().ok_or(INVALID)?;
    stored(c, key, b, hash, &v.to_string()).map(|_| ())
}

pub(super) fn preview(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    let (input, products, hash, duplicates, _) = prepare(c, b, p, false)?;
    let mut summary = counts(&input);
    summary["duplicates"] = json!(duplicates);
    Ok(
        json!({"contentHash":hash,"evidenceMode":input["evidenceMode"],"sourceLabel":input["sourceLabel"],"currency":input["currency"],"coverageStart":input["coverageStart"],"coverageEnd":input["coverageEnd"],"counts":summary,"products":products,"alreadyImported":find_hash(c,b,&hash)?.is_some()}),
    )
}
pub(super) fn apply(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    let (input, products, hash, _, name) = prepare(c, b, p, true)?;
    if p["expectedHash"].as_str() != Some(hash.as_str()) {
        return Err("预览已失效，请重新预览归因资料".into());
    }
    if let Some(v) = find_hash(c, b, &hash)? {
        return Ok(
            json!({"datasetId":v["id"],"alreadyImported":true,"evidenceMode":v["input"]["evidenceMode"]}),
        );
    }
    let count: i64 = c
        .query_row(
            "SELECT count(*) FROM m_attribution_datasets WHERE brand_id=?1",
            [b],
            |r| r.get(0),
        )
        .map_err(db_error)?;
    if count >= 50 {
        return Err("每个品牌最多保存50份归因数据集".into());
    }
    let key = id();
    let v = json!({"id":key,"brandId":b,"name":name,"createdAt":now(),"contentHash":hash,"input":input,"products":products});
    c.execute(
        "INSERT INTO m_attribution_datasets(id,brand_id,content_hash,data)VALUES(?1,?2,?3,?4)",
        params![key, b, hash, v.to_string()],
    )
    .map_err(db_error)?;
    Ok(json!({"datasetId":key,"alreadyImported":false,"evidenceMode":input["evidenceMode"]}))
}
pub(super) fn load_dataset(c: &Connection, b: &str, key: &str) -> Result<Value> {
    let row: Option<(String, String)> = c
        .query_row(
            "SELECT content_hash,data FROM m_attribution_datasets WHERE id=?1 AND brand_id=?2",
            params![key, b],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(db_error)?;
    let (hash, data) = row.ok_or("记录不存在或不属于当前品牌")?;
    stored(c, key, b, &hash, &data)
}
pub(super) fn list(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    exact(p, &["brandId"])?;
    if p["brandId"] != b {
        return Err(INVALID.into());
    }
    brand(c, b)?;
    let mut q = c
        .prepare("SELECT id,content_hash,data FROM m_attribution_datasets WHERE brand_id=?1")
        .map_err(db_error)?;
    let rows = q
        .query_map([b], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(db_error)?;
    let mut out = vec![];
    for row in rows {
        let (key, hash, data) = row.map_err(db_error)?;
        let v = stored(c, &key, b, &hash, &data)?;
        out.push(json!({"id":v["id"],"name":v["name"],"createdAt":v["createdAt"],"evidenceMode":v["input"]["evidenceMode"],"sourceLabel":v["input"]["sourceLabel"],"currency":v["input"]["currency"],"coverageStart":v["input"]["coverageStart"],"coverageEnd":v["input"]["coverageEnd"],"counts":counts(&v["input"]),"contentHash":hash}));
        if out.len() > 50 {
            return Err(INVALID.into());
        }
    }
    out.sort_by(|a, b| {
        b["createdAt"]
            .as_str()
            .cmp(&a["createdAt"].as_str())
            .then_with(|| a["id"].as_str().cmp(&b["id"].as_str()))
    });
    Ok(json!({"datasets":out}))
}
pub(super) fn validate_database(c: &Connection) -> Result<()> {
    let mut q = c
        .prepare("SELECT id,brand_id,content_hash,data FROM m_attribution_datasets")
        .map_err(db_error)?;
    let rows = q
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })
        .map_err(db_error)?;
    let mut counts = BTreeMap::<String, usize>::new();
    let mut hashes = BTreeSet::new();
    for row in rows {
        let (key, b, hash, data) = row.map_err(db_error)?;
        stored(c, &key, &b, &hash, &data)?;
        let count = counts.entry(b.clone()).or_default();
        *count += 1;
        if *count > 50 || !hashes.insert((b, hash)) {
            return Err(INVALID.into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn phase6_helpers_validate_canonical_exchange_and_historical_snapshot() {
        let (mut c, b, p) = fixture();
        let raw = exchange();
        let canonical = normalize_exchange(&raw.to_string()).unwrap();
        assert_eq!(canonical["visits"][0]["occurredAt"], "2026-10-02T00:00:00Z");
        assert!(normalize_exchange("{\"format\":1,\"format\":2}").is_err());
        assert!(normalize_exchange(&" ".repeat(512 * 1024 + 1)).is_err());
        let saved = save(&mut c, &b, &canonical);
        let frozen = load_dataset(&c, &b, saved["datasetId"].as_str().unwrap()).unwrap();
        handle(&mut c,"product.save",json!({"brandId":b,"id":p["id"],"expectedRevision":p["revision"],"name":"虚构已改名","sku":"SIM-CHANGED","facts":[]})).unwrap();
        validate_frozen_dataset(&c, &b, &frozen).unwrap();
        let other = handle(&mut c, "brand.create", json!({"name":"另一个虚构品牌"})).unwrap();
        assert!(validate_frozen_dataset(&c, other["id"].as_str().unwrap(), &frozen).is_err());
        let mut tampered = frozen;
        tampered["input"]["orders"][0]["amountMinor"] = json!(2000);
        assert!(validate_frozen_dataset(&c, &b, &tampered).is_err());
    }

    fn fixture() -> (Connection, String, Value) {
        let mut c = Connection::open_in_memory().unwrap();
        initialize(&mut c).unwrap();
        let b = handle(&mut c, "brand.create", json!({"name":"虚构归因品牌"})).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let p = handle(
            &mut c,
            "product.save",
            json!({"brandId":b,"name":"虚构茶杯","sku":"SIM-1","facts":[]}),
        )
        .unwrap();
        (c, b, p)
    }
    fn exchange() -> Value {
        json!({"format":"m-geo-attribution-v1","evidenceMode":"simulated","sourceLabel":"虚构演练","coverageStart":"2026-10-01T00:00:00Z","coverageEnd":"2026-11-01T00:00:00Z","currency":"CNY","visits":[{"id":"v1","sessionId":"session-1","sku":"SIM-1","occurredAt":"2026-10-02T08:00:00+08:00","source":"doubao","sourceEvidence":"tagged"}],"orders":[{"id":"o1","sessionId":"session-1","sku":"SIM-1","occurredAt":"2026-10-03T00:00:00Z","status":"paid","amountMinor":1000}],"refunds":[{"id":"r1","orderId":"o1","occurredAt":"2026-10-04T00:00:00Z","amountMinor":100}]})
    }
    fn payload(b: &str, x: &Value) -> Value {
        json!({"brandId":b,"name":"虚构导入","content":x.to_string()})
    }
    fn save(c: &mut Connection, b: &str, x: &Value) -> Value {
        let mut p = payload(b, x);
        let v = handle(c, "attribution.preview", p.clone()).unwrap();
        p["expectedHash"] = v["contentHash"].clone();
        handle(c, "attribution.apply", p).unwrap()
    }
    fn count(c: &Connection) -> i64 {
        c.query_row("SELECT count(*) FROM m_attribution_datasets", [], |r| {
            r.get(0)
        })
        .unwrap()
    }

    // A catalog-valid SKU must survive our own template, immutable storage and backups.
    #[test]
    fn attribution_product_sku_newline_tab_survives_template_export_and_backup() {
        use std::io::{Cursor, Read};
        let mut c = crate::runtime::open_database(std::path::Path::new(":memory:")).unwrap();
        let b = handle(&mut c, "brand.create", json!({"name":"虚构SKU兼容演练"})).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let product = handle(
            &mut c,
            "product.save",
            json!({"brandId":b,"name":"虚构茶杯","sku":"SIM-1","facts":[]}),
        )
        .unwrap();
        let accepted_sku = "SIM-A\nBLUE\t演练";
        let mut edit = product.clone();
        edit["sku"] = json!(accepted_sku);
        edit["expectedRevision"] = product["revision"].clone();
        let saved_product = handle(&mut c, "product.save", edit).unwrap();
        assert_eq!(saved_product["sku"], accepted_sku);
        for invalid in ["SIM\0BAD", " \n\t "] {
            assert!(handle(
                &mut c,
                "product.save",
                json!({"brandId":b,"sku":invalid,"name":"虚构非法SKU","facts":[]})
            )
            .is_err());
        }
        let template = handle(&mut c, "attribution.template", json!({"brandId":b})).unwrap();
        let x = template["exchange"].clone();
        assert_eq!(x["visits"][0]["sku"], accepted_sku);
        assert_eq!(x["orders"][0]["sku"], accepted_sku);
        assert_eq!(count(&c), 0);
        let preview = handle(&mut c, "attribution.preview", payload(&b, &x)).unwrap();
        assert_eq!(preview["products"][0]["sku"], accepted_sku);
        for kind in ["visits", "orders"] {
            for invalid in ["SIM\0BAD", " \n\t "] {
                let mut bad = x.clone();
                bad[kind][0]["sku"] = json!(invalid);
                assert!(handle(&mut c, "attribution.preview", payload(&b, &bad)).is_err());
            }
        }
        let saved = save(&mut c, &b, &x);
        let dataset_id = saved["datasetId"].as_str().unwrap();
        let original = load_dataset(&c, &b, dataset_id).unwrap();
        assert_eq!(original["products"][0]["sku"], accepted_sku);
        assert_eq!(save(&mut c, &b, &x)["alreadyImported"], true);
        let request = json!({"brandId":b,"datasetId":dataset_id,"from":"2026-10-01T00:00:00Z","to":"2026-10-23T00:00:00Z","lookbackDays":7});
        let report = handle(&mut c, "attribution.report", request.clone()).unwrap();
        assert_eq!(report["byProduct"][0]["sku"], accepted_sku);
        assert_eq!(report["summary"]["grossMinor"], 81000);
        assert_eq!(report["summary"]["refundMinor"], 22000);
        assert_eq!(report["summary"]["netMinor"], 59000);
        let zip = crate::files::site_zip(&report).unwrap();
        let mut archive = zip::ZipArchive::new(Cursor::new(zip)).unwrap();
        assert_eq!(archive.len(), 9);
        let mut input_json = String::new();
        archive
            .by_name("input.json")
            .unwrap()
            .read_to_string(&mut input_json)
            .unwrap();
        let exported_input: Value = serde_json::from_str(&input_json).unwrap();
        assert_eq!(exported_input, original["input"]);
        assert_eq!(exported_input["visits"][0]["sku"], accepted_sku);
        assert!(
            handle(&mut c, "attribution.preview", payload(&b, &exported_input)).unwrap()
                ["alreadyImported"]
                .as_bool()
                .unwrap()
        );
        let mut dataset_json = String::new();
        archive
            .by_name("dataset.json")
            .unwrap()
            .read_to_string(&mut dataset_json)
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&dataset_json).unwrap(),
            original
        );
        let mut csv = String::new();
        archive
            .by_name("products.csv")
            .unwrap()
            .read_to_string(&mut csv)
            .unwrap();
        assert!(csv.contains(&format!("\"{accepted_sku}\"")));
        let mut edit = saved_product.clone();
        edit["sku"] = json!("LATER-SKU");
        edit["name"] = json!("后来更名的虚构商品");
        edit["expectedRevision"] = saved_product["revision"].clone();
        handle(&mut c, "product.save", edit).unwrap();
        super::super::validate_database(&c).unwrap();
        let bytes = crate::backup::encrypt_snapshot(&c, "fictional-test-password").unwrap();
        let mut restored = crate::runtime::open_database(std::path::Path::new(":memory:")).unwrap();
        crate::backup::restore_snapshot(&mut restored, &bytes, "fictional-test-password").unwrap();
        assert_eq!(load_dataset(&restored, &b, dataset_id).unwrap(), original);
        assert_eq!(
            handle(&mut restored, "attribution.report", request).unwrap(),
            report
        );
        super::super::validate_database(&restored).unwrap();
        for invalid in ["SIM\0BAD", " \n\t "] {
            let mut corrupt = original.clone();
            corrupt["products"][0]["sku"] = json!(invalid);
            corrupt["contentHash"] = json!(fingerprint(
                &json!({"input":corrupt["input"],"products":corrupt["products"]})
            ));
            c.execute(
                "UPDATE m_attribution_datasets SET data=?1,content_hash=?2 WHERE id=?3",
                params![
                    corrupt.to_string(),
                    corrupt["contentHash"].as_str(),
                    dataset_id
                ],
            )
            .unwrap();
            assert!(load_dataset(&c, &b, dataset_id).is_err());
            assert!(validate_database(&c).is_err());
            assert!(crate::backup::encrypt_snapshot(&c, "fictional-test-password").is_err());
        }
    }

    // Product names accepted by product.save must remain usable as frozen evidence.
    #[test]
    fn attribution_product_name_newline_tab_survives_public_flow() {
        let (mut c, b, product) = fixture();
        let accepted_name = "虚构茶杯\n双层款\t演练";
        let mut edit = product.clone();
        edit["name"] = json!(accepted_name);
        edit["expectedRevision"] = product["revision"].clone();
        let saved_product = handle(&mut c, "product.save", edit).unwrap();
        assert_eq!(saved_product["name"], accepted_name);
        let x = exchange();
        let preview = handle(&mut c, "attribution.preview", payload(&b, &x)).unwrap();
        assert_eq!(preview["products"][0]["name"], accepted_name);
        let saved = save(&mut c, &b, &x);
        let dataset_id = saved["datasetId"].as_str().unwrap();
        assert_eq!(
            load_dataset(&c, &b, dataset_id).unwrap()["products"][0]["name"],
            accepted_name
        );
        assert!(validate_database(&c).is_ok());
        assert_eq!(
            handle(&mut c, "attribution.list", json!({"brandId":b})).unwrap()["datasets"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        let report_payload = json!({"brandId":b,"datasetId":dataset_id,"from":"2026-10-01T00:00:00Z","to":"2026-11-01T00:00:00Z","lookbackDays":7});
        let report = handle(&mut c, "attribution.report", report_payload.clone()).unwrap();
        assert_eq!(report["byProduct"][0]["name"], accepted_name);
        assert_eq!(report["summary"]["netMinor"], 900);
        let mut edit = saved_product.clone();
        edit["name"] = json!("后来的商品名称");
        edit["expectedRevision"] = saved_product["revision"].clone();
        handle(&mut c, "product.save", edit).unwrap();
        assert!(validate_database(&c).is_ok());
        assert_eq!(
            handle(&mut c, "attribution.report", report_payload).unwrap()["byProduct"][0]["name"],
            accepted_name
        );
        let mut corrupt = load_dataset(&c, &b, dataset_id).unwrap();
        corrupt["products"][0]["name"] = json!("虚构\0名称");
        corrupt["contentHash"] = json!(fingerprint(
            &json!({"input":corrupt["input"],"products":corrupt["products"]})
        ));
        c.execute(
            "UPDATE m_attribution_datasets SET data=?1,content_hash=?2 WHERE id=?3",
            params![
                corrupt.to_string(),
                corrupt["contentHash"].as_str(),
                dataset_id
            ],
        )
        .unwrap();
        assert!(load_dataset(&c, &b, dataset_id).is_err());
        assert!(validate_database(&c).is_err());
    }
    // Missing strict normalization would merge conflicts or leak a partial save.
    #[test]
    fn attribution_preview_duplicate_conflict_atomic() {
        let (mut c, b, _) = fixture();
        let mut x = exchange();
        for kind in ["visits", "orders", "refunds"] {
            let row = x[kind][0].clone();
            x[kind].as_array_mut().unwrap().push(row);
        }
        x["visits"][1]["occurredAt"] = json!("2026-10-02T00:00:00Z");
        let p = payload(&b, &x);
        let v = handle(&mut c, "attribution.preview", p).unwrap();
        assert_eq!(
            v["counts"],
            json!({"visits":1,"orders":1,"refunds":1,"duplicates":3})
        );
        assert_eq!(v["coverageStart"], "2026-10-01T00:00:00Z");
        assert_eq!(count(&c), 0);
        let result = save(&mut c, &b, &x);
        let d = load_dataset(&c, &b, result["datasetId"].as_str().unwrap()).unwrap();
        assert_eq!(
            d["input"]["visits"][0]["occurredAt"],
            "2026-10-02T00:00:00Z"
        );
        assert!(!d.to_string().contains("sourceText"));
        assert_eq!(d["products"][0]["name"], "虚构茶杯");
        x["visits"][1]["source"] = json!("baidu");
        assert!(handle(&mut c, "attribution.preview", payload(&b, &x)).is_err());
        assert_eq!(count(&c), 1);
        x = exchange();
        let mut p = payload(&b, &x);
        p["expectedHash"] =
            handle(&mut c, "attribution.preview", p.clone()).unwrap()["contentHash"].clone();
        x["refunds"][0]["amountMinor"] = json!(1001);
        p["content"] = json!(x.to_string());
        assert!(handle(&mut c, "attribution.apply", p).is_err());
        assert_eq!(count(&c), 1);
    }
    // Missing expected-hash binding or cap ordering permits stale saves or blocks replay.
    #[test]
    fn attribution_apply_replay_stale_cross_brand() {
        let (mut c, b, product) = fixture();
        let x = exchange();
        let mut p = payload(&b, &x);
        let v = handle(&mut c, "attribution.preview", p.clone()).unwrap();
        p["expectedHash"] = v["contentHash"].clone();
        let mut changed = product.clone();
        changed["revision"] = json!(2);
        update(&c, "m_products", &changed).unwrap();
        assert!(handle(&mut c, "attribution.apply", p).is_err());
        assert_eq!(count(&c), 0);
        let first = save(&mut c, &b, &x);
        let second = save(&mut c, &b, &x);
        assert_eq!(first["datasetId"], second["datasetId"]);
        assert_eq!(second["alreadyImported"], true);
        let other = handle(&mut c, "brand.create", json!({"name":"另一虚构品牌"})).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(load_dataset(&c, &other, first["datasetId"].as_str().unwrap()).is_err());
        assert!(handle(&mut c, "attribution.preview", payload(&other, &x)).is_err());
        for n in 1..50 {
            let mut next = x.clone();
            next["sourceLabel"] = json!(format!("虚构演练{n}"));
            save(&mut c, &b, &next);
        }
        assert_eq!(count(&c), 50);
        assert_eq!(
            handle(&mut c, "attribution.list", json!({"brandId":b})).unwrap()["datasets"]
                .as_array()
                .unwrap()
                .len(),
            50
        );
        assert_eq!(save(&mut c, &b, &x)["alreadyImported"], true);
        let mut next = x.clone();
        next["sourceLabel"] = json!("第51份");
        let mut p = payload(&b, &next);
        p["expectedHash"] =
            handle(&mut c, "attribution.preview", p.clone()).unwrap()["contentHash"].clone();
        assert!(handle(&mut c, "attribution.apply", p).is_err());
        assert_eq!(count(&c), 50);
        changed["sku"] = json!("RENAMED");
        changed["name"] = json!("后来名称");
        c.execute(
            "UPDATE m_products SET sku='RENAMED',data=?1 WHERE id=?2",
            params![changed.to_string(), product["id"].as_str()],
        )
        .unwrap();
        let d = load_dataset(&c, &b, first["datasetId"].as_str().unwrap()).unwrap();
        assert_eq!(d["products"][0]["sku"], "SIM-1");
        assert!(validate_database(&c).is_ok());
    }
    // Missing raw/shape/type/time gates silently changes imported identities or money.
    #[test]
    fn attribution_input_limits_keys_money_dates() {
        let (mut c, b, _) = fixture();
        let valid = exchange();
        assert!(handle(&mut c, "attribution.preview", payload(&b, &valid)).is_ok());
        let mut cases = vec![];
        for (field, value) in [
            ("amountMinor", json!(1.0)),
            ("amountMinor", json!(true)),
            ("amountMinor", json!(0)),
            ("amountMinor", json!(1_000_000_001)),
            ("sku", json!("NO-SKU")),
            ("sessionId", json!("a@b.com")),
            ("occurredAt", json!("2026-10-03T00:00:00.000Z")),
            ("occurredAt", json!("2026-11-01T00:00:00Z")),
        ] {
            let mut x = valid.clone();
            x["orders"][0][field] = value;
            cases.push(x);
        }
        for (field, value) in [
            ("orderId", json!("missing")),
            ("occurredAt", json!("2026-10-02T00:00:00Z")),
            ("occurredAt", json!("2026-11-01T00:00:01Z")),
        ] {
            let mut x = valid.clone();
            x["refunds"][0][field] = value;
            cases.push(x);
        }
        for field in ["email", "url", "ip", "deviceId", "name"] {
            let mut x = valid.clone();
            x["visits"][0][field] = json!("PRIVATE_MARKER");
            cases.push(x);
        }
        let mut x = valid.clone();
        x["visits"][0]["sourceEvidence"] = json!("unknown");
        cases.push(x);
        let mut x = valid.clone();
        x["orders"][0]["status"] = json!("cancelled");
        cases.push(x);
        let mut x = valid.clone();
        x["visits"] = json!(vec![x["visits"][0].clone(); 1001]);
        x["orders"] = json!([]);
        x["refunds"] = json!([]);
        cases.push(x);
        let mut x = valid.clone();
        x["coverageEnd"] = json!("2027-10-02T00:00:01Z");
        cases.push(x);
        let mut x = valid.clone();
        x["coverageEnd"] = x["coverageStart"].clone();
        cases.push(x);
        let mut x = valid.clone();
        x["visits"] = json!([]);
        x["orders"] = json!([]);
        x["refunds"] = json!([]);
        cases.push(x);
        let mut x = valid.clone();
        x["email"] = json!("PRIVATE_MARKER");
        cases.push(x);
        for x in cases {
            let err = handle(&mut c, "attribution.preview", payload(&b, &x)).unwrap_err();
            assert!(!err.contains("PRIVATE_MARKER"));
        }
        let raw = valid.to_string();
        for content in [
            raw.replacen("{", "{\"format\":\"m-geo-attribution-v1\",", 1),
            raw.replacen(
                "\"sessionId\":\"session-1\"",
                "\"sessionId\":\"session-1\",\"sessionId\":\"session-1\"",
                1,
            ),
            format!("{raw}{}", " ".repeat(512 * 1024)),
        ] {
            assert!(handle(
                &mut c,
                "attribution.preview",
                json!({"brandId":b,"name":"虚构","content":content})
            )
            .is_err());
        }
        assert_eq!(count(&c), 0);
        let raw = format!("{raw}{}", " ".repeat(512 * 1024 - raw.len()));
        assert_eq!(raw.len(), 512 * 1024);
        assert!(handle(
            &mut c,
            "attribution.preview",
            json!({"brandId":b,"name":"虚构","content":raw})
        )
        .is_ok());
        let mut boundary = valid.clone();
        boundary["coverageEnd"] = json!("2027-10-02T00:00:00Z");
        boundary["orders"][0]["amountMinor"] = json!(1_000_000_000);
        assert!(handle(&mut c, "attribution.preview", payload(&b, &boundary)).is_ok());
        let mut boundary = valid.clone();
        boundary["visits"] = json!(vec![boundary["visits"][0].clone(); 1000]);
        boundary["orders"] = json!([]);
        boundary["refunds"] = json!([]);
        assert_eq!(
            handle(&mut c, "attribution.preview", payload(&b, &boundary)).unwrap()["counts"],
            json!({"visits":1,"orders":0,"refunds":0,"duplicates":999})
        );
        let simulated_hash = handle(&mut c, "attribution.preview", payload(&b, &valid)).unwrap()
            ["contentHash"]
            .clone();
        let mut recorded = valid;
        recorded["evidenceMode"] = json!("recorded");
        assert_ne!(
            handle(&mut c, "attribution.preview", payload(&b, &recorded)).unwrap()["contentHash"],
            simulated_hash
        );
        recorded["orders"][0]["sessionId"] = Value::Null;
        assert!(handle(&mut c, "attribution.preview", payload(&b, &recorded)).is_ok());
    }
    // Missing read validation trusts edited hashes, snapshots, shapes or evidence modes.
    #[test]
    fn attribution_stored_hash_snapshot_and_mode_validation() {
        let (mut c, b, _) = fixture();
        let saved = save(&mut c, &b, &exchange());
        let key = saved["datasetId"].as_str().unwrap();
        let original = load_dataset(&c, &b, key).unwrap();
        assert!(validate_database(&c).is_ok());
        let listed = handle(&mut c, "attribution.list", json!({"brandId":b})).unwrap();
        assert_eq!(listed["datasets"].as_array().unwrap().len(), 1);
        let mut corruptions = vec![];
        let mut v = original.clone();
        v["contentHash"] = json!("0".repeat(64));
        corruptions.push(v);
        let mut v = original.clone();
        v["input"]["evidenceMode"] = json!("unverified");
        corruptions.push(v);
        let mut v = original.clone();
        let snapshot = v["products"][0].clone();
        v["products"].as_array_mut().unwrap().push(snapshot);
        corruptions.push(v);
        let mut v = original.clone();
        v["products"][0]["id"] = json!(Uuid::new_v4().to_string());
        corruptions.push(v);
        let mut v = original.clone();
        v["products"][0]["privateNotes"] = json!("PRIVATE_MARKER");
        corruptions.push(v);
        let mut v = original.clone();
        v["input"]["visits"][0]["occurredAt"] = json!("2026-10-02T08:00:00+08:00");
        corruptions.push(v);
        let mut v = original.clone();
        let row = v["input"]["visits"][0].clone();
        v["input"]["visits"].as_array_mut().unwrap().push(row);
        corruptions.push(v);
        let mut v = original.clone();
        v["brandId"] = json!(Uuid::new_v4().to_string());
        corruptions.push(v);
        let mut v = original.clone();
        v["products"][0]["revision"] = json!(0);
        corruptions.push(v);
        for (index, mut v) in corruptions.into_iter().enumerate() {
            if index > 0 {
                v["contentHash"] = json!(fingerprint(
                    &json!({"input":v["input"],"products":v["products"]})
                ));
            }
            c.execute(
                "UPDATE m_attribution_datasets SET data=?1,content_hash=?3 WHERE id=?2",
                params![v.to_string(), key, v["contentHash"].as_str()],
            )
            .unwrap();
            assert!(load_dataset(&c, &b, key).is_err());
            assert!(validate_database(&c).is_err());
            assert!(handle(&mut c, "attribution.list", json!({"brandId":b})).is_err());
        }
        c.execute(
            "UPDATE m_attribution_datasets SET data=?1,content_hash=?3 WHERE id=?2",
            params![original.to_string(), key, original["contentHash"].as_str()],
        )
        .unwrap();
        assert!(validate_database(&c).is_ok());
        c.execute(
            "UPDATE m_attribution_datasets SET content_hash=?1 WHERE id=?2",
            params!["f".repeat(64), key],
        )
        .unwrap();
        assert!(load_dataset(&c, &b, key).is_err());
        assert!(validate_database(&c).is_err());
        c.execute(
            "UPDATE m_attribution_datasets SET content_hash=?1 WHERE id=?2",
            params![original["contentHash"].as_str(), key],
        )
        .unwrap();
        for n in 1..51 {
            let mut v = original.clone();
            v["id"] = json!(Uuid::new_v4().to_string());
            v["input"]["sourceLabel"] = json!(format!("虚构超限{n}"));
            v["contentHash"] = json!(fingerprint(
                &json!({"input":v["input"],"products":v["products"]})
            ));
            c.execute("INSERT INTO m_attribution_datasets(id,brand_id,content_hash,data)VALUES(?1,?2,?3,?4)",params![v["id"].as_str(),b,v["contentHash"].as_str(),v.to_string()]).unwrap();
        }
        assert_eq!(count(&c), 51);
        assert!(validate_database(&c).is_err());
        assert!(handle(&mut c, "attribution.list", json!({"brandId":b})).is_err());
        assert_eq!(count(&c), 51);
    }
}
