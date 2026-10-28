//! Persistent offline rehearsal. This module has no network/provider transport path.
//! A transaction owns the task, mock CMS result and every budget debit together.
use super::*;
use std::collections::HashMap;

pub(super) const TABLE_SQL: &str = "CREATE TABLE IF NOT EXISTS m_p0_workflows(id TEXT PRIMARY KEY,brand_id TEXT NOT NULL,request_id TEXT NOT NULL,data TEXT NOT NULL,UNIQUE(brand_id,request_id),FOREIGN KEY(brand_id) REFERENCES m_brands(id));CREATE TABLE IF NOT EXISTS m_p0_policy(id INTEGER PRIMARY KEY CHECK(id=1),data TEXT NOT NULL);INSERT OR IGNORE INTO m_p0_policy(id,data)VALUES(1,'{\"revision\":1,\"dailyMaxUnits\":100,\"dailyMaxCalls\":100,\"halted\":false}');";
const KINDS: [&str; 7] = [
    "baseline", "generate", "audit", "review", "publish", "readback", "followup",
];
const MAX_BYTES: usize = 2 * 1024 * 1024;
const STATUSES: [&str; 10] = [
    "paused",
    "running",
    "awaiting_review",
    "awaiting_publish_approval",
    "awaiting_reconcile",
    "retry_wait",
    "budget_blocked",
    "completed",
    "failed",
    "cancelled",
];
fn shape(v: &Value, fields: &[&str]) -> Result<()> {
    let o = v.as_object().ok_or("P0 记录必须为对象")?;
    if o.len() != fields.len() || o.keys().any(|k| !fields.contains(&k.as_str())) {
        return Err("P0 记录字段缺失或包含未知字段".into());
    }
    Ok(())
}
fn uint(v: &Value, k: &str, min: u64, max: u64) -> Result<u64> {
    v[k].as_u64()
        .filter(|n| (min..=max).contains(n))
        .ok_or_else(|| format!("P0 字段 {k} 超出范围"))
}
fn hash(v: &Value, k: &str) -> Result<String> {
    let h = text(v, k, 64, true)?;
    if h.len() != 64
        || !h
            .bytes()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    {
        return Err("P0 摘要无效".into());
    }
    Ok(h)
}
fn error_field(v: &Value) -> Result<()> {
    if v.get("error") == Some(&Value::Null) {
        return Ok(());
    }
    text(v, "error", 2000, true).map(|_| ())
}
fn timestamp(v: &Value, k: &str) -> Result<String> {
    let value = date_time(v, k)?;
    if v[k] != value {
        return Err("P0 时间必须为标准 UTC 秒时间".into());
    }
    Ok(value)
}
fn effects() -> Value {
    json!({"networkRequests":0,"modelCalls":0,"publications":0})
}
fn target(v: &Value) -> Result<String> {
    let s = text(v, "targetUrl", 2000, true)?;
    let u = url::Url::parse(&s).map_err(|_| "模拟目标 URL 无效")?;
    if u.scheme() != "https"
        || !u
            .host_str()
            .is_some_and(|h| h.ends_with(".invalid") && h.len() > 8)
        || !u.username().is_empty()
        || u.password().is_some()
        || u.port().is_some()
        || u.fragment().is_some()
    {
        return Err("仅允许无凭据的 HTTPS .invalid 模拟目标".into());
    }
    Ok(s)
}
fn policy(c: &Connection) -> Result<Value> {
    let mut st = c
        .prepare("SELECT id,data FROM m_p0_policy")
        .map_err(db_error)?;
    let rows = st
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))
        .map_err(db_error)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(db_error)?;
    if rows.len() != 1 || rows[0].0 != 1 || rows[0].1.len() > 1024 {
        return Err("P0 工作区策略损坏".into());
    }
    let p = strict_json::parse(&rows[0].1)?;
    shape(
        &p,
        &["revision", "dailyMaxUnits", "dailyMaxCalls", "halted"],
    )?;
    uint(&p, "revision", 1, 9_007_199_254_740_991)?;
    uint(&p, "dailyMaxUnits", 1, 10000)?;
    uint(&p, "dailyMaxCalls", 1, 10000)?;
    boolean(&p, "halted")?;
    Ok(p)
}
fn all_tasks(c: &Connection, b: Option<&str>) -> Result<Vec<Value>> {
    let mut st = c.prepare("SELECT id,brand_id,request_id,data FROM m_p0_workflows WHERE (?1 IS NULL OR brand_id=?1) ORDER BY rowid").map_err(db_error)?;
    let rows = st
        .query_map([b], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })
        .map_err(db_error)?;
    let mut tasks = Vec::new();
    let mut counts = HashMap::new();
    for row in rows {
        let (tid, bid, rid, data) = row.map_err(db_error)?;
        if data.len() > MAX_BYTES {
            return Err("P0 任务超过2MiB".into());
        }
        let t = strict_json::parse(&data)?;
        if t["id"] != tid || t["brandId"] != bid || t["requestId"] != rid {
            return Err("P0 任务身份与索引不一致".into());
        }
        validate_record(c, &t)?;
        let n = counts.entry(bid).or_insert(0usize);
        *n += 1;
        if *n > 100 {
            return Err("每品牌最多保留100个 P0 任务".into());
        }
        tasks.push(t);
    }
    Ok(tasks)
}
fn usage(c: &Connection) -> Result<(String, u64, u64)> {
    let day = Utc::now().format("%Y-%m-%d").to_string();
    let (mut units, mut calls) = (0, 0);
    for t in all_tasks(c, None)? {
        for e in t["ledger"].as_array().unwrap() {
            if e["day"] == day {
                units += e["units"].as_u64().unwrap();
                calls += e["calls"].as_u64().unwrap();
            }
        }
    }
    Ok((day, units, calls))
}
fn policy_view(c: &Connection) -> Result<Value> {
    let mut p = policy(c)?;
    let (day, units, calls) = usage(c)?;
    p["day"] = json!(day);
    p["usedUnits"] = json!(units);
    p["usedCalls"] = json!(calls);
    Ok(p)
}
fn save_policy(c: &Connection, p: &mut Value) -> Result<()> {
    p["revision"] = json!(uint(p, "revision", 1, 9_007_199_254_740_990)? + 1);
    c.execute("UPDATE m_p0_policy SET data=?1 WHERE id=1", [p.to_string()])
        .map_err(db_error)?;
    Ok(())
}
fn halt_tasks(c: &Connection) -> Result<()> {
    for mut t in all_tasks(c, None)? {
        if ["running", "retry_wait"].contains(&t["status"].as_str().unwrap_or("")) {
            t["status"] = json!("paused");
            persist(c, &mut t)?;
        }
    }
    Ok(())
}
fn get(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    let tid = guid(p, "taskId")?;
    let data: String = c
        .query_row(
            "SELECT data FROM m_p0_workflows WHERE id=?1 AND brand_id=?2",
            params![tid, b],
            |r| r.get(0),
        )
        .optional()
        .map_err(db_error)?
        .ok_or("P0 任务不存在或不属于当前品牌")?;
    if data.len() > MAX_BYTES {
        return Err("P0 任务超过2MiB".into());
    }
    let t = strict_json::parse(&data)?;
    if t["id"] != tid || t["brandId"] != b {
        return Err("P0 任务身份不一致".into());
    }
    validate_record(c, &t)?;
    Ok(t)
}
fn persist(c: &Connection, t: &mut Value) -> Result<()> {
    t["revision"] = json!(uint(t, "revision", 1, 9_007_199_254_740_990)? + 1);
    t["updatedAt"] = json!(now());
    validate_record(c, t)?;
    update(c, "m_p0_workflows", t)
}
fn current_product(c: &Connection, t: &Value) -> Result<Value> {
    let p = load(
        c,
        "m_products",
        t["brandId"].as_str().ok_or("品牌损坏")?,
        t["productId"].as_str().ok_or("商品损坏")?,
    )?;
    if p["revision"] != t["productRevision"] {
        return Err("商品事实版本已变化，请新建演练任务".into());
    }
    confirmed_public(&p)?;
    Ok(p)
}
fn draft(c: &Connection, t: &Value, require_review: bool) -> Result<Value> {
    current_product(c, t)?;
    let b = t["brandId"].as_str().unwrap();
    let d = load(
        c,
        "m_drafts",
        b,
        t["draftId"].as_str().ok_or("尚未生成候选稿")?,
    )?;
    if d["productId"] != t["productId"] {
        return Err("候选稿不属于任务商品".into());
    }
    current_draft(c, b, &d, require_review)?;
    Ok(d)
}
fn content_snapshot(d: &Value) -> Value {
    json!({"draftId":d["id"],"title":d["title"],"body":d["body"],"sourceRevision":d["sourceRevision"]})
}
fn reviewed_draft(c: &Connection, t: &Value) -> Result<Value> {
    let d = draft(c, t, false)?;
    let r = &t["nodes"][3]["result"];
    if t["nodes"][3]["status"] != "completed"
        || r["draftId"] != d["id"]
        || r["draftRevision"] != d["revision"]
        || r["draftHash"] != fingerprint(&d)
        || r["reviewedAt"] != d["reviewedAt"]
    {
        return Err(
            "本任务审核节点完成后稿件已变化，请取消旧任务并新建演练，再重新审核和批准".into(),
        );
    }
    current_draft(c, t["brandId"].as_str().unwrap(), &d, true)?;
    Ok(d)
}
fn approval_value(t: &Value, at: &str, bind_preview: bool) -> Value {
    let r = &t["nodes"][3]["result"];
    let mut a = json!({"productId":t["productId"],"productRevision":t["productRevision"],"draftId":t["draftId"],"draftRevision":r["draftRevision"],"draftHash":r["draftHash"],"targetUrl":t["targetUrl"],"approvedAt":at});
    if bind_preview {
        a["previewHash"] = t["remote"]["preview"]["contentHash"].clone();
    }
    a["hash"] = json!(fingerprint(&a));
    a
}
fn current_approval(c: &Connection, t: &Value) -> Result<()> {
    reviewed_draft(c, t)?;
    let at = timestamp(&t["approval"], "approvedAt")?;
    if t["approval"] != approval_value(t, &at, t["approval"].get("previewHash").is_some()) {
        return Err("模拟发布批准已失效，请重新批准当前稿件与目标版本".into());
    }
    Ok(())
}
fn create(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    shape(
        p,
        &[
            "brandId",
            "requestId",
            "name",
            "productId",
            "mode",
            "question",
            "targetUrl",
            "scenario",
            "maxUnits",
            "maxCalls",
            "transport",
            "evidenceMode",
        ],
    )?;
    let rid = guid(p, "requestId")?;
    let pid = guid(p, "productId")?;
    let name = text(p, "name", 100, true)?;
    let mode = choice(p, "mode", &["natural_discovery", "informed_qa"])?;
    let question = text(p, "question", 500, true)?;
    let url = target(p)?;
    let scenario = choice(
        p,
        "scenario",
        &[
            "success",
            "rate_limit_once",
            "publish_timeout",
            "readback_mismatch",
            "robots_blocked",
        ],
    )?;
    choice(p, "transport", &["fixture"])?;
    choice(p, "evidenceMode", &["simulated"])?;
    let units = uint(p, "maxUnits", 1, 100)?;
    let calls = uint(p, "maxCalls", 1, 100)?;
    let product = load(c, "m_products", b, &pid)?;
    confirmed_public(&product)?;
    p0_contracts::monitor_request(&mode, &question, &brand(c, b)?, &product)?;
    for t in all_tasks(c, Some(b))? {
        if t["requestId"] != rid {
            continue;
        }
        for k in [
            "productId",
            "name",
            "mode",
            "question",
            "targetUrl",
            "scenario",
            "transport",
            "evidenceMode",
        ] {
            if t[k] != p[k] {
                return Err("请求标识已用于另一任务".into());
            }
        }
        if t["budget"]["maxUnits"] != units || t["budget"]["maxCalls"] != calls {
            return Err("请求标识已用于另一任务预算".into());
        }
        return Ok(t);
    }
    let count: i64 = c
        .query_row(
            "SELECT count(*) FROM m_p0_workflows WHERE brand_id=?1",
            [b],
            |r| r.get(0),
        )
        .map_err(db_error)?;
    if count >= 100 {
        return Err("每品牌最多保留100个 P0 任务".into());
    }
    let nodes: Vec<Value> = KINDS
        .iter()
        .map(|k| json!({"id":k,"kind":k,"status":"pending","result":null,"error":null}))
        .collect();
    let t = json!({"id":id(),"brandId":b,"productId":pid,"productRevision":product["revision"],"requestId":rid,"name":name,"revision":1,"mode":mode,"question":question,"targetUrl":url,"scenario":scenario,"transport":"fixture","evidenceMode":"simulated","status":"paused","createdAt":now(),"updatedAt":now(),"draftId":null,"approval":null,"error":null,"budget":{"maxUnits":units,"maxCalls":calls,"usedUnits":0,"usedCalls":0},"nodes":nodes,"ledger":[],"remote":{"preview":null,"published":null,"previous":null,"receipt":null,"rollback":null},"realEffects":effects()});
    validate_record(c, &t)?;
    c.execute(
        "INSERT INTO m_p0_workflows(id,brand_id,request_id,data)VALUES(?1,?2,?3,?4)",
        params![t["id"].as_str(), b, rid, t.to_string()],
    )
    .map_err(db_error)?;
    Ok(t)
}
fn operation_hash(t: &Value, node: &str) -> String {
    fingerprint(
        &json!({"taskId":t["id"],"productId":t["productId"],"productRevision":t["productRevision"],"mode":t["mode"],"question":t["question"],"targetUrl":t["targetUrl"],"scenario":t["scenario"],"nodeId":node,"approvalHash":if ["publish","readback","followup","rollback"].contains(&node){t["approval"]["hash"].clone()}else{Value::Null}}),
    )
}
fn attempts<'a>(t: &'a Value, node: &str) -> Vec<&'a Value> {
    t["ledger"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["nodeId"] == node)
        .collect()
}
fn ready_retry(t: &Value) -> Result<()> {
    if let Some(e) = t["ledger"].as_array().unwrap().last() {
        if e["status"] == "rate_limited" {
            let due = DateTime::parse_from_rfc3339(e["retryAt"].as_str().ok_or("缺少重试时间")?)
                .map_err(|_| "重试时间无效")?;
            if Utc::now() < due {
                return Err("429 模拟退避尚未到期，至少等待1秒".into());
            }
        }
    }
    Ok(())
}
fn can_charge(c: &Connection, t: &Value, units: u64) -> Result<bool> {
    let p = policy(c)?;
    if p["halted"] == true {
        return Ok(false);
    }
    let (_, used_units, used_calls) = usage(c)?;
    Ok(t["budget"]["usedUnits"].as_u64().unwrap() + units
        <= t["budget"]["maxUnits"].as_u64().unwrap()
        && t["budget"]["usedCalls"].as_u64().unwrap() + 1
            <= t["budget"]["maxCalls"].as_u64().unwrap()
        && used_units + units <= p["dailyMaxUnits"].as_u64().unwrap()
        && used_calls + 1 <= p["dailyMaxCalls"].as_u64().unwrap())
}
fn append_entry(
    t: &mut Value,
    node: &str,
    operation: &str,
    status: &str,
    result: &Value,
    at: &str,
) {
    let n = attempts(t, node).len() + 1;
    let limited = status == "rate_limited";
    let retry_at = if limited {
        json!(
            (DateTime::parse_from_rfc3339(at).unwrap() + chrono::Duration::seconds(1))
                .to_rfc3339_opts(SecondsFormat::Secs, true)
        )
    } else {
        Value::Null
    };
    let entry = json!({"operationId":operation,"nodeId":node,"attempt":n,"day":&at[..10],"startedAt":at,"finishedAt":at,"status":status,"units":if limited{0}else{1},"calls":1,"retryAt":retry_at,"requestHash":operation_hash(t,node),"resultHash":fingerprint(result)});
    t["ledger"].as_array_mut().unwrap().push(entry);
    t["budget"]["usedCalls"] = json!(t["budget"]["usedCalls"].as_u64().unwrap() + 1);
    if !limited {
        t["budget"]["usedUnits"] = json!(t["budget"]["usedUnits"].as_u64().unwrap() + 1);
    }
}
fn preview_for(t: &Value, d: &Value) -> Value {
    let mut f = p0_contracts::site_fixture(
        t["targetUrl"].as_str().unwrap(),
        d["title"].as_str().unwrap(),
        d["body"].as_str().unwrap(),
    );
    if t["scenario"] == "robots_blocked" {
        f["robotsTxt"] = json!("User-agent: *\nDisallow: /\n");
    }
    json!({"contentHash":fingerprint(&f),"fixture":f})
}
fn preview(t: &Value) -> Value {
    preview_for(t, &t["nodes"][1]["result"])
}
fn publication_result(t: &Value) -> Value {
    json!({"receipt":t["remote"]["receipt"]})
}
fn readback_result(t: &Value) -> Result<Value> {
    let mut f = t["remote"]["preview"]["fixture"].clone();
    if t["scenario"] == "readback_mismatch" {
        f["html"]=json!("<!doctype html><html><head><title>错误版本</title></head><body>另一个模拟页面</body></html>");
    }
    let audit = p0_contracts::audit_site_fixture(&f)?;
    Ok(
        json!({"fixture":f,"audit":audit,"matchesPublished":fingerprint(&f)==t["remote"]["receipt"]["contentHash"]}),
    )
}
fn perform(c: &Connection, t: &mut Value, i: usize, op: &str, at: &str) -> Result<Value> {
    let b = t["brandId"].as_str().unwrap().to_owned();
    match i {
        0 => {
            let req = p0_contracts::monitor_request(
                t["mode"].as_str().unwrap(),
                t["question"].as_str().unwrap(),
                &brand(c, &b)?,
                &current_product(c, t)?,
            )?;
            Ok(json!({"response":p0_contracts::monitor_fixture(&req)?,"request":req}))
        }
        1 => {
            let ctx = prepare(
                c,
                &b,
                &brand(c, &b)?,
                &json!({"productId":t["productId"],"kind":"description"}),
            )?;
            let d = insert_draft(c, &ctx, &local_body(&ctx)?, "local")?;
            c.execute(
                "DELETE FROM m_ai_preparations WHERE hash=?1",
                [fingerprint(&ctx)],
            )
            .map_err(db_error)?;
            t["draftId"] = d["id"].clone();
            Ok(content_snapshot(&d))
        }
        2 => {
            draft(c, t, false)?;
            let p = preview(t);
            let audit = p0_contracts::audit_site_fixture(&p["fixture"])?;
            t["remote"]["preview"] = p;
            Ok(audit)
        }
        4 => {
            let p = t["remote"]["preview"].clone();
            let receipt = json!({"operationId":op,"taskId":t["id"],"targetUrl":t["targetUrl"],"version":1,"contentHash":p["contentHash"],"publishedAt":at,"transport":"fixture","evidenceMode":"simulated"});
            t["remote"]["published"] =
                json!({"version":1,"contentHash":p["contentHash"],"fixture":p["fixture"]});
            t["remote"]["receipt"] = receipt;
            Ok(publication_result(t))
        }
        5 => readback_result(t),
        6 => {
            let req = t["nodes"][0]["result"]["request"].clone();
            Ok(
                json!({"request":req,"response":p0_contracts::monitor_fixture(&req)?,"comparisonLabel":"同一确定性模拟夹具复测，不代表真实可见性变化","resultChanged":false}),
            )
        }
        _ => Err("不支持的模拟节点".into()),
    }
}
fn fail(t: &mut Value, i: usize, error: &str) {
    t["status"] = json!("failed");
    t["error"] = json!(error);
    t["nodes"][i]["status"] = json!("failed");
    t["nodes"][i]["error"] = json!(error);
}
fn advance(c: &Connection, t: &mut Value) -> Result<bool> {
    if !["running", "awaiting_review", "retry_wait"].contains(&t["status"].as_str().unwrap_or("")) {
        return Ok(false);
    }
    if policy(c)?["halted"] == true {
        t["status"] = json!("paused");
        persist(c, t)?;
        return Ok(true);
    }
    if ready_retry(t).is_err() {
        return Ok(false);
    }
    let i = t["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .position(|n| n["status"] != "completed")
        .ok_or("任务节点状态损坏")?;
    let preflight = current_product(c, t).and_then(|_| {
        if i >= 4 {
            current_approval(c, t)
        } else {
            Ok(())
        }
    });
    if let Err(e) = preflight {
        fail(t, i, &e);
        persist(c, t)?;
        return Ok(true);
    }
    if i == 3 {
        let d = match draft(c, t, false) {
            Ok(d) => d,
            Err(e) => {
                fail(t, i, &e);
                persist(c, t)?;
                return Ok(true);
            }
        };
        if d["reviewedAt"].is_null() {
            if t["status"] == "awaiting_review" {
                return Ok(false);
            }
            t["status"] = json!("awaiting_review");
            t["nodes"][i]["status"] = json!("blocked");
            persist(c, t)?;
            return Ok(true);
        }
        if let Err(e) = current_draft(c, t["brandId"].as_str().unwrap(), &d, true) {
            fail(t, i, &e);
            persist(c, t)?;
            return Ok(true);
        }
        let mut result = json!({"draftId":d["id"],"draftRevision":d["revision"],"draftHash":fingerprint(&d),"reviewedAt":d["reviewedAt"]});
        let content = content_snapshot(&d);
        if content != t["nodes"][1]["result"] {
            let prior = attempts(t, "review_audit");
            if prior.len() >= 3 {
                fail(t, i, "审核稿模拟审计已达到3次尝试上限");
                persist(c, t)?;
                return Ok(true);
            }
            if !can_charge(c, t, 1)? {
                t["status"] = json!("budget_blocked");
                t["error"] = json!("审核稿模拟审计预算不足");
                t["nodes"][i]["status"] = json!("blocked");
                persist(c, t)?;
                return Ok(true);
            }
            let op = prior
                .first()
                .map(|e| e["operationId"].as_str().unwrap().to_owned())
                .unwrap_or_else(id);
            let at = now();
            let p = preview_for(t, &content);
            let audit = p0_contracts::audit_site_fixture(&p["fixture"])?;
            if audit["blocking"] != 0 {
                append_entry(t, "review_audit", &op, "failed", &Value::Null, &at);
                fail(t, i, "审核稿模拟页面审计不通过");
                persist(c, t)?;
                return Ok(true);
            }
            result["content"] = content;
            result["audit"] = audit;
            t["remote"]["preview"] = p;
            append_entry(t, "review_audit", &op, "success", &result, &at);
        }
        t["nodes"][i]["result"] = result;
        t["nodes"][i]["status"] = json!("completed");
        t["nodes"][i]["error"] = Value::Null;
        t["status"] = json!("awaiting_publish_approval");
        t["error"] = Value::Null;
        persist(c, t)?;
        return Ok(true);
    }
    let node = KINDS[i];
    let prior = attempts(t, node);
    if prior.len() >= 3 {
        fail(t, i, "当前模拟操作已达到3次尝试上限");
        persist(c, t)?;
        return Ok(true);
    }
    let op = prior
        .first()
        .map(|e| e["operationId"].as_str().unwrap().to_owned())
        .unwrap_or_else(id);
    let limited = t["scenario"] == "rate_limit_once" && i == 0 && prior.is_empty();
    if !can_charge(c, t, if limited { 0 } else { 1 })? {
        t["status"] = json!("budget_blocked");
        t["error"] = json!("模拟单位或调用次数预算不足，请检查任务与工作区日限额");
        t["nodes"][i]["status"] = json!("blocked");
        persist(c, t)?;
        return Ok(true);
    }
    let at = now();
    if limited {
        append_entry(t, node, &op, "rate_limited", &Value::Null, &at);
        t["status"] = json!("retry_wait");
        t["nodes"][i]["status"] = json!("blocked");
        t["error"] = json!("模拟429：至少1秒后可重试");
        persist(c, t)?;
        return Ok(true);
    }
    // The caller's outer transaction includes the final ledger write. A failed
    // generation rolls back its native draft/preparation before recording failure.
    c.execute_batch("SAVEPOINT p0_node").map_err(db_error)?;
    let before = t.clone();
    let result = match perform(c, t, i, &op, &at) {
        Ok(v) => {
            c.execute_batch("RELEASE p0_node").map_err(db_error)?;
            v
        }
        Err(e) => {
            c.execute_batch("ROLLBACK TO p0_node; RELEASE p0_node")
                .map_err(db_error)?;
            *t = before;
            fail(t, i, &e);
            append_entry(t, node, &op, "failed", &Value::Null, &at);
            persist(c, t)?;
            return Ok(true);
        }
    };
    let blocked = (i == 2 && result["blocking"].as_u64().unwrap_or(1) > 0)
        || (i == 5
            && (result["audit"]["blocking"].as_u64().unwrap_or(1) > 0
                || result["matchesPublished"] != true));
    let unknown = i == 4 && t["scenario"] == "publish_timeout";
    append_entry(
        t,
        node,
        &op,
        if unknown {
            "unknown"
        } else if blocked {
            "failed"
        } else {
            "success"
        },
        &result,
        &at,
    );
    t["error"] = Value::Null;
    t["nodes"][i]["error"] = Value::Null;
    if unknown {
        t["nodes"][i]["status"] = json!("blocked");
        t["status"] = json!("awaiting_reconcile");
        t["error"] = json!("模拟发布响应超时；须使用原操作 ID 对账，不能重新发布");
    } else {
        t["nodes"][i]["result"] = result;
        t["nodes"][i]["status"] = json!("completed");
        t["status"] = json!(if i == 6 { "completed" } else { "running" });
        if blocked {
            fail(t, i, "模拟页面审计或回读不通过");
        }
    }
    persist(c, t)?;
    Ok(true)
}
fn export_workflow(c: &Connection, b: &str, t: &Value) -> Result<Value> {
    if t["status"] != "completed"
        && !(t["status"] == "cancelled" && !t["remote"]["rollback"].is_null())
    {
        return Err("请先完成演练及原生稿件审核后再导出证据".into());
    }
    let d = reviewed_draft(c, t)?;
    current_approval(c, t)?;
    let mut files = vec![
        json!({"path":"p0-workflow/task.json","content":serde_json::to_string_pretty(t).map_err(|_|"报告生成失败")?}),
        json!({"path":"p0-workflow/capabilities.json","content":serde_json::to_string_pretty(&p0_contracts::capabilities()).map_err(|_|"能力说明生成失败")?}),
        json!({"path":"p0-workflow/report.md","content":format!("# P0 本地模拟联调演练\n\n{}\n\n仅限 fixture / simulated；不代表真实模型效果、消费者平台结果或实际发布。\n真实网络请求：0；模型调用：0；发布：0。\n\n任务状态：{}；模拟单位：{}；模拟调用次数：{}。\n\n原生稿件审核与模拟发布批准分别保留；全部操作 ID 和预算记账见 task.json。\n",t["name"].as_str().unwrap(),t["status"].as_str().unwrap(),t["budget"]["usedUnits"],t["budget"]["usedCalls"])}),
        json!({"path":"p0-workflow/ledger.json","content":serde_json::to_string_pretty(&t["ledger"]).map_err(|_|"台账生成失败")?}),
        json!({"path":"p0-workflow/mock-cms.json","content":serde_json::to_string_pretty(&t["remote"]).map_err(|_|"模拟回执生成失败")?}),
    ];
    for format in ["markdown", "html"] {
        let e = super::export(
            c,
            b,
            &json!({"id":d["id"],"expectedRevision":d["revision"],"format":format}),
        )?;
        files.push(json!({"path":format!("p0-workflow/{}",e["fileName"].as_str().ok_or("导出名称无效")?),"content":e["content"]}));
    }
    Ok(
        json!({"taskId":t["id"],"revision":t["revision"],"transport":"fixture","evidenceMode":"simulated","realEffects":effects(),"files":files}),
    )
}
pub(super) fn dispatch(c: &Connection, b: &str, a: &str, p: &Value) -> Result<Value> {
    if guid(p, "brandId")? != b {
        return Err("品牌参数不一致".into());
    }
    brand(c, b)?;
    match a {
        "p0Workflow.create" => return create(c, b, p),
        "p0Workflow.list" => {
            shape(p, &["brandId"])?;
            return Ok(json!({"tasks":all_tasks(c,Some(b))?}));
        }
        "p0Workflow.previewMonitor" => {
            shape(p, &["brandId", "productId", "mode", "question"])?;
            let prod = load(c, "m_products", b, &guid(p, "productId")?)?;
            return p0_contracts::monitor_request(
                &text(p, "mode", 50, true)?,
                &text(p, "question", 500, true)?,
                &brand(c, b)?,
                &prod,
            );
        }
        "p0Workflow.auditFixture" => {
            shape(p, &["brandId", "fixture"])?;
            return p0_contracts::audit_site_fixture(&p["fixture"]);
        }
        "p0Workflow.policyGet" => {
            shape(p, &["brandId"])?;
            return policy_view(c);
        }
        "p0Workflow.policyUpdate" => {
            shape(
                p,
                &[
                    "brandId",
                    "expectedRevision",
                    "dailyMaxUnits",
                    "dailyMaxCalls",
                    "halted",
                ],
            )?;
            let mut pol = policy(c)?;
            revision(&pol, p)?;
            let units = uint(p, "dailyMaxUnits", 1, 10000)?;
            let calls = uint(p, "dailyMaxCalls", 1, 10000)?;
            let halted = boolean(p, "halted")?;
            pol["dailyMaxUnits"] = json!(units);
            pol["dailyMaxCalls"] = json!(calls);
            pol["halted"] = json!(halted);
            save_policy(c, &mut pol)?;
            if halted {
                halt_tasks(c)?;
            }
            return policy_view(c);
        }
        "p0Workflow.haltAll" => {
            shape(p, &["brandId", "expectedRevision"])?;
            let mut pol = policy(c)?;
            revision(&pol, p)?;
            pol["halted"] = json!(true);
            save_policy(c, &mut pol)?;
            halt_tasks(c)?;
            return policy_view(c);
        }
        _ => {}
    }
    if a == "p0Workflow.get" {
        shape(p, &["brandId", "taskId"])?;
        return get(c, b, p);
    }
    let consent = [
        "p0Workflow.approvePublish",
        "p0Workflow.reconcile",
        "p0Workflow.rollback",
    ]
    .contains(&a);
    if consent {
        shape(p, &["brandId", "taskId", "expectedRevision", "confirm"])?;
        if p["confirm"] != true {
            return Err("请明确确认本次模拟操作".into());
        }
    } else {
        shape(p, &["brandId", "taskId", "expectedRevision"])?;
    }
    let mut t = get(c, b, p)?;
    revision(&t, p)?;
    match a {
        "p0Workflow.export" => return export_workflow(c, b, &t),
        "p0Workflow.step" => {
            // Explicit one-step rehearsal is allowed while paused, without
            // enrolling the task in the background worker afterward.
            let one_step = t["status"] == "paused";
            if one_step {
                if policy(c)?["halted"] == true {
                    return Err("工作区已全局停止".into());
                }
                ready_retry(&t)?;
                if t["nodes"][3]["status"] == "completed" && t["approval"].is_null() {
                    reviewed_draft(c, &t)?;
                    t["status"] = json!("awaiting_publish_approval");
                    persist(c, &mut t)?;
                    return Ok(t);
                }
                t["status"] = json!("running");
            }
            advance(c, &mut t)?;
            if one_step && t["status"] == "running" {
                t["status"] = json!("paused");
                persist(c, &mut t)?;
            }
            return Ok(t);
        }
        "p0Workflow.resume" => {
            if !["paused", "awaiting_review", "budget_blocked"]
                .contains(&t["status"].as_str().unwrap())
            {
                return Err("当前任务状态不能继续".into());
            }
            if policy(c)?["halted"] == true {
                return Err("工作区已全局停止，请先解除停止后逐项继续".into());
            }
            current_product(c, &t)?;
            ready_retry(&t)?;
            t["status"] = json!(
                if t["nodes"][3]["status"] == "completed" && t["approval"].is_null() {
                    "awaiting_publish_approval"
                } else {
                    "running"
                }
            );
            t["error"] = Value::Null;
        }
        "p0Workflow.pause" => {
            if t["status"] == "paused" {
                return Ok(t);
            }
            if ![
                "running",
                "awaiting_review",
                "awaiting_publish_approval",
                "retry_wait",
                "budget_blocked",
            ]
            .contains(&t["status"].as_str().unwrap())
            {
                return Err("当前任务状态不能暂停".into());
            }
            t["status"] = json!("paused");
        }
        "p0Workflow.cancel" => {
            if t["status"] == "cancelled" {
                return Ok(t);
            }
            if ["completed", "awaiting_reconcile"].contains(&t["status"].as_str().unwrap()) {
                return Err("已完成任务不可取消；未知结果须先对账".into());
            }
            t["status"] = json!("cancelled");
        }
        "p0Workflow.retry" => {
            if !["failed", "retry_wait", "budget_blocked"].contains(&t["status"].as_str().unwrap())
            {
                return Err("仅失败、预算阻塞或退避任务可重试；未知发布必须对账".into());
            }
            if policy(c)?["halted"] == true {
                return Err("工作区已全局停止".into());
            }
            current_product(c, &t)?;
            ready_retry(&t)?;
            let i = t["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .position(|n| n["status"] != "completed")
                .ok_or("没有待重试节点")?;
            if attempts(&t, KINDS[i]).len() >= 3 {
                return Err("当前模拟操作已达到3次尝试上限".into());
            }
            t["nodes"][i]["status"] = json!("pending");
            t["nodes"][i]["result"] = Value::Null;
            t["nodes"][i]["error"] = Value::Null;
            t["status"] = json!("running");
            t["error"] = Value::Null;
        }
        "p0Workflow.approvePublish" => {
            if t["status"] != "awaiting_publish_approval" {
                return Err("须先完成原生稿件审核节点，才能单独批准模拟发布".into());
            }
            if policy(c)?["halted"] == true {
                return Err("工作区已全局停止".into());
            }
            reviewed_draft(c, &t)?;
            t["approval"] = approval_value(&t, &now(), true);
            t["status"] = json!("running");
            t["error"] = Value::Null;
        }
        "p0Workflow.reconcile" => {
            if t["status"] != "awaiting_reconcile" {
                return Err("当前任务没有待对账的未知发布".into());
            }
            // Reconciliation checks the already-persisted operation and remote
            // receipt. Later product/draft changes must not strand an unknown
            // outcome; subsequent execution still rechecks current approval.
            let i = t["ledger"]
                .as_array()
                .unwrap()
                .iter()
                .position(|e| e["nodeId"] == "publish" && e["status"] == "unknown")
                .ok_or("原发布操作不存在")?;
            if t["ledger"][i]["operationId"] != t["remote"]["receipt"]["operationId"] {
                return Err("模拟远端回执与原操作 ID 不一致".into());
            }
            let result = publication_result(&t);
            if t["ledger"][i]["resultHash"] != fingerprint(&result) {
                return Err("模拟远端结果与原操作不一致".into());
            }
            t["ledger"][i]["status"] = json!("reconciled");
            t["ledger"][i]["finishedAt"] = json!(now());
            t["nodes"][4]["status"] = json!("completed");
            t["nodes"][4]["result"] = result;
            t["nodes"][4]["error"] = Value::Null;
            t["error"] = Value::Null;
            t["status"] = json!(if policy(c)?["halted"] == true {
                "paused"
            } else {
                "running"
            });
        }
        "p0Workflow.rollback" => {
            if !t["remote"]["rollback"].is_null() {
                return Ok(t);
            }
            if t["remote"]["published"].is_null() || t["status"] == "awaiting_reconcile" {
                return Err("须先确认已有模拟发布回执，未知结果须先对账".into());
            }
            if !can_charge(c, &t, 1)? {
                return Err("工作区已停止或模拟回滚预算不足".into());
            }
            let op = id();
            let at = now();
            let result = json!({"operationId":op,"fromVersion":1,"toVersion":0,"rolledBackAt":at,"receiptHash":fingerprint(&t["remote"]["receipt"]),"transport":"fixture","evidenceMode":"simulated"});
            append_entry(&mut t, "rollback", &op, "success", &result, &at);
            t["remote"]["rollback"] = result;
            t["remote"]["published"] = Value::Null;
            t["status"] = json!("cancelled");
            t["error"] = Value::Null;
        }
        _ => return Err("不支持的 P0 工作流操作".into()),
    }
    persist(c, &mut t)?;
    Ok(t)
}
fn validate_record(c: &Connection, t: &Value) -> Result<()> {
    if t.to_string().len() > MAX_BYTES {
        return Err("P0 任务超过2MiB".into());
    }
    shape(
        t,
        &[
            "id",
            "brandId",
            "productId",
            "productRevision",
            "requestId",
            "name",
            "revision",
            "mode",
            "question",
            "targetUrl",
            "scenario",
            "transport",
            "evidenceMode",
            "status",
            "createdAt",
            "updatedAt",
            "draftId",
            "approval",
            "error",
            "budget",
            "nodes",
            "ledger",
            "remote",
            "realEffects",
        ],
    )?;
    for k in ["id", "brandId", "productId", "requestId"] {
        guid(t, k)?;
    }
    uint(t, "revision", 1, 9_007_199_254_740_991)?;
    let source_revision = uint(t, "productRevision", 1, 9_007_199_254_740_991)?;
    text(t, "name", 100, true)?;
    text(t, "question", 500, true)?;
    target(t)?;
    choice(t, "mode", &["natural_discovery", "informed_qa"])?;
    choice(
        t,
        "scenario",
        &[
            "success",
            "rate_limit_once",
            "publish_timeout",
            "readback_mismatch",
            "robots_blocked",
        ],
    )?;
    choice(t, "transport", &["fixture"])?;
    choice(t, "evidenceMode", &["simulated"])?;
    let status = choice(t, "status", &STATUSES)?;
    error_field(t)?;
    let created = timestamp(t, "createdAt")?;
    let updated = timestamp(t, "updatedAt")?;
    if created > updated || updated > now() {
        return Err("P0 任务时间顺序无效".into());
    }
    if t["realEffects"] != effects() {
        return Err("P0 演练不能包含真实副作用".into());
    }
    let b = t["brandId"].as_str().unwrap();
    brand(c, b)?;
    let product = load(c, "m_products", b, t["productId"].as_str().unwrap())?;
    if uint(&product, "revision", 1, 9_007_199_254_740_991)? < source_revision {
        return Err("P0 商品版本超出当前商品".into());
    }
    shape(
        &t["budget"],
        &["maxUnits", "maxCalls", "usedUnits", "usedCalls"],
    )?;
    let max_units = uint(&t["budget"], "maxUnits", 1, 100)?;
    let max_calls = uint(&t["budget"], "maxCalls", 1, 100)?;
    let used_units = uint(&t["budget"], "usedUnits", 0, max_units)?;
    let used_calls = uint(&t["budget"], "usedCalls", 0, max_calls)?;
    let nodes = t["nodes"]
        .as_array()
        .filter(|a| a.len() == 7)
        .ok_or("P0 固定节点图损坏")?;
    let mut first = None;
    let mut failed = 0;
    for (i, n) in nodes.iter().enumerate() {
        shape(n, &["id", "kind", "status", "result", "error"])?;
        if n["id"] != KINDS[i] || n["kind"] != KINDS[i] {
            return Err("P0 固定节点身份损坏".into());
        }
        let ns = choice(n, "status", &["pending", "completed", "blocked", "failed"])?;
        error_field(n)?;
        if ns == "completed" {
            if first.is_some() || n["result"].is_null() || !n["error"].is_null() {
                return Err("P0 节点绕过依赖或完成结果缺失".into());
            }
        } else {
            if first.is_none() {
                first = Some(i);
            } else if ns != "pending" {
                return Err("P0 后续节点不能越过前置节点".into());
            }
            if ns == "failed" {
                failed += 1;
                if n["error"].is_null() {
                    return Err("失败节点缺少错误".into());
                }
            } else if !n["error"].is_null() || !n["result"].is_null() {
                return Err("未执行节点不得含结果或错误".into());
            }
        }
    }
    if status == "completed" && first.is_some() {
        return Err("P0 未完成所有节点不能标记完成".into());
    }
    if first.is_none() && !["completed", "cancelled"].contains(&status.as_str()) {
        return Err("P0 已完成节点与任务状态不符".into());
    }
    if (status == "failed") != (failed == 1) && status != "cancelled" {
        return Err("P0 失败状态与节点不一致".into());
    }
    if status == "awaiting_review" && (first != Some(3) || nodes[3]["status"] != "blocked") {
        return Err("P0 审核阻塞状态无效".into());
    }
    if status == "awaiting_publish_approval" && (first != Some(4) || !t["approval"].is_null()) {
        return Err("P0 发布批准状态无效".into());
    }
    let d = if t["draftId"].is_null() {
        None
    } else {
        guid(t, "draftId")?;
        let d = load(c, "m_drafts", b, t["draftId"].as_str().unwrap())?;
        if d["productId"] != t["productId"] || d["sourceRevision"] != t["productRevision"] {
            return Err("P0 稿件商品或版本关联无效".into());
        }
        Some(d)
    };
    if (nodes[1]["status"] == "completed") != d.is_some() {
        return Err("P0 稿件引用与生成节点不一致".into());
    }
    if !nodes[1]["result"].is_null() {
        let r = &nodes[1]["result"];
        shape(r, &["draftId", "title", "body", "sourceRevision"])?;
        if r["draftId"] != t["draftId"] || r["sourceRevision"] != t["productRevision"] {
            return Err("P0 候选稿快照关联无效".into());
        }
        text(r, "title", 300, true)?;
        raw(r, "body", 30000, true)?;
        // The generation snapshot represents the native initial revision.
        // Native edits advance revision; an unchanged initial draft therefore
        // supplies an independent check even if a task ledger was rehashed.
        if let Some(draft) = d.as_ref() {
            if draft["revision"] == 1 && content_snapshot(draft) != *r {
                return Err("P0 生成快照与原生初始稿件不一致".into());
            }
        }
    }
    for i in [0, 6] {
        if !nodes[i]["result"].is_null() {
            let r = &nodes[i]["result"];
            if i == 0 {
                shape(r, &["request", "response"])?;
            } else {
                shape(
                    r,
                    &["request", "response", "comparisonLabel", "resultChanged"],
                )?;
                if r["request"] != nodes[0]["result"]["request"]
                    || r["comparisonLabel"] != "同一确定性模拟夹具复测，不代表真实可见性变化"
                    || r["resultChanged"] != false
                {
                    return Err("P0 复测必须使用原模拟夹具".into());
                }
            }
            if r["request"]["mode"] != t["mode"]
                || r["request"]["question"] != t["question"]
                || r["response"] != p0_contracts::monitor_fixture(&r["request"])?
            {
                return Err("P0 监测模拟结果与请求不一致".into());
            }
        }
    }
    if !nodes[3]["result"].is_null() {
        let r = &nodes[3]["result"];
        let edited = r.get("content").is_some();
        if edited {
            shape(
                r,
                &[
                    "draftId",
                    "draftRevision",
                    "draftHash",
                    "reviewedAt",
                    "content",
                    "audit",
                ],
            )?;
            let content = &r["content"];
            shape(content, &["draftId", "title", "body", "sourceRevision"])?;
            text(content, "title", 300, true)?;
            raw(content, "body", 30000, true)?;
            if content["draftId"] != t["draftId"]
                || content["sourceRevision"] != t["productRevision"]
                || *content == nodes[1]["result"]
            {
                return Err("P0 审核稿快照关联无效".into());
            }
            let p = preview_for(t, content);
            if t["remote"]["preview"] != p
                || r["audit"] != p0_contracts::audit_site_fixture(&p["fixture"])?
                || r["audit"]["blocking"] != 0
            {
                return Err("P0 审核稿预览或审计无效".into());
            }
        } else {
            shape(r, &["draftId", "draftRevision", "draftHash", "reviewedAt"])?;
        }
        hash(r, "draftHash")?;
        timestamp(r, "reviewedAt")?;
        let rev = uint(r, "draftRevision", 1, 9_007_199_254_740_991)?;
        let d = d.as_ref().ok_or("审核节点缺少稿件")?;
        if r["draftId"] != t["draftId"] || rev > d["revision"].as_u64().ok_or("稿件版本无效")?
        {
            return Err("P0 审核稿件引用无效".into());
        }
        if r["draftRevision"] == d["revision"]
            && (r["draftHash"] != fingerprint(d)
                || r["reviewedAt"] != d["reviewedAt"]
                || d["reviewedAt"].is_null()
                || (edited && r["content"] != content_snapshot(d))
                || (!edited && nodes[1]["result"] != content_snapshot(d)))
        {
            return Err("P0 审核快照与原生稿件不一致".into());
        }
    }
    if !t["approval"].is_null() {
        if nodes[3]["status"] != "completed" {
            return Err("P0 发布批准绕过原生审核节点".into());
        }
        let at = timestamp(&t["approval"], "approvedAt")?;
        if at < created
            || at > updated
            || (nodes[3]["result"].get("content").is_some()
                && t["approval"].get("previewHash").is_none())
            || t["approval"] != approval_value(t, &at, t["approval"].get("previewHash").is_some())
        {
            return Err("P0 发布批准摘要或版本无效".into());
        }
    }
    shape(
        &t["remote"],
        &["preview", "published", "previous", "receipt", "rollback"],
    )?;
    if !t["remote"]["previous"].is_null() {
        return Err("P0 初次模拟发布不得伪造历史版本".into());
    }
    if !t["remote"]["preview"].is_null() {
        let expected = if nodes[3]["result"].get("content").is_some() {
            preview_for(t, &nodes[3]["result"]["content"])
        } else {
            preview(t)
        };
        if nodes[1]["status"] != "completed" || t["remote"]["preview"] != expected {
            return Err("P0 模拟预览与生成稿件不一致".into());
        }
    }
    if !nodes[2]["result"].is_null() {
        if t["remote"]["preview"].is_null()
            || nodes[2]["result"] != p0_contracts::audit_site_fixture(&preview(t)["fixture"])?
        {
            return Err("P0 审计结果与模拟预览不一致".into());
        }
        if nodes[2]["status"] == "completed" && nodes[2]["result"]["blocking"] != 0 {
            return Err("P0 阻断审计不能标记完成".into());
        }
    }
    let receipt = &t["remote"]["receipt"];
    if !receipt.is_null() {
        shape(
            receipt,
            &[
                "operationId",
                "taskId",
                "targetUrl",
                "version",
                "contentHash",
                "publishedAt",
                "transport",
                "evidenceMode",
            ],
        )?;
        guid(receipt, "operationId")?;
        timestamp(receipt, "publishedAt")?;
        if t["approval"].is_null()
            || nodes[2]["status"] != "completed"
            || receipt["taskId"] != t["id"]
            || receipt["targetUrl"] != t["targetUrl"]
            || receipt["version"] != 1
            || receipt["contentHash"] != t["remote"]["preview"]["contentHash"]
            || receipt["transport"] != "fixture"
            || receipt["evidenceMode"] != "simulated"
        {
            return Err("P0 模拟发布回执关联无效".into());
        }
        if t["remote"]["rollback"].is_null() {
            let p = &t["remote"]["preview"];
            if t["remote"]["published"]
                != json!({"version":1,"contentHash":p["contentHash"],"fixture":p["fixture"]})
            {
                return Err("P0 已发布模拟版本与预览不一致".into());
            }
        } else {
            let r = &t["remote"]["rollback"];
            shape(
                r,
                &[
                    "operationId",
                    "fromVersion",
                    "toVersion",
                    "rolledBackAt",
                    "receiptHash",
                    "transport",
                    "evidenceMode",
                ],
            )?;
            guid(r, "operationId")?;
            timestamp(r, "rolledBackAt")?;
            if !t["remote"]["published"].is_null()
                || r["fromVersion"] != 1
                || r["toVersion"] != 0
                || r["receiptHash"] != fingerprint(receipt)
                || r["transport"] != "fixture"
                || r["evidenceMode"] != "simulated"
                || status != "cancelled"
            {
                return Err("P0 回滚回执不一致".into());
            }
        }
    } else if !t["remote"]["published"].is_null() || !t["remote"]["rollback"].is_null() {
        return Err("P0 模拟远端状态缺少发布回执".into());
    }
    if !nodes[4]["result"].is_null()
        && (receipt.is_null() || nodes[4]["result"] != publication_result(t))
    {
        return Err("P0 发布节点与回执不一致".into());
    }
    if !nodes[5]["result"].is_null() {
        let r = readback_result(t)?;
        if receipt.is_null() || nodes[5]["result"] != r {
            return Err("P0 回读结果与模拟远端不一致".into());
        }
        if nodes[5]["status"] == "completed"
            && (r["audit"]["blocking"] != 0 || r["matchesPublished"] != true)
        {
            return Err("P0 回读失败不能标记完成".into());
        }
    }
    validate_ledger(t, used_units, used_calls, &created, &updated)?;
    Ok(())
}
fn ledger_result(t: &Value, node: &str) -> Result<Value> {
    Ok(match node {
        "baseline" => t["nodes"][0]["result"].clone(),
        "generate" => t["nodes"][1]["result"].clone(),
        "audit" => {
            if t["remote"]["preview"].is_null() {
                Value::Null
            } else {
                p0_contracts::audit_site_fixture(&preview(t)["fixture"])?
            }
        }
        "review_audit" => t["nodes"][3]["result"].clone(),
        "publish" => {
            if t["remote"]["receipt"].is_null() {
                Value::Null
            } else {
                publication_result(t)
            }
        }
        "readback" => {
            if t["remote"]["receipt"].is_null() {
                Value::Null
            } else {
                readback_result(t)?
            }
        }
        "followup" => t["nodes"][6]["result"].clone(),
        "rollback" => t["remote"]["rollback"].clone(),
        _ => return Err("台账节点无效".into()),
    })
}
fn validate_ledger(t: &Value, units: u64, calls: u64, created: &str, updated: &str) -> Result<()> {
    let ledger = t["ledger"]
        .as_array()
        .filter(|a| a.len() <= 22)
        .ok_or("P0 台账数量无效")?;
    let (mut total_units, mut total_calls, mut previous_index, mut unknown) = (0, 0, 0, 0);
    let mut per_node: HashMap<&str, Vec<&Value>> = HashMap::new();
    let mut operation_nodes = HashMap::new();
    let mut previous_at = created.to_owned();
    for e in ledger {
        shape(
            e,
            &[
                "operationId",
                "nodeId",
                "attempt",
                "day",
                "startedAt",
                "finishedAt",
                "status",
                "units",
                "calls",
                "retryAt",
                "requestHash",
                "resultHash",
            ],
        )?;
        let op = guid(e, "operationId")?;
        let node = e["nodeId"].as_str().ok_or("台账节点无效")?;
        let index = if node == "rollback" {
            7
        } else if node == "review_audit" {
            3
        } else {
            KINDS
                .iter()
                .position(|k| *k == node)
                .filter(|i| *i != 3)
                .ok_or("台账节点无效")?
        };
        if index < previous_index {
            return Err("P0 台账节点顺序无效".into());
        }
        previous_index = index;
        if let Some(prior) = operation_nodes.insert(op.clone(), node) {
            if prior != node {
                return Err("操作 ID 被多个节点复用".into());
            }
        }
        let es = per_node.entry(node).or_default();
        let attempt = uint(e, "attempt", 1, 3)?;
        if attempt != es.len() as u64 + 1 || (node == "rollback" && attempt != 1) {
            return Err("P0 尝试次数不连续或超限".into());
        }
        if let Some(prev) = es.last() {
            if prev["operationId"] != op
                || !["failed", "rate_limited"].contains(&prev["status"].as_str().unwrap_or(""))
            {
                return Err("成功或未知操作不得再次提交".into());
            }
            if prev["status"] == "rate_limited"
                && e["startedAt"].as_str() < prev["retryAt"].as_str()
            {
                return Err("P0 429 重试绕过退避时间".into());
            }
        }
        let start = timestamp(e, "startedAt")?;
        let finish = timestamp(e, "finishedAt")?;
        if start < previous_at
            || start.as_str() < created
            || start > finish
            || finish.as_str() > updated
            || e["day"] != start[..10]
        {
            return Err("P0 台账日期或时间顺序不一致".into());
        }
        previous_at = start.clone();
        let state = choice(
            e,
            "status",
            &["success", "failed", "rate_limited", "unknown", "reconciled"],
        )?;
        hash(e, "requestHash")?;
        hash(e, "resultHash")?;
        if e["requestHash"] != operation_hash(t, node) || e["calls"] != 1 {
            return Err("P0 台账请求摘要或调用扣量无效".into());
        }
        let expected = ledger_result(t, node)?;
        if state == "rate_limited" {
            let due = (DateTime::parse_from_rfc3339(&start).map_err(|_| "时间损坏")?
                + chrono::Duration::seconds(1))
            .to_rfc3339_opts(SecondsFormat::Secs, true);
            if node != "baseline"
                || attempt != 1
                || t["scenario"] != "rate_limit_once"
                || e["units"] != 0
                || e["retryAt"] != due
                || e["resultHash"] != fingerprint(&Value::Null)
            {
                return Err("P0 429 台账无效".into());
            }
        } else {
            if e["units"] != 1 || !e["retryAt"].is_null() {
                return Err("P0 模拟操作必须扣1单位且无退避标记".into());
            }
            if state == "failed" {
                if e["resultHash"] != fingerprint(&Value::Null)
                    && e["resultHash"] != fingerprint(&expected)
                {
                    return Err("P0 失败台账结果摘要无效".into());
                }
            } else if expected.is_null() || e["resultHash"] != fingerprint(&expected) {
                return Err("P0 成功台账结果缺失或摘要无效".into());
            }
        }
        if ["unknown", "reconciled"].contains(&state.as_str()) {
            if node != "publish" || t["scenario"] != "publish_timeout" || attempt != 1 {
                return Err("P0 未知结果仅允许原始超时发布操作".into());
            }
            if state == "unknown" {
                unknown += 1;
            }
        }
        if node == "publish" && state != "failed" {
            let receipt = &t["remote"]["receipt"];
            if receipt["operationId"] != op
                || receipt["publishedAt"] != start
                || state == "rate_limited"
            {
                return Err("P0 发布回执与唯一原操作不一致".into());
            }
            if t["scenario"] == "publish_timeout" && state == "success" {
                return Err("P0 超时发布必须保留对账记录".into());
            }
        }
        if node == "rollback"
            && (t["remote"]["rollback"]["operationId"] != op
                || t["remote"]["rollback"]["rolledBackAt"] != start
                || state != "success")
        {
            return Err("P0 回滚与台账不一致".into());
        }
        if index >= 4 && t["approval"].is_null() {
            return Err("P0 模拟发布操作缺少独立批准".into());
        }
        total_units += e["units"].as_u64().ok_or("单位扣量无效")?;
        total_calls += 1;
        es.push(e);
    }
    if units != total_units || calls != total_calls {
        return Err("P0 已用预算与台账不一致".into());
    }
    for (i, node) in KINDS.iter().enumerate().filter(|(i, _)| *i != 3) {
        let last = per_node.get(node).and_then(|es| es.last());
        if t["nodes"][i]["status"] == "completed" {
            let e = last.ok_or("P0 完成节点缺少预算台账")?;
            if !["success", "reconciled"].contains(&e["status"].as_str().unwrap_or("")) {
                return Err("P0 完成节点不能使用失败或未知结果".into());
            }
        } else if last.is_some_and(|e| {
            ["success", "reconciled"].contains(&e["status"].as_str().unwrap_or(""))
        }) {
            return Err("P0 成功操作的节点状态不一致".into());
        }
    }
    let review_audit = per_node.get("review_audit").and_then(|es| es.last());
    let edited = t["nodes"][3]["result"].get("content").is_some();
    if edited != review_audit.is_some_and(|e| e["status"] == "success") {
        return Err("P0 审核稿审计与台账不一致".into());
    }
    if !t["remote"]["receipt"].is_null()
        && !per_node.get("publish").is_some_and(|es| {
            es.iter().any(|e| {
                ["success", "unknown", "reconciled"].contains(&e["status"].as_str().unwrap_or(""))
            })
        })
    {
        return Err("P0 远端回执缺少发布台账".into());
    }
    if !t["remote"]["rollback"].is_null() && !per_node.contains_key("rollback") {
        return Err("P0 回滚缺少预算台账".into());
    }
    if unknown > 1 || (unknown == 1) != (t["status"] == "awaiting_reconcile") {
        return Err("P0 未知操作与待对账状态不一致".into());
    }
    if unknown == 1 && (t["nodes"][4]["status"] != "blocked" || !t["nodes"][4]["result"].is_null())
    {
        return Err("P0 未对账发布不能完成".into());
    }
    if t["status"] == "retry_wait" && !ledger.last().is_some_and(|e| e["status"] == "rate_limited")
    {
        return Err("P0 退避状态缺少429记录".into());
    }
    Ok(())
}
pub(super) fn validate_database(c: &Connection) -> Result<()> {
    policy(c)?;
    all_tasks(c, None)?;
    Ok(())
}
pub(super) fn tick(c: &mut Connection, recover: bool) -> Result<usize> {
    // The 250ms idle worker must not re-run every historical HTML audit.
    // Full database validation belongs to open/restore; runnable records are
    // strictly validated again inside their own transaction below.
    let ids = {
        let mut statement = c.prepare(
            "SELECT id,brand_id FROM m_p0_workflows WHERE json_extract(data,'$.status')='running' OR (?1=1 AND json_extract(data,'$.status')='retry_wait') ORDER BY rowid",
        ).map_err(db_error)?;
        let rows = statement
            .query_map([recover], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(db_error)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(db_error)?
    };
    let mut changed = 0;
    for (tid, b) in ids {
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_error)?;
        let mut t = get(&tx, &b, &json!({"taskId":tid}))?;
        if t["status"] != "running" && !(recover && t["status"] == "retry_wait") {
            tx.commit().map_err(db_error)?;
            continue;
        }
        let did = if recover || policy(&tx)?["halted"] == true {
            t["status"] = json!("paused");
            persist(&tx, &mut t)?;
            true
        } else {
            advance(&tx, &mut t)?
        };
        if did {
            tx.execute(
                "INSERT INTO m_audit(occurred_at,action,brand_id,object_id)VALUES(?1,?2,?3,?4)",
                params![
                    now(),
                    if recover {
                        "p0Workflow.pause"
                    } else {
                        "p0Workflow.step"
                    },
                    b,
                    tid
                ],
            )
            .map_err(db_error)?;
            changed += 1;
        }
        tx.commit().map_err(db_error)?;
    }
    Ok(changed)
}
