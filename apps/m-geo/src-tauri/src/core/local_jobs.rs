//! Durable, manually advanced local attribution imports. The caller owns the transaction.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

pub(super) const TABLE_SQL: &str = "CREATE TABLE IF NOT EXISTS m_local_jobs(id TEXT PRIMARY KEY,brand_id TEXT NOT NULL,content_hash TEXT NOT NULL,data TEXT NOT NULL,UNIQUE(brand_id,content_hash),FOREIGN KEY(brand_id) REFERENCES m_brands(id));";
const INVALID: &str = "本地导入队列资料无效，请检查队列与备份";
const DOMAIN_ERRORS: [&str; 3] = [
    "预览已失效，请重新预览归因资料",
    "每个品牌最多保存50份归因数据集",
    "归因资料格式无效，请检查字段、时间、金额与引用",
];
const JOB_BYTES: usize = 2_100_000;

fn exact(v: &Value, keys: &[&str]) -> Result<()> {
    let o = v.as_object().ok_or(INVALID)?;
    if o.len() != keys.len() || keys.iter().any(|k| !o.contains_key(*k)) {
        return Err(INVALID.into());
    }
    Ok(())
}
fn name(v: &Value) -> Result<String> {
    let s = v["name"].as_str().ok_or(INVALID)?;
    if s.trim().is_empty() || s.chars().count() > 100 || s.chars().any(char::is_control) {
        return Err(INVALID.into());
    }
    Ok(s.to_owned())
}
fn hash(v: &Value, k: &str) -> Result<String> {
    let s = v[k].as_str().ok_or(INVALID)?;
    if s.len() != 64
        || !s
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    {
        return Err(INVALID.into());
    }
    Ok(s.to_owned())
}
fn canonical_time(v: &Value, k: &str) -> Result<i64> {
    let s = v[k].as_str().ok_or(INVALID)?;
    if s.len() > 80 || s.contains('.') {
        return Err(INVALID.into());
    }
    let t = DateTime::parse_from_rfc3339(s).map_err(|_| INVALID)?;
    if t.timestamp_subsec_nanos() != 0
        || t.with_timezone(&Utc)
            .to_rfc3339_opts(SecondsFormat::Secs, true)
            != s
    {
        return Err(INVALID.into());
    }
    Ok(t.timestamp())
}
fn request(c: &Connection, b: &str, p: &Value, keys: &[&str]) -> Result<()> {
    exact(p, keys)?;
    if p["brandId"] != b {
        return Err(INVALID.into());
    }
    brand(c, b)?;
    Ok(())
}
fn canonical_entries(entries: &[Value]) -> Value {
    json!(entries
        .iter()
        .map(|e| json!({"name":e["name"],"content":e["content"],"expectedHash":e["expectedHash"]}))
        .collect::<Vec<_>>())
}
fn job_hash(name: &str, entries: &[Value]) -> String {
    fingerprint(&json!({"name":name,"entries":canonical_entries(entries)}))
}
fn entry_count(v: &Value, status: &str) -> usize {
    v["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["status"] == status)
        .count()
}
fn summary(v: &Value) -> Value {
    json!({"id":v["id"],"name":v["name"],"status":v["status"],"revision":v["revision"],"createdAt":v["createdAt"],"updatedAt":v["updatedAt"],"total":v["entries"].as_array().unwrap().len(),"succeeded":entry_count(v,"succeeded"),"failed":entry_count(v,"failed"),"pending":entry_count(v,"pending"),"cancelled":entry_count(v,"cancelled")})
}
fn stored(c: &Connection, key: &str, b: &str, h: &str, data: &str) -> Result<Value> {
    if data.len() > JOB_BYTES {
        return Err(INVALID.into());
    }
    let v: Value = serde_json::from_str(data).map_err(|_| INVALID)?;
    // All writes are canonical JSON; this also rejects duplicate members rather than silently trusting their last value.
    let canonical_data = v.to_string();
    if canonical_data != data {
        return Err(INVALID.into());
    }
    exact(
        &v,
        &[
            "id",
            "brandId",
            "name",
            "contentHash",
            "createdAt",
            "updatedAt",
            "revision",
            "status",
            "entries",
        ],
    )?;
    if v["id"] != key || v["brandId"] != b || v["contentHash"] != h {
        return Err(INVALID.into());
    }
    Uuid::parse_str(key).map_err(|_| INVALID)?;
    Uuid::parse_str(b).map_err(|_| INVALID)?;
    brand(c, b)?;
    let count: i64 = c
        .query_row(
            "SELECT count(*) FROM m_local_jobs WHERE brand_id=?1",
            [b],
            |r| r.get(0),
        )
        .map_err(db_error)?;
    if count > 50 {
        return Err(INVALID.into());
    }
    let label = name(&v)?;
    hash(&v, "contentHash")?;
    let created = canonical_time(&v, "createdAt")?;
    if canonical_time(&v, "updatedAt")? < created
        || v["revision"].as_i64().filter(|x| *x > 0).is_none()
    {
        return Err(INVALID.into());
    }
    let entries = v["entries"]
        .as_array()
        .filter(|x| !x.is_empty() && x.len() <= 20)
        .ok_or(INVALID)?;
    let mut ids = BTreeSet::new();
    for e in entries {
        exact(
            e,
            &[
                "id",
                "name",
                "content",
                "expectedHash",
                "products",
                "status",
                "attempts",
                "datasetId",
                "alreadyImported",
                "error",
            ],
        )?;
        let eid = e["id"].as_str().ok_or(INVALID)?;
        Uuid::parse_str(eid).map_err(|_| INVALID)?;
        if !ids.insert(eid) {
            return Err(INVALID.into());
        }
        name(e)?;
        let expected = hash(e, "expectedHash")?;
        let content = e["content"].as_str().ok_or(INVALID)?;
        let input = attribution::normalize_exchange(content)?;
        let canonical_content = input.to_string();
        if canonical_content != content {
            return Err(INVALID.into());
        }
        let candidate = json!({"id":eid,"brandId":b,"name":e["name"],"createdAt":v["createdAt"],"contentHash":expected,"input":input,"products":e["products"]});
        attribution::validate_frozen_dataset(c, b, &candidate)?;
        let attempts = e["attempts"].as_u64().filter(|x| *x <= 5).ok_or(INVALID)?;
        let replay = e["alreadyImported"].as_bool().ok_or(INVALID)?;
        match e["status"].as_str().ok_or(INVALID)? {
            "pending" | "cancelled" => {
                if attempts > 4 || !e["datasetId"].is_null() || !e["error"].is_null() || replay {
                    return Err(INVALID.into());
                }
            }
            "failed" => {
                if attempts == 0
                    || !e["datasetId"].is_null()
                    || replay
                    || !DOMAIN_ERRORS.contains(&e["error"].as_str().ok_or(INVALID)?)
                {
                    return Err(INVALID.into());
                }
            }
            "succeeded" => {
                if attempts == 0 || !e["error"].is_null() {
                    return Err(INVALID.into());
                }
                let dataset =
                    attribution::load_dataset(c, b, e["datasetId"].as_str().ok_or(INVALID)?)?;
                if dataset["contentHash"] != expected {
                    return Err(INVALID.into());
                }
            }
            _ => return Err(INVALID.into()),
        }
    }
    if job_hash(&label, entries) != h {
        return Err(INVALID.into());
    }
    let pending = entry_count(&v, "pending");
    let cancelled = entry_count(&v, "cancelled");
    match v["status"].as_str().ok_or(INVALID)? {
        "paused" | "ready" if pending > 0 && cancelled == 0 => {}
        "completed" if pending == 0 && cancelled == 0 => {}
        "cancelled" if pending == 0 && cancelled > 0 => {}
        _ => return Err(INVALID.into()),
    }
    Ok(v)
}
fn load(c: &Connection, b: &str, key: &str) -> Result<Value> {
    Uuid::parse_str(key).map_err(|_| INVALID)?;
    let row: Option<(String, String)> = c
        .query_row(
            "SELECT content_hash,data FROM m_local_jobs WHERE id=?1 AND brand_id=?2",
            params![key, b],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(db_error)?;
    let (h, data) = row.ok_or("记录不存在或不属于当前品牌")?;
    stored(c, key, b, &h, &data)
}
fn mutation(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    request(c, b, p, &["brandId", "jobId", "expectedRevision"])?;
    let v = load(c, b, p["jobId"].as_str().ok_or(INVALID)?)?;
    revision(&v, p)?;
    Ok(v)
}
fn persist(c: &Connection, b: &str, mut v: Value) -> Result<Value> {
    let rev = v["revision"]
        .as_i64()
        .ok_or(INVALID)?
        .checked_add(1)
        .ok_or(INVALID)?;
    v["revision"] = json!(rev);
    v["updatedAt"] = json!(now());
    let key = v["id"].as_str().ok_or(INVALID)?;
    let h = v["contentHash"].as_str().ok_or(INVALID)?;
    let data = v.to_string();
    stored(c, key, b, h, &data)?;
    if c.execute(
        "UPDATE m_local_jobs SET data=?1 WHERE id=?2 AND brand_id=?3 AND content_hash=?4",
        params![data, key, b, h],
    )
    .map_err(db_error)?
        != 1
    {
        return Err(INVALID.into());
    }
    Ok(v)
}
// Creation reserves an upper bound for every future mutable field. This
// projection deliberately combines success and failure fields: it need not be a
// valid state, but must be at least as large as every valid checkpoint state.
fn ensure_outcome_capacity(v: &Value) -> Result<()> {
    let mut maximum = v.clone();
    maximum["status"] = json!("completed");
    maximum["revision"] = json!(i64::MAX);
    maximum["updatedAt"] =
        json!(DateTime::<Utc>::MAX_UTC.to_rfc3339_opts(SecondsFormat::Secs, true));
    let longest_error = DOMAIN_ERRORS
        .iter()
        .max_by_key(|error| json!(error).to_string().len())
        .ok_or(INVALID)?;
    for entry in maximum["entries"].as_array_mut().ok_or(INVALID)? {
        entry["status"] = json!("succeeded");
        entry["attempts"] = json!(5);
        // Historical UUID validation also permits the longer URN spelling.
        entry["datasetId"] = json!(format!("urn:uuid:{}", Uuid::nil()));
        entry["alreadyImported"] = json!(false);
        entry["error"] = json!(longest_error);
    }
    if maximum.to_string().len() > JOB_BYTES {
        return Err("队列过大，无法预留处理结果空间，请减少条目后重试".into());
    }
    Ok(())
}

pub(super) fn create(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    request(c, b, p, &["brandId", "name", "entries"])?;
    let label = name(p)?;
    if p["entries"].to_string().len() > 1_500_000 {
        return Err(INVALID.into());
    }
    let raw = p["entries"]
        .as_array()
        .filter(|x| !x.is_empty() && x.len() <= 20)
        .ok_or(INVALID)?;
    let mut entries = Vec::new();
    for e in raw {
        exact(e, &["name", "content", "expectedHash"])?;
        let label = name(e)?;
        let expected = hash(e, "expectedHash")?;
        let input = attribution::normalize_exchange(e["content"].as_str().ok_or(INVALID)?)?;
        entries.push(json!({"name":label,"content":input.to_string(),"expectedHash":expected}));
    }
    let h = job_hash(&label, &entries);
    // Historical replay precedes today's catalog preview and the current queue limit.
    let row: Option<(String, String)> = c
        .query_row(
            "SELECT id,data FROM m_local_jobs WHERE brand_id=?1 AND content_hash=?2",
            params![b, h],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(db_error)?;
    if let Some((key, data)) = row {
        let mut v = stored(c, &key, b, &h, &data)?;
        v["alreadyExists"] = json!(true);
        return Ok(v);
    }
    let count: i64 = c
        .query_row(
            "SELECT count(*) FROM m_local_jobs WHERE brand_id=?1",
            [b],
            |r| r.get(0),
        )
        .map_err(db_error)?;
    if count >= 50 {
        return Err("每个品牌最多保存50份本地导入队列".into());
    }
    for e in &mut entries {
        let preview = attribution::preview(
            c,
            b,
            &json!({"brandId":b,"name":e["name"],"content":e["content"]}),
        )?;
        if preview["contentHash"] != e["expectedHash"] {
            return Err(DOMAIN_ERRORS[0].into());
        }
        e["id"] = json!(id());
        e["products"] = preview["products"].clone();
        e["status"] = json!("pending");
        e["attempts"] = json!(0);
        e["datasetId"] = Value::Null;
        e["alreadyImported"] = json!(false);
        e["error"] = Value::Null;
    }
    let key = id();
    let at = now();
    let mut v = json!({"id":key,"brandId":b,"name":label,"contentHash":h,"createdAt":at,"updatedAt":at,"revision":1,"status":"paused","entries":entries});
    let data = v.to_string();
    stored(c, &key, b, &h, &data)?;
    ensure_outcome_capacity(&v)?;
    if c.execute(
        "INSERT INTO m_local_jobs(id,brand_id,content_hash,data)VALUES(?1,?2,?3,?4)",
        params![key, b, h, data],
    )
    .map_err(db_error)?
        != 1
    {
        return Err(INVALID.into());
    }
    v["alreadyExists"] = json!(false);
    Ok(v)
}
pub(super) fn get(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    request(c, b, p, &["brandId", "jobId"])?;
    load(c, b, p["jobId"].as_str().ok_or(INVALID)?)
}
pub(super) fn list(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    request(c, b, p, &["brandId"])?;
    let mut q = c
        .prepare("SELECT id,content_hash,data FROM m_local_jobs WHERE brand_id=?1")
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
    let mut jobs = vec![];
    for row in rows {
        let (key, h, data) = row.map_err(db_error)?;
        jobs.push(summary(&stored(c, &key, b, &h, &data)?));
        if jobs.len() > 50 {
            return Err(INVALID.into());
        }
    }
    jobs.sort_by(|a, b| {
        b["createdAt"]
            .as_str()
            .cmp(&a["createdAt"].as_str())
            .then_with(|| b["id"].as_str().cmp(&a["id"].as_str()))
    });
    Ok(json!({"jobs":jobs}))
}
pub(super) fn resume(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    let mut v = mutation(c, b, p)?;
    match v["status"].as_str() {
        Some("ready") => return Ok(v),
        Some("paused") => {}
        _ => return Err("当前队列不能继续".into()),
    };
    v["status"] = json!("ready");
    persist(c, b, v)
}
pub(super) fn pause(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    let mut v = mutation(c, b, p)?;
    match v["status"].as_str() {
        Some("paused") => return Ok(v),
        Some("ready") => {}
        _ => return Err("当前队列不能暂停".into()),
    };
    v["status"] = json!("paused");
    persist(c, b, v)
}
pub(super) fn step(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    let mut v = mutation(c, b, p)?;
    if v["status"] != "ready" {
        return Err("请先手动继续队列".into());
    }
    let index = v["entries"]
        .as_array()
        .unwrap()
        .iter()
        .position(|e| e["status"] == "pending")
        .ok_or(INVALID)?;
    let e = v["entries"][index].clone();
    c.execute_batch("SAVEPOINT local_job_item")
        .map_err(db_error)?;
    let applied = attribution::apply(
        c,
        b,
        &json!({"brandId":b,"name":e["name"],"content":e["content"],"expectedHash":e["expectedHash"]}),
    );
    let result = match applied {
        Ok(out) => {
            // Audit failure is infrastructure failure: never convert it into a consumed attempt.
            if c.execute("INSERT INTO m_audit(occurred_at,action,brand_id,object_id)VALUES(?1,'attribution.apply',?2,?3)",params![now(),b,out["datasetId"].as_str()]).map_err(db_error)? != 1 {
                return Err(INVALID.into());
            }
            c.execute_batch("RELEASE local_job_item")
                .map_err(db_error)?;
            Ok(out)
        }
        Err(error) => {
            c.execute_batch("ROLLBACK TO local_job_item; RELEASE local_job_item")
                .map_err(db_error)?;
            if !DOMAIN_ERRORS.contains(&error.as_str()) {
                return Err(error);
            }
            Err(error)
        }
    };
    let entry = &mut v["entries"][index];
    entry["attempts"] = json!(e["attempts"].as_u64().unwrap() + 1);
    match result {
        Ok(out) => {
            entry["status"] = json!("succeeded");
            entry["datasetId"] = out["datasetId"].clone();
            entry["alreadyImported"] = out["alreadyImported"].clone();
        }
        Err(error) => {
            entry["status"] = json!("failed");
            entry["error"] = json!(error);
        }
    }
    if entry_count(&v, "pending") == 0 {
        v["status"] = json!("completed");
    }
    persist(c, b, v)
}
pub(super) fn retry(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    let mut v = mutation(c, b, p)?;
    if v["status"] != "paused" && v["status"] != "completed" {
        return Err("当前队列不能重试".into());
    }
    let mut changed = false;
    for e in v["entries"].as_array_mut().unwrap() {
        if e["status"] == "failed" && e["attempts"].as_u64().unwrap() < 5 {
            e["status"] = json!("pending");
            e["error"] = Value::Null;
            changed = true;
        }
    }
    if !changed {
        return Err("没有可重试的失败项，最多尝试5次".into());
    }
    v["status"] = json!("paused");
    persist(c, b, v)
}
pub(super) fn cancel(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    let mut v = mutation(c, b, p)?;
    if v["status"] == "cancelled" {
        return Ok(v);
    }
    if v["status"] != "paused" && v["status"] != "ready" {
        return Err("当前队列不能取消".into());
    }
    for e in v["entries"].as_array_mut().unwrap() {
        if e["status"] == "pending" {
            e["status"] = json!("cancelled");
        }
    }
    v["status"] = json!("cancelled");
    persist(c, b, v)
}
pub(super) fn prepare_export(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    let v = get(c, b, p)?;
    let mut s = summary(&v);
    s["localOnly"] = json!(true);
    s["modelCalls"] = json!(0);
    s["modelFeeMinor"] = json!(0);
    Ok(
        json!({"files":[{"path":"job.json","content":v.to_string()},{"path":"summary.json","content":s.to_string()},{"path":"methodology.txt","content":"本地导入队列：每份数据集独立保留，不汇总重叠指标，不推断因果提升。成功导入的结果在暂停、取消或失败后保留；取消只停止剩余待处理项。队列需用户手动开始或继续，重启不会自动执行。失败项需显式重试，最多尝试5次；冻结预览失效时需重新预览并新建队列。仅执行本地校验与SQLite写入，模型调用0次，本地模型费用0。\n"}]}),
    )
}
pub(super) fn validate_database(c: &Connection) -> Result<()> {
    let mut q = c
        .prepare("SELECT id,brand_id,content_hash,data FROM m_local_jobs")
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
    for row in rows {
        let (key, b, h, data) = row.map_err(db_error)?;
        stored(c, &key, &b, &h, &data)?;
        let n = counts.entry(b).or_default();
        *n += 1;
        if *n > 50 {
            return Err(INVALID.into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Connection, String, Value) {
        let mut c = crate::runtime::open_database(std::path::Path::new(":memory:")).unwrap();
        let b = handle(&mut c, "brand.create", json!({"name":"虚构批量运营"})).unwrap()["id"]
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
    fn exchange(label: &str) -> Value {
        json!({"format":"m-geo-attribution-v1","evidenceMode":"simulated","sourceLabel":label,"coverageStart":"2026-10-01T00:00:00Z","coverageEnd":"2026-11-01T00:00:00Z","currency":"CNY","visits":[],"orders":[{"id":"o1","sessionId":null,"sku":"SIM-1","occurredAt":"2026-10-03T00:00:00Z","status":"paid","amountMinor":1000}],"refunds":[]})
    }
    fn request(c: &Connection, b: &str, name: &str, n: usize) -> Value {
        let entries: Vec<Value> = (0..n)
            .map(|i| {
                let input = exchange(&format!("虚构{name}-{i}"));
                let p = json!({"brandId":b,"name":"虚构数据集","content":input.to_string()});
                let hash = attribution::preview(c, b, &p).unwrap()["contentHash"].clone();
                json!({"name":"虚构数据集","content":input.to_string(),"expectedHash":hash})
            })
            .collect();
        json!({"brandId":b,"name":name,"entries":entries})
    }
    type Action = fn(&Connection, &str, &Value) -> Result<Value>;
    fn call(c: &mut Connection, b: &str, action: Action, p: Value) -> Result<Value> {
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_error)?;
        let out = action(&tx, b, &p)?;
        tx.commit().map_err(db_error)?;
        Ok(out)
    }
    fn mutation(c: &mut Connection, b: &str, action: Action, j: &Value) -> Result<Value> {
        call(
            c,
            b,
            action,
            json!({"brandId":b,"jobId":j["id"],"expectedRevision":j["revision"]}),
        )
    }
    fn reload(c: &Connection, b: &str, j: &Value) -> Value {
        get(c, b, &json!({"brandId":b,"jobId":j["id"]})).unwrap()
    }
    fn count(c: &Connection, table: &str) -> i64 {
        c.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }
    fn edit_product(c: &Connection, p: &Value, rev: i64) {
        let mut v = p.clone();
        v["revision"] = json!(rev);
        update(c, "m_products", &v).unwrap();
    }

    #[test]
    fn local_jobs_pause_reopen_resume_cancel() {
        let (mut c, b, _) = fixture();
        let p = request(&c, &b, "三份独立数据", 3);
        let mut j = call(&mut c, &b, create, p).unwrap();
        assert_eq!(j["status"], "paused");
        assert_eq!(j["revision"], 1);
        assert_eq!(count(&c, "m_attribution_datasets"), 0);
        j = mutation(&mut c, &b, resume, &j).unwrap();
        j = mutation(&mut c, &b, step, &j).unwrap();
        assert_eq!(count(&c, "m_attribution_datasets"), 1);
        j = mutation(&mut c, &b, pause, &j).unwrap();
        let bytes = crate::backup::encrypt_snapshot(&c, "fictional-queue-password").unwrap();
        let mut restored = crate::runtime::open_database(std::path::Path::new(":memory:")).unwrap();
        crate::backup::restore_snapshot(&mut restored, &bytes, "fictional-queue-password").unwrap();
        assert_eq!(reload(&restored, &b, &j), j);
        assert_eq!(count(&restored, "m_attribution_datasets"), 1);
        j = mutation(&mut restored, &b, resume, &j).unwrap();
        j = mutation(&mut restored, &b, step, &j).unwrap();
        j = mutation(&mut restored, &b, cancel, &j).unwrap();
        assert_eq!(j["status"], "cancelled");
        assert_eq!(j["entries"][0]["status"], "succeeded");
        assert_eq!(j["entries"][2]["status"], "cancelled");
        assert_eq!(count(&restored, "m_attribution_datasets"), 2);
        assert!(mutation(&mut restored, &b, resume, &j).is_err());
        assert!(mutation(&mut restored, &b, retry, &j).is_err());
        assert_eq!(mutation(&mut restored, &b, cancel, &j).unwrap(), j);
        validate_database(&restored).unwrap();
    }

    #[test]
    fn local_jobs_atomic_fault_retry_lost_reply() {
        let (mut c, b, product) = fixture();
        let p = request(&c, &b, "故障回滚", 2);
        c.execute_batch("CREATE TRIGGER create_fault BEFORE INSERT ON m_local_jobs BEGIN SELECT RAISE(IGNORE); END;").unwrap();
        assert!(call(&mut c, &b, create, p.clone()).is_err());
        assert_eq!(count(&c, "m_local_jobs"), 0);
        c.execute_batch("DROP TRIGGER create_fault").unwrap();
        let mut j = call(&mut c, &b, create, p).unwrap();
        j = mutation(&mut c, &b, resume, &j).unwrap();
        for sql in ["CREATE TRIGGER inject BEFORE INSERT ON m_attribution_datasets BEGIN SELECT RAISE(ABORT,'fault'); END;", "CREATE TRIGGER inject BEFORE INSERT ON m_audit WHEN NEW.action='attribution.apply' BEGIN SELECT RAISE(ABORT,'fault'); END;", "CREATE TRIGGER inject BEFORE UPDATE ON m_local_jobs BEGIN SELECT RAISE(ABORT,'fault'); END;", "CREATE TRIGGER inject BEFORE INSERT ON m_audit WHEN NEW.action='attribution.apply' BEGIN SELECT RAISE(IGNORE); END;"] {
            c.execute_batch(sql).unwrap();let before=count(&c,"m_audit");assert!(mutation(&mut c,&b,step,&j).is_err());assert_eq!(reload(&c,&b,&j),j);assert_eq!(count(&c,"m_attribution_datasets"),0);assert_eq!(count(&c,"m_audit"),before);c.execute_batch("DROP TRIGGER inject").unwrap();
        }
        c.execute_batch("CREATE TABLE deferred_fault(brand_id TEXT REFERENCES m_brands(id) DEFERRABLE INITIALLY DEFERRED); CREATE TRIGGER commit_fault AFTER INSERT ON m_attribution_datasets BEGIN INSERT INTO deferred_fault VALUES('missing-brand'); END;").unwrap();
        let audits = count(&c, "m_audit");
        assert!(mutation(&mut c, &b, step, &j).is_err());
        assert_eq!(reload(&c, &b, &j), j);
        assert_eq!(count(&c, "m_attribution_datasets"), 0);
        assert_eq!(count(&c, "m_audit"), audits);
        c.execute_batch("DROP TRIGGER commit_fault; DROP TABLE deferred_fault;")
            .unwrap();
        j = mutation(&mut c, &b, pause, &j).unwrap();
        j = mutation(&mut c, &b, resume, &j).unwrap();
        let old = j.clone();
        let committed = mutation(&mut c, &b, step, &j).unwrap();
        assert_eq!(reload(&c, &b, &j), committed);
        assert!(mutation(&mut c, &b, step, &old).is_err());
        j = committed;
        assert_eq!(count(&c, "m_attribution_datasets"), 1);
        assert_eq!(j["entries"][0]["attempts"], 1);
        edit_product(&c, &product, 2);
        j = mutation(&mut c, &b, step, &j).unwrap();
        assert_eq!(j["status"], "completed");
        assert_eq!(j["entries"][1]["status"], "failed");
        assert_eq!(j["entries"][1]["attempts"], 1);
        edit_product(&c, &product, 1);
        j = mutation(&mut c, &b, retry, &j).unwrap();
        assert_eq!(j["status"], "paused");
        j = mutation(&mut c, &b, resume, &j).unwrap();
        j = mutation(&mut c, &b, step, &j).unwrap();
        assert_eq!(j["entries"][1]["attempts"], 2);
        assert_eq!(count(&c, "m_attribution_datasets"), 2);
        assert_eq!(j["entries"][0]["attempts"], 1);
    }

    #[test]
    fn local_jobs_limits_replay_revision_scope() {
        let (mut c, b, product) = fixture();
        let p = request(&c, &b, "原始队列", 1);
        let original = call(&mut c, &b, create, p.clone()).unwrap();
        let mut equivalent = p.clone();
        let mut input = exchange("虚构原始队列-0");
        let duplicate = input["orders"][0].clone();
        input["orders"].as_array_mut().unwrap().push(duplicate);
        equivalent["entries"][0]["content"] = json!(serde_json::to_string_pretty(&input).unwrap());
        assert_eq!(
            call(&mut c, &b, create, equivalent).unwrap()["id"],
            original["id"]
        );
        let entry = p["entries"][0].clone();
        for entries in [json!([]), json!(vec![entry.clone(); 21])] {
            assert!(call(
                &mut c,
                &b,
                create,
                json!({"brandId":b,"name":"非法数量","entries":entries})
            )
            .is_err());
        }
        let mut large = entry.clone();
        let content = large["content"].as_str().unwrap().to_owned();
        large["content"] = json!(format!("{content}{}", " ".repeat(400_000)));
        assert!(call(
            &mut c,
            &b,
            create,
            json!({"brandId":b,"name":"超出原始体积","entries":vec![large;4]})
        )
        .is_err());
        let mut malformed = p.clone();
        malformed["entries"][0]["expectedHash"] = json!("Z".repeat(64));
        assert!(call(&mut c, &b, create, malformed).is_err());
        let mut stale = request(&c, &b, "全有或全无", 2);
        stale["entries"][1]["expectedHash"] = json!("0".repeat(64));
        assert!(call(&mut c, &b, create, stale).is_err());
        assert_eq!(count(&c, "m_local_jobs"), 1);
        let mut j = original.clone();
        edit_product(&c, &product, 2);
        for attempt in 1..=5 {
            j = mutation(&mut c, &b, resume, &j).unwrap();
            j = mutation(&mut c, &b, step, &j).unwrap();
            assert_eq!(j["entries"][0]["attempts"], attempt);
            if attempt < 5 {
                j = mutation(&mut c, &b, retry, &j).unwrap();
            }
        }
        assert!(mutation(&mut c, &b, retry, &j).is_err());
        assert!(mutation(&mut c, &b, pause, &original).is_err());
        edit_product(&c, &product, 1);
        for n in 1..50 {
            let next = request(&c, &b, &format!("独立队列{n}"), 1);
            call(&mut c, &b, create, next).unwrap();
        }
        assert_eq!(count(&c, "m_local_jobs"), 50);
        edit_product(&c, &product, 2);
        let replay = call(&mut c, &b, create, p).unwrap();
        assert_eq!(replay["id"], j["id"]);
        assert_eq!(replay["alreadyExists"], true);
        let next = request(&c, &b, "第51份", 1);
        assert!(call(&mut c, &b, create, next).is_err());
        let other = handle(&mut c, "brand.create", json!({"name":"另一个虚构品牌"})).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(get(&c, &other, &json!({"brandId":other,"jobId":j["id"]})).is_err());
        assert!(get(&c, &b, &json!({"brandId":b,"jobId":j["id"],"extra":true})).is_err());
        assert!(call(
            &mut c,
            &b,
            create,
            json!({"brandId":b,"name":"空","entries":[]})
        )
        .is_err());
    }

    #[test]
    fn local_jobs_frozen_backup_validation() {
        let (mut c, b, product) = fixture();
        let p = request(&c, &b, "冻结历史", 2);
        let mut j = call(&mut c, &b, create, p.clone()).unwrap();
        j = mutation(&mut c, &b, resume, &j).unwrap();
        j = mutation(&mut c, &b, step, &j).unwrap();
        j = mutation(&mut c, &b, pause, &j).unwrap();
        let mut edit = product.clone();
        edit["name"] = json!("改名后的商品");
        edit["sku"] = json!("RENAMED");
        edit["expectedRevision"] = product["revision"].clone();
        handle(&mut c, "product.save", edit).unwrap();
        assert_eq!(reload(&c, &b, &j), j);
        validate_database(&c).unwrap();
        assert_eq!(call(&mut c, &b, create, p).unwrap()["alreadyExists"], true);
        assert!(prepare_export(&c, &b, &json!({"brandId":b,"jobId":j["id"]})).is_ok());
        j = mutation(&mut c, &b, resume, &j).unwrap();
        j = mutation(&mut c, &b, step, &j).unwrap();
        assert_eq!(j["entries"][1]["status"], "failed");
        assert_eq!(j["entries"][1]["attempts"], 1);
        validate_database(&c).unwrap();
        let mut variants = vec![];
        let mut v = j.clone();
        v["entries"][0]["products"][0]["name"] = json!("改写快照");
        variants.push(v);
        let mut v = j.clone();
        v["entries"][0]["datasetId"] = json!(id());
        variants.push(v);
        let mut v = j.clone();
        v["entries"][0]["error"] = json!("PRIVATE_MARKER");
        variants.push(v);
        let mut v = j.clone();
        v["updatedAt"] = json!("2000-01-01T00:00:00Z");
        variants.push(v);
        let mut v = j.clone();
        v["entries"][0]["attempts"] = json!(0);
        variants.push(v);
        let mut v = j.clone();
        v["entries"][0]["id"] = json!("not-a-uuid");
        variants.push(v);
        for v in variants {
            c.execute(
                "UPDATE m_local_jobs SET data=?1 WHERE id=?2",
                params![v.to_string(), j["id"].as_str()],
            )
            .unwrap();
            assert!(get(&c, &b, &json!({"brandId":b,"jobId":j["id"]})).is_err());
            assert!(validate_database(&c).is_err());
            assert!(crate::backup::encrypt_snapshot(&c, "fictional-queue-password").is_err());
        }
        c.execute(
            "UPDATE m_local_jobs SET data=?1 WHERE id=?2",
            params![j.to_string(), j["id"].as_str()],
        )
        .unwrap();
        validate_database(&c).unwrap();
    }

    #[test]
    fn local_jobs_export_truth() {
        let (mut c, b, _) = fixture();
        let p = request(&c, &b, "独立指标导出", 2);
        let mut j = call(&mut c, &b, create, p).unwrap();
        j = mutation(&mut c, &b, resume, &j).unwrap();
        j = mutation(&mut c, &b, step, &j).unwrap();
        j = mutation(&mut c, &b, cancel, &j).unwrap();
        let p = json!({"brandId":b,"jobId":j["id"]});
        let files = prepare_export(&c, &b, &p).unwrap();
        assert_eq!(files, prepare_export(&c, &b, &p).unwrap());
        assert_eq!(files["files"].as_array().unwrap().len(), 3);
        assert_eq!(files["files"][0]["path"], "job.json");
        assert_eq!(
            serde_json::from_str::<Value>(files["files"][0]["content"].as_str().unwrap()).unwrap(),
            j
        );
        let s: Value =
            serde_json::from_str(files["files"][1]["content"].as_str().unwrap()).unwrap();
        assert_eq!(s["succeeded"], 1);
        assert_eq!(s["cancelled"], 1);
        assert_eq!(s["modelCalls"], 0);
        assert_eq!(s["modelFeeMinor"], 0);
        assert_eq!(s["localOnly"], true);
        assert_eq!(count(&c, "m_attribution_datasets"), 1);
        assert_eq!(
            list(&c, &b, &json!({"brandId":b})).unwrap()["jobs"][0]["revision"],
            j["revision"]
        );
    }
    #[test]
    fn local_jobs_stored_shape_state_limits() {
        let (mut c, b, _) = fixture();
        let p = request(&c, &b, "验证存档", 2);
        let created = call(&mut c, &b, create, p).unwrap();
        let original = reload(&c, &b, &created);
        let mut variants = vec![];
        for (key, value) in [
            ("attempts", json!(5)),
            ("attempts", json!(1.0)),
            ("datasetId", json!(id())),
            ("alreadyImported", json!(true)),
            ("error", json!(DOMAIN_ERRORS[0])),
            ("status", json!("running")),
        ] {
            let mut v = original.clone();
            v["entries"][0][key] = value;
            variants.push(v);
        }
        let mut v = original.clone();
        v["entries"][1]["id"] = v["entries"][0]["id"].clone();
        variants.push(v);
        let mut v = original.clone();
        v["status"] = json!("completed");
        variants.push(v);
        let mut v = original.clone();
        v["entries"][0]["status"] = json!("failed");
        v["entries"][0]["attempts"] = json!(1);
        v["entries"][0]["error"] = json!("PRIVATE_MARKER");
        variants.push(v);
        let mut v = original.clone();
        v["entries"][0]["products"][0]["id"] = json!(id());
        let input: Value =
            serde_json::from_str(v["entries"][0]["content"].as_str().unwrap()).unwrap();
        v["entries"][0]["expectedHash"] = json!(fingerprint(
            &json!({"input":input,"products":v["entries"][0]["products"]})
        ));
        v["contentHash"] = json!(job_hash(
            v["name"].as_str().unwrap(),
            v["entries"].as_array().unwrap()
        ));
        variants.push(v);
        for v in variants {
            c.execute(
                "UPDATE m_local_jobs SET data=?1,content_hash=?2 WHERE id=?3",
                params![
                    v.to_string(),
                    v["contentHash"].as_str(),
                    original["id"].as_str()
                ],
            )
            .unwrap();
            assert!(get(&c, &b, &json!({"brandId":b,"jobId":original["id"]})).is_err());
            assert!(validate_database(&c).is_err());
        }
        c.execute(
            "UPDATE m_local_jobs SET data=?1,content_hash=?2 WHERE id=?3",
            params![
                original.to_string(),
                original["contentHash"].as_str(),
                original["id"].as_str()
            ],
        )
        .unwrap();
        for n in 1..=50 {
            let mut v = original.clone();
            v["id"] = json!(id());
            v["name"] = json!(format!("篡改新增{n}"));
            v["contentHash"] = json!(job_hash(
                v["name"].as_str().unwrap(),
                v["entries"].as_array().unwrap()
            ));
            c.execute(
                "INSERT INTO m_local_jobs(id,brand_id,content_hash,data)VALUES(?1,?2,?3,?4)",
                params![
                    v["id"].as_str(),
                    b,
                    v["contentHash"].as_str(),
                    v.to_string()
                ],
            )
            .unwrap();
        }
        assert!(get(&c, &b, &json!({"brandId":b,"jobId":original["id"]})).is_err());
        assert!(validate_database(&c).is_err());
        assert!(list(&c, &b, &json!({"brandId":b})).is_err());
    }
    #[test]
    fn local_jobs_creation_reserves_all_outcome_bytes() {
        let mut c = crate::runtime::open_database(std::path::Path::new(":memory:")).unwrap();
        let b = handle(&mut c, "brand.create", json!({"name":"虚构大小边界演练"})).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let mut products = vec![];
        let mut rows = vec![];
        for n in 0..1000 {
            let sku = format!("B{n:04}");
            products.push(
                handle(
                    &mut c,
                    "product.save",
                    json!({"brandId":b,"sku":sku,"name":"x".repeat(200),"facts":[]}),
                )
                .unwrap(),
            );
            rows.push(json!({"id":format!("o{n:04}"),"sessionId":null,"sku":sku,"occurredAt":"2026-10-03T00:00:00Z","status":"paid","amountMinor":1}));
        }
        let mut input = exchange("虚构上限");
        input["orders"] = json!(rows);
        let content = attribution::normalize_exchange(&input.to_string())
            .unwrap()
            .to_string();
        let preview = attribution::preview(
            &c,
            &b,
            &json!({"brandId":b,"name":"边界数据","content":content}),
        )
        .unwrap();
        let entry = json!({"id":id(),"name":"边界数据","content":content,"expectedHash":preview["contentHash"],"products":preview["products"],"status":"pending","attempts":0,"datasetId":null,"alreadyImported":false,"error":null});
        let at = now();
        let mut projected = json!({"id":id(),"brandId":b,"name":"A","contentHash":"0".repeat(64),"createdAt":at,"updatedAt":at,"revision":1,"status":"paused","entries":vec![entry;3]});
        // Catalog names are frozen three times. Tune their valid UTF-8 sizes so
        // the initial job fits, while even one normal UUID result cannot fit.
        let gap = 2_099_990usize
            .checked_sub(projected.to_string().len())
            .unwrap();
        let mut extra = gap / 3;
        for (n, product) in products.iter_mut().enumerate() {
            let mut add = extra.min(400);
            if add == 399 {
                add = 398;
            }
            let bytes = 200 + add;
            let chinese = (bytes - 200).div_ceil(2);
            let ascii = bytes - 3 * chinese;
            let label = format!("{}{}", "茶".repeat(chinese), "x".repeat(ascii));
            assert!(label.chars().count() <= 200);
            assert_eq!(label.len(), bytes);
            product["name"] = json!(label);
            update(&c, "m_products", product).unwrap();
            for e in projected["entries"].as_array_mut().unwrap() {
                e["products"][n]["name"] = product["name"].clone();
            }
            extra -= add;
        }
        assert_eq!(extra, 0);
        projected["name"] = json!("A".repeat(1 + gap % 3));
        for e in projected["entries"].as_array_mut().unwrap() {
            e["expectedHash"] = json!(fingerprint(
                &json!({"input":input,"products":e["products"]})
            ));
        }
        let bytes = projected.to_string().len();
        assert_eq!(bytes, 2_099_990);
        assert!(bytes + 35 > 2_100_000);
        let build_request = |products: &[Value], label: &str| {
            let snapshots:Vec<_>=products.iter().map(|p|json!({"id":p["id"],"sku":p["sku"],"name":p["name"],"revision":p["revision"]})).collect();
            let expected = fingerprint(&json!({"input":input,"products":snapshots}));
            json!({"brandId":b,"name":label,"entries":vec![json!({"name":"边界数据","content":content,"expectedHash":expected});3]})
        };
        let request = build_request(&products, projected["name"].as_str().unwrap());
        assert!(request["entries"].to_string().len() < 1_500_000);
        assert!(content.len() <= 512 * 1024);
        let audits = count(&c, "m_audit");
        assert!(
            call(&mut c, &b, create, request).is_err(),
            "an accepted job must reserve room for bounded outcomes"
        );
        assert_eq!(count(&c, "m_local_jobs"), 0);
        assert_eq!(count(&c, "m_attribution_datasets"), 0);
        assert_eq!(count(&c, "m_audit"), audits);
        // A nearby job with sufficient room still records all success/replay
        // outcomes; conservative reservation must not disable valid large jobs.
        products[0]["name"] = json!("x".repeat(200));
        update(&c, "m_products", &products[0]).unwrap();
        let request = build_request(&products, "large accepted");
        let mut job = call(&mut c, &b, create, request.clone()).unwrap();
        job = mutation(&mut c, &b, resume, &job).unwrap();
        for _ in 0..3 {
            job = mutation(&mut c, &b, step, &job).unwrap();
        }
        assert_eq!(job["status"], "completed");
        assert_eq!(entry_count(&job, "succeeded"), 3);
        assert_eq!(count(&c, "m_attribution_datasets"), 1);
        assert!(job.to_string().len() <= 2_100_000);
        assert_eq!(
            call(&mut c, &b, create, request).unwrap()["alreadyExists"],
            true
        );
        // The same reserved capacity also accommodates the longest domain error
        // on every independent entry after current SKU ownership changes.
        let request = build_request(&products, "large failures");
        let mut failed = call(&mut c, &b, create, request).unwrap();
        products[0]["sku"] = json!("RENAMED");
        c.execute(
            "UPDATE m_products SET sku='RENAMED',data=?1 WHERE id=?2",
            params![products[0].to_string(), products[0]["id"].as_str()],
        )
        .unwrap();
        failed = mutation(&mut c, &b, resume, &failed).unwrap();
        for _ in 0..3 {
            failed = mutation(&mut c, &b, step, &failed).unwrap();
        }
        assert_eq!(failed["status"], "completed");
        assert_eq!(entry_count(&failed, "failed"), 3);
        assert!(failed.to_string().len() <= 2_100_000);
        for e in failed["entries"].as_array().unwrap() {
            assert_eq!(e["error"], "归因资料格式无效，请检查字段、时间、金额与引用");
        }
        validate_database(&c).unwrap();
    }
}
