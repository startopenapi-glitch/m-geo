//! Durable fixed offline fixture graph. No network, model or publication capability.
use super::*;
use imports::keys;
pub(super) const TABLE_SQL:&str="CREATE TABLE IF NOT EXISTS m_offline_workflows(id TEXT PRIMARY KEY,brand_id TEXT NOT NULL,request_id TEXT NOT NULL,data TEXT NOT NULL,UNIQUE(brand_id,request_id),FOREIGN KEY(brand_id) REFERENCES m_brands(id));CREATE INDEX IF NOT EXISTS m_offline_workflows_brand ON m_offline_workflows(brand_id);";
const KINDS: [&str; 5] = ["baseline", "rewrite", "audit", "review", "followup"];
const MAX_BYTES: usize = 1_000_000;
fn questions(p: &Value) -> Result<Vec<String>> {
    let items = p["questions"]
        .as_array()
        .filter(|a| !a.is_empty() && a.len() <= 10)
        .ok_or("请提供1至10条无品牌问题")?;
    let mut out = Vec::new();
    for item in items {
        let q = text(&json!({"q":item}), "q", 500, true)?;
        if out.contains(&q) {
            return Err("问题不能重复".into());
        }
        out.push(q);
    }
    Ok(out)
}
fn bounds(p: &Value) -> Result<(Vec<String>, u32, u32)> {
    let q = questions(p)?;
    let repetitions = p["repetitions"]
        .as_u64()
        .filter(|n| (1..=3).contains(n))
        .ok_or("重复次数须为1至3")? as u32;
    let max = p["budget"]["maxSamples"]
        .as_u64()
        .or_else(|| p["maxSamples"].as_u64())
        .filter(|n| (1..=60).contains(n))
        .ok_or("样本预算须为1至60")? as u32;
    if q.len() as u32 * repetitions * 2 > max {
        return Err("两轮模拟所需样本超过预算".into());
    }
    Ok((q, repetitions, max))
}
fn aliases(br: &Value) -> Vec<String> {
    br["profile"]["aliases"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}
fn neutral(qs: &[String], br: &Value, p: &Value) -> Result<()> {
    let mut names = aliases(br);
    for v in [&br["name"], &p["name"]] {
        if let Some(s) = v.as_str() {
            names.push(s.into());
        }
    }
    for q in qs {
        let q = q.to_lowercase();
        if names
            .iter()
            .any(|n| !n.is_empty() && q.contains(&n.to_lowercase()))
        {
            return Err("问题不能包含品牌名称、别名或完整商品名称".into());
        }
    }
    Ok(())
}
fn create(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    keys(
        p,
        &[
            "brandId",
            "requestId",
            "name",
            "productId",
            "questions",
            "repetitions",
            "maxSamples",
            "evidenceMode",
        ],
    )?;
    choice(p, "evidenceMode", &["simulated"])?;
    let request = guid(p, "requestId")?;
    let pid = guid(p, "productId")?;
    let name = text(p, "name", 200, true)?;
    let (qs, reps, max) = bounds(p)?;
    let br = brand(c, b)?;
    let product = load(c, "m_products", b, &pid)?;
    neutral(&qs, &br, &product)?;
    let existing: Option<String> = c
        .query_row(
            "SELECT data FROM m_offline_workflows WHERE brand_id=?1 AND request_id=?2",
            params![b, request],
            |r| r.get(0),
        )
        .optional()
        .map_err(db_error)?;
    if let Some(data) = existing {
        let mut t = parse_data(data)?;
        validate_record(c, &t)?;
        if t["productId"] != pid
            || t["name"] != name
            || t["questions"] != json!(qs)
            || t["repetitions"] != reps
            || t["budget"]["maxSamples"] != max
        {
            return Err("请求标识已用于另一任务".into());
        }
        t["alreadyExists"] = json!(true);
        return Ok(t);
    }
    confirmed_public(&product)?;
    let count: i64 = c
        .query_row(
            "SELECT count(*) FROM m_offline_workflows WHERE brand_id=?1",
            [b],
            |r| r.get(0),
        )
        .map_err(db_error)?;
    if count >= 100 {
        return Err("每品牌最多保留100个离线任务".into());
    }
    let nodes:Vec<Value>=KINDS.iter().enumerate().map(|(i,k)|json!({"id":k,"kind":k,"dependsOn":if i==0{vec![]}else{vec![KINDS[i-1]]},"status":"pending","result":null,"error":null})).collect();
    let t = json!({"id":id(),"brandId":b,"productId":pid,"productRevision":product["revision"],"requestId":request,"name":name,"revision":1,"status":"paused","questions":qs,"repetitions":reps,"draftId":null,"createdAt":now(),"updatedAt":now(),"error":null,"budget":{"maxSamples":max,"usedSamples":0,"networkRequests":0,"modelCalls":0,"publications":0},"nodes":nodes});
    c.execute(
        "INSERT INTO m_offline_workflows(id,brand_id,request_id,data)VALUES(?1,?2,?3,?4)",
        params![t["id"].as_str(), b, request, t.to_string()],
    )
    .map_err(db_error)?;
    Ok(t)
}
fn get(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    let t = load(c, "m_offline_workflows", b, &guid(p, "taskId")?)?;
    validate_record(c, &t)?;
    Ok(t)
}
fn persist(c: &Connection, t: &mut Value) -> Result<()> {
    t["revision"] = json!(number(t, "revision")?
        .checked_add(1)
        .ok_or("任务版本超限")?);
    t["updatedAt"] = json!(now());
    if t.to_string().len() > MAX_BYTES {
        return Err("任务证据超过限制".into());
    }
    // Keep persistence and strict restore/get invariants identical.
    validate_record(c, t)?;
    update(c, "m_offline_workflows", t)
}
fn current_product(c: &Connection, t: &Value) -> Result<Value> {
    let p = load(
        c,
        "m_products",
        t["brandId"].as_str().ok_or("品牌损坏")?,
        t["productId"].as_str().ok_or("商品损坏")?,
    )?;
    if p["revision"] != t["productRevision"] {
        return Err("商品事实版本已变化，请新建任务".into());
    }
    confirmed_public(&p)?;
    neutral(
        &questions(t)?,
        &brand(c, t["brandId"].as_str().unwrap())?,
        &p,
    )?;
    Ok(p)
}
fn samples(c: &Connection, t: &Value) -> Result<Value> {
    let b = t["brandId"].as_str().ok_or("品牌损坏")?;
    let br = brand(c, b)?;
    let p = current_product(c, t)?;
    offline_geo::simulate_monitor(
        &questions(t)?,
        t["repetitions"].as_u64().unwrap() as u32,
        br["name"].as_str().unwrap_or(""),
        &aliases(&br),
        p["name"].as_str().unwrap_or(""),
    )
}
fn generated_html(d: &Value) -> String {
    format!("<!doctype html><html><head><title>{}</title><meta name=\"description\" content=\"已核验公开商品资料的本地结构整理\"></head><body><h1>{}</h1><pre>{}</pre></body></html>",escape(d["title"].as_str().unwrap_or("")),escape(d["title"].as_str().unwrap_or("")),escape(d["body"].as_str().unwrap_or("")))
}
fn run_node(c: &Connection, t: &mut Value, i: usize) -> Result<bool> {
    let b = t["brandId"].as_str().ok_or("品牌损坏")?.to_owned();
    let result = match i {
        0 => samples(c, t)?,
        1 => {
            let br = brand(c, &b)?;
            let ctx = prepare(
                c,
                &b,
                &br,
                &json!({"productId":t["productId"],"kind":"description"}),
            )?;
            let body = offline_geo::rewrite_body(&local_body(&ctx)?)?;
            let draft = insert_draft(c, &ctx, &body, "local")?;
            c.execute(
                "DELETE FROM m_ai_preparations WHERE hash=?1",
                [fingerprint(&ctx)],
            )
            .map_err(db_error)?;
            t["draftId"] = draft["id"].clone();
            json!({"draftId":draft["id"],"title":draft["title"]})
        }
        2 => {
            let d = load(
                c,
                "m_drafts",
                &b,
                t["draftId"].as_str().ok_or("缺少候选稿")?,
            )?;
            current_draft(c, &b, &d, false)?;
            offline_geo::audit_html(&generated_html(&d))?
        }
        3 => {
            let d = load(
                c,
                "m_drafts",
                &b,
                t["draftId"].as_str().ok_or("缺少候选稿")?,
            )?;
            if d["reviewedAt"].is_null() {
                t["status"] = json!("awaiting_review");
                t["nodes"][i]["status"] = json!("blocked");
                return Ok(false);
            }
            current_draft(c, &b, &d, true)?;
            json!({"draftId":d["id"],"draftRevision":d["revision"],"reviewedAt":d["reviewedAt"]})
        }
        4 => {
            reviewed_draft(c, t)?;
            // Both rounds replay the task's frozen deterministic fixture. A later
            // brand-profile edit must not rewrite historical comparison evidence.
            let mut result = t["nodes"][0]["result"].clone();
            result["mentionDelta"] = json!(
                result["mentionCount"].as_i64().ok_or("判分损坏")?
                    - t["nodes"][0]["result"]["mentionCount"]
                        .as_i64()
                        .ok_or("基线损坏")?
            );
            result["comparisonLabel"] = json!("同一确定性模拟夹具的离线比较；不代表真实可见性提升");
            result
        }
        _ => return Err("不支持的任务节点".into()),
    };
    if i == 0 || i == 4 {
        let used = t["budget"]["usedSamples"].as_u64().ok_or("预算损坏")?;
        let count = result["sampleCount"].as_u64().ok_or("样本数量损坏")?;
        if used + count > t["budget"]["maxSamples"].as_u64().unwrap() {
            return Err("样本预算不足".into());
        }
        t["budget"]["usedSamples"] = json!(used + count);
    }
    t["nodes"][i]["result"] = result;
    t["nodes"][i]["status"] = json!("completed");
    t["nodes"][i]["error"] = Value::Null;
    t["status"] = json!(if i == 4 { "completed" } else { "running" });
    Ok(true)
}
fn advance(c: &Connection, t: &mut Value) -> Result<bool> {
    if !["running", "awaiting_review"].contains(&t["status"].as_str().unwrap_or("")) {
        return Ok(false);
    }
    let i = t["nodes"]
        .as_array()
        .ok_or("节点损坏")?
        .iter()
        .position(|n| n["status"] != "completed")
        .ok_or("节点状态损坏")?;
    // A savepoint ensures a failed node cannot leave an orphan draft or spent budget.
    c.execute_batch("SAVEPOINT offline_workflow_node")
        .map_err(db_error)?;
    let before = t.clone();
    let outcome = current_product(c, t).and_then(|_| run_node(c, t, i));
    match outcome {
        Ok(changed) => {
            c.execute_batch("RELEASE offline_workflow_node")
                .map_err(db_error)?;
            if changed || *t != before {
                persist(c, t)?;
                return Ok(true);
            }
            Ok(false)
        }
        Err(error) => {
            c.execute_batch("ROLLBACK TO offline_workflow_node; RELEASE offline_workflow_node")
                .map_err(db_error)?;
            *t = before;
            t["status"] = json!("failed");
            t["error"] = json!(error);
            t["nodes"][i]["status"] = json!("failed");
            t["nodes"][i]["error"] = t["error"].clone();
            persist(c, t)?;
            Ok(true)
        }
    }
}
fn reviewed_draft(c: &Connection, t: &Value) -> Result<Value> {
    let b = t["brandId"].as_str().ok_or("品牌损坏")?;
    let d = load(
        c,
        "m_drafts",
        b,
        t["draftId"].as_str().ok_or("尚未生成候选稿")?,
    )?;
    current_product(c, t)?;
    current_draft(c, b, &d, true)?;
    let review = &t["nodes"][3];
    if review["status"] != "completed"
        || review["result"]["draftId"] != d["id"]
        || review["result"]["draftRevision"] != d["revision"]
        || review["result"]["reviewedAt"] != d["reviewedAt"]
    {
        return Err("当前稿件版本尚未通过本任务人工审核，请新建任务或完成审核节点".into());
    }
    Ok(d)
}
fn export_workflow(c: &Connection, b: &str, t: &Value) -> Result<Value> {
    if t["status"] != "completed" {
        return Err("请先完成任务与人工审核".into());
    }
    let d = reviewed_draft(c, t)?;
    let mut files = vec![
        json!({"path":"offline-workflow/report.json","content":serde_json::to_string_pretty(t).map_err(|_|"报告生成失败")?}),
        json!({"path":"offline-workflow/report.md","content":format!("# 离线模拟任务证据\n\n{}\n\n全部样本来自确定性 simulated / fixture，不代表真实搜索或模型表现。\n联网请求：0；模型调用：0；发布：0。\n\n基线样本：{}；复测样本：{}；提及差值：0。\n",t["name"].as_str().unwrap_or(""),t["nodes"][0]["result"]["sampleCount"],t["nodes"][4]["result"]["sampleCount"])}),
    ];
    for format in ["markdown", "html"] {
        let e = super::export(
            c,
            b,
            &json!({"id":d["id"],"expectedRevision":d["revision"],"format":format}),
        )?;
        files.push(json!({"path":format!("offline-workflow/{}",e["fileName"].as_str().unwrap_or("draft.md")),"content":e["content"]}));
    }
    Ok(json!({"files":files}))
}
pub(super) fn dispatch(c: &Connection, b: &str, a: &str, p: &Value) -> Result<Value> {
    if a == "offlineGeo.audit" {
        keys(p, &["brandId", "html"])?;
        let html = raw(p, "html", 262144, true)?;
        if html.len() > 262144 {
            return Err("HTML 超过256KiB".into());
        }
        return offline_geo::audit_html(&html);
    }
    if a == "offlineWorkflow.create" {
        return create(c, b, p);
    }
    if a == "offlineWorkflow.list" {
        keys(p, &["brandId"])?;
        let mut st=c.prepare("SELECT data FROM m_offline_workflows WHERE brand_id=?1 ORDER BY rowid DESC LIMIT 100").map_err(db_error)?;
        let rows = st
            .query_map([b], |r| r.get::<_, String>(0))
            .map_err(db_error)?;
        let mut tasks = vec![];
        for row in rows {
            let t = parse_data(row.map_err(db_error)?)?;
            validate_record(c, &t)?;
            tasks.push(t)
        }
        return Ok(json!({"tasks":tasks}));
    }
    keys(p, &["brandId", "taskId", "expectedRevision"])?;
    let mut t = get(c, b, p)?;
    if a == "offlineWorkflow.get" {
        return Ok(t);
    }
    revision(&t, p)?;
    match a {
        "offlineWorkflow.export" => return export_workflow(c, b, &t),
        "offlineWorkflow.step" => {
            advance(c, &mut t)?;
            return Ok(t);
        }
        "offlineWorkflow.resume" => {
            if !["paused", "awaiting_review"].contains(&t["status"].as_str().unwrap_or("")) {
                return Err("此状态不能继续任务".into());
            }
            current_product(c, &t)?;
            t["status"] = json!("running");
        }
        "offlineWorkflow.pause" => {
            if !["running", "awaiting_review", "paused"]
                .contains(&t["status"].as_str().unwrap_or(""))
            {
                return Err("此状态不能暂停任务".into());
            }
            if t["status"] == "paused" {
                return Ok(t);
            }
            t["status"] = json!("paused");
        }
        "offlineWorkflow.cancel" => {
            if t["status"] == "completed" {
                return Err("已完成的任务不能取消".into());
            }
            if t["status"] == "cancelled" {
                return Ok(t);
            }
            t["status"] = json!("cancelled");
        }
        "offlineWorkflow.retry" => {
            if t["status"] != "failed" {
                return Err("仅失败任务可重试".into());
            }
            current_product(c, &t)?;
            t["status"] = json!("paused");
            t["error"] = Value::Null;
            for node in t["nodes"].as_array_mut().unwrap() {
                if node["status"] == "failed" {
                    node["status"] = json!("pending");
                    node["error"] = Value::Null;
                }
            }
        }
        _ => return Err("不支持的离线任务操作".into()),
    }
    persist(c, &mut t)?;
    Ok(t)
}
fn nonnegative(v: &Value, k: &str) -> Result<u64> {
    v[k].as_u64()
        .ok_or_else(|| format!("离线证据字段 {k} 无效"))
}
fn zero(v: &Value, k: &str) -> Result<()> {
    if nonnegative(v, k)? != 0 {
        return Err("离线任务不能包含真实请求、模型调用或发布".into());
    }
    Ok(())
}
fn nullable_error(v: &Value) -> Result<()> {
    match v.get("error") {
        Some(Value::Null) => Ok(()),
        Some(Value::String(s))
            if !s.is_empty() && s.chars().count() <= 2000 && !s.contains('\0') =>
        {
            Ok(())
        }
        _ => Err("离线任务错误字段无效".into()),
    }
}
fn validate_samples(t: &Value, r: &Value, followup: bool) -> Result<()> {
    let mut allowed = vec![
        "sampleCount",
        "mentionCount",
        "citationCount",
        "samples",
        "networkRequests",
        "modelCalls",
    ];
    if followup {
        allowed.extend(["mentionDelta", "comparisonLabel"])
    }
    keys(r, &allowed)?;
    zero(r, "networkRequests")?;
    zero(r, "modelCalls")?;
    let qs = questions(t)?;
    let expected = qs.len() * t["repetitions"].as_u64().unwrap() as usize;
    let samples = r["samples"]
        .as_array()
        .filter(|a| a.len() == expected)
        .ok_or("离线样本数量错误")?;
    if nonnegative(r, "sampleCount")? != expected as u64 {
        return Err("离线样本数量不一致".into());
    }
    let mut mentions = 0;
    let mut citations = 0;
    let mut question_counts = std::collections::HashMap::new();
    for s in samples {
        keys(
            s,
            &[
                "question",
                "answer",
                "citations",
                "mentioned",
                "evidenceMode",
                "surface",
            ],
        )?;
        let q = text(s, "question", 500, true)?;
        if !qs.contains(&q) {
            return Err("离线样本问题不属于任务".into());
        }
        *question_counts.entry(q).or_insert(0usize) += 1;
        text(s, "answer", 30000, true)?;
        choice(s, "evidenceMode", &["simulated"])?;
        choice(s, "surface", &["fixture"])?;
        if boolean(s, "mentioned")? {
            mentions += 1;
        }
        let urls = s["citations"]
            .as_array()
            .filter(|a| a.len() <= 10)
            .ok_or("离线引用无效")?;
        for u in urls {
            let s = u.as_str().ok_or("离线引用无效")?;
            let url = url::Url::parse(s).map_err(|_| "离线引用必须为示例域名")?;
            // Inspect the original authority too: URL normalization removes an
            // explicit default :443 port before Url::port() can report it.
            let authority = s
                .strip_prefix("https://")
                .and_then(|rest| rest.split(['/', '?', '#']).next());
            if s.len() > 2000
                || url.scheme() != "https"
                || url.host_str() != Some("example.invalid")
                || !url.username().is_empty()
                || url.password().is_some()
                || url.port().is_some()
                || authority != Some("example.invalid")
            {
                return Err("离线引用必须为无凭据、无端口的 example.invalid 示例域名".into());
            }
        }
        citations += urls.len();
    }
    if question_counts.len() != qs.len()
        || question_counts
            .values()
            .any(|n| *n != t["repetitions"].as_u64().unwrap() as usize)
        || nonnegative(r, "mentionCount")? != mentions
        || nonnegative(r, "citationCount")? != citations as u64
    {
        return Err("离线判分与样本不一致".into());
    }
    if followup {
        if r["mentionDelta"] != 0 || r["samples"] != t["nodes"][0]["result"]["samples"] {
            return Err("确定性离线夹具不得虚构提升".into());
        }
        text(r, "comparisonLabel", 500, true)?;
    }
    Ok(())
}
fn validate_audit(r: &Value) -> Result<()> {
    keys(
        r,
        &[
            "scope",
            "checks",
            "passed",
            "warning",
            "blocking",
            "networkRequests",
        ],
    )?;
    choice(r, "scope", &["offline_html"])?;
    zero(r, "networkRequests")?;
    let checks = r["checks"]
        .as_array()
        .filter(|a| a.len() == 6)
        .ok_or("离线检查项错误")?;
    let mut ids = HashSet::new();
    let mut tally = [0u64; 3];
    for check in checks {
        keys(check, &["id", "label", "status", "detail"])?;
        let cid = choice(
            check,
            "id",
            &[
                "title",
                "description",
                "h1",
                "body",
                "json_ld",
                "product_consistency",
            ],
        )?;
        if !ids.insert(cid) {
            return Err("重复检查项".into());
        }
        text(check, "label", 300, true)?;
        text(check, "detail", 2000, true)?;
        let status = choice(check, "status", &["pass", "warning", "fail"])?;
        tally[match status.as_str() {
            "pass" => 0,
            "warning" => 1,
            _ => 2,
        }] += 1;
    }
    for (i, k) in ["passed", "warning", "blocking"].iter().enumerate() {
        if nonnegative(r, k)? != tally[i] {
            return Err("离线检查计数错误".into());
        }
    }
    Ok(())
}
fn validate_record(c: &Connection, t: &Value) -> Result<()> {
    keys(
        t,
        &[
            "id",
            "brandId",
            "productId",
            "productRevision",
            "requestId",
            "name",
            "revision",
            "status",
            "questions",
            "repetitions",
            "draftId",
            "createdAt",
            "updatedAt",
            "error",
            "budget",
            "nodes",
        ],
    )?;
    for k in ["id", "brandId", "productId", "requestId"] {
        let s = guid(t, k)?;
        if t[k] != s {
            return Err("离线任务标识不规范".into());
        }
    }
    number(t, "revision")?;
    number(t, "productRevision")?;
    text(t, "name", 200, true)?;
    date_time(t, "createdAt")?;
    date_time(t, "updatedAt")?;
    nullable_error(t)?;
    let status = choice(
        t,
        "status",
        &[
            "paused",
            "running",
            "awaiting_review",
            "completed",
            "failed",
            "cancelled",
        ],
    )?;
    let (_, reps, _) = bounds(t)?;
    let b = t["brandId"].as_str().unwrap();
    let product = load(c, "m_products", b, t["productId"].as_str().unwrap())?;
    if number(&product, "revision")? < number(t, "productRevision")? {
        return Err("离线任务商品版本来自未来".into());
    }
    let budget = &t["budget"];
    keys(
        budget,
        &[
            "maxSamples",
            "usedSamples",
            "networkRequests",
            "modelCalls",
            "publications",
        ],
    )?;
    for k in ["networkRequests", "modelCalls", "publications"] {
        zero(budget, k)?;
    }
    let nodes = t["nodes"]
        .as_array()
        .filter(|a| a.len() == 5)
        .ok_or("离线任务图错误")?;
    let mut incomplete = false;
    let mut completed = 0;
    let mut used = 0;
    let mut failed = 0;
    for (i, n) in nodes.iter().enumerate() {
        keys(n, &["id", "kind", "dependsOn", "status", "result", "error"])?;
        if n["id"] != KINDS[i]
            || n["kind"] != KINDS[i]
            || n["dependsOn"]
                != if i == 0 {
                    json!([])
                } else {
                    json!([KINDS[i - 1]])
                }
        {
            return Err("离线任务仅支持固定节点图".into());
        }
        nullable_error(n)?;
        let ns = choice(n, "status", &["pending", "completed", "blocked", "failed"])?;
        if ns == "completed" {
            if incomplete {
                return Err("离线节点绕过依赖".into());
            }
            completed += 1;
            if !n["error"].is_null() {
                return Err("已完成节点含错误".into());
            }
            let r = &n["result"];
            match i {
                0 | 4 => {
                    validate_samples(t, r, i == 4)?;
                    used += questions(t)?.len() as u64 * reps as u64;
                }
                1 => {
                    keys(r, &["draftId", "title"])?;
                    if r["draftId"] != t["draftId"] {
                        return Err("候选稿关联不一致".into());
                    }
                    text(r, "title", 300, true)?;
                }
                2 => validate_audit(r)?,
                3 => {
                    keys(r, &["draftId", "draftRevision", "reviewedAt"])?;
                    if r["draftId"] != t["draftId"] {
                        return Err("审核稿件关联不一致".into());
                    }
                    number(r, "draftRevision")?;
                    date_time(r, "reviewedAt")?;
                }
                _ => unreachable!(),
            }
        } else {
            if incomplete && ns != "pending" {
                return Err("离线后续节点状态错误".into());
            }
            incomplete = true;
            if !n["result"].is_null() {
                return Err("未完成节点不能包含结果".into());
            }
            if ns == "blocked" && i != 3 {
                return Err("仅人工审核节点可阻塞".into());
            }
            if ns == "failed" {
                failed += 1;
                if n["error"].is_null() {
                    return Err("失败节点缺少错误".into());
                }
            } else if !n["error"].is_null() {
                return Err("未失败节点含错误".into());
            }
        }
    }
    if nonnegative(budget, "usedSamples")? != used || used > nonnegative(budget, "maxSamples")? {
        return Err("离线预算与节点不一致".into());
    }
    if status == "completed" && completed != 5
        || status != "completed" && completed == 5
        || status == "awaiting_review" && (completed != 3 || nodes[3]["status"] != "blocked")
        || status == "failed" && (failed != 1 || t["error"].is_null())
        || status != "failed" && status != "cancelled" && (failed != 0 || !t["error"].is_null())
    {
        return Err("离线任务状态与节点不一致".into());
    }
    if completed >= 2 {
        let did = guid(t, "draftId")?;
        let draft = load(c, "m_drafts", b, &did)?;
        if draft["productId"] != t["productId"]
            || draft["sourceRevision"] != t["productRevision"]
            || draft["generationMode"] != "local"
        {
            return Err("离线候选稿来源错误".into());
        }
        // Later normal edits invalidate export, but must not invalidate historical backups.
        if completed >= 4 {
            let reviewed = &nodes[3]["result"];
            let review_revision = number(reviewed, "draftRevision")?;
            let draft_revision = number(&draft, "revision")?;
            if review_revision > draft_revision
                || (review_revision == draft_revision
                    && (draft["reviewedAt"].is_null()
                        || draft["reviewedAt"] != reviewed["reviewedAt"]))
            {
                return Err("离线审核结果缺少相应的原生稿件审核记录".into());
            }
        }
    } else if !t["draftId"].is_null() {
        return Err("候选稿出现于改写前".into());
    }
    Ok(())
}
pub(super) fn validate_database(c: &Connection) -> Result<()> {
    let mut st = c
        .prepare("SELECT id,brand_id,request_id,data FROM m_offline_workflows")
        .map_err(db_error)?;
    let rows = st
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })
        .map_err(db_error)?;
    let mut counts = std::collections::HashMap::new();
    for row in rows {
        let (id, b, request, data) = row.map_err(db_error)?;
        if data.len() > MAX_BYTES {
            return Err("离线记录超限".into());
        }
        let t = strict_json::parse(&data)?;
        if t["id"] != id || t["brandId"] != b || t["requestId"] != request {
            return Err("离线任务身份与索引不一致".into());
        }
        validate_record(c, &t)?;
        *counts.entry(b).or_insert(0usize) += 1;
    }
    if counts.values().any(|n| *n > 100) {
        return Err("品牌离线任务超过100项".into());
    }
    Ok(())
}
pub(super) fn tick(c: &mut Connection, recover: bool) -> Result<usize> {
    let ids = {
        let mut st=c.prepare("SELECT id,brand_id FROM m_offline_workflows WHERE json_extract(data,'$.status')='running'").map_err(db_error)?;
        let rows = st
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(db_error)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(db_error)?
    };
    let mut changed = 0;
    for (id, b) in ids {
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_error)?;
        let mut t = get(&tx, &b, &json!({"taskId":id}))?;
        let did_change = if recover {
            t["status"] = json!("paused");
            persist(&tx, &mut t)?;
            true
        } else {
            advance(&tx, &mut t)?
        };
        if did_change {
            tx.execute(
                "INSERT INTO m_audit(occurred_at,action,brand_id,object_id)VALUES(?1,?2,?3,?4)",
                params![
                    now(),
                    if recover {
                        "offlineWorkflow.pause"
                    } else {
                        "offlineWorkflow.step"
                    },
                    b,
                    id
                ],
            )
            .map_err(db_error)?;
        }
        tx.commit().map_err(db_error)?;
        if did_change {
            changed += 1;
        }
    }
    Ok(changed)
}
