//! Durable read-only inspection schedules. The caller owns the transaction and decides when `run_due` is invoked.
//! This module has no timer, network client, model call, command execution, or publication write.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

pub(super) const TABLE_SQL: &str = "CREATE TABLE IF NOT EXISTS m_automation_tasks(id TEXT PRIMARY KEY,brand_id TEXT NOT NULL,dedup TEXT NOT NULL,data TEXT NOT NULL,UNIQUE(brand_id,dedup),FOREIGN KEY(brand_id) REFERENCES m_brands(id));CREATE INDEX IF NOT EXISTS m_automation_tasks_brand ON m_automation_tasks(brand_id);";
const FORMAT: &str = "m-geo-automation-task-v1";
const INVALID: &str = "巡检任务资料无效，请检查任务或备份";
const MAX_TASKS: usize = 50;
const MAX_NODES: usize = 8;
const MAX_REPORTS: usize = 5;
const MAX_REMINDERS_PER_NODE: usize = 50;
const MAX_TASK_BYTES: usize = 2_100_000;
const KINDS: [&str; 3] = [
    "fact_expiry_scan",
    "publication_readback_due",
    "visibility_retest_reminder",
];

fn exact(v: &Value, fields: &[&str]) -> Result<()> {
    let object = v.as_object().ok_or(INVALID)?;
    if object.len() != fields.len() || fields.iter().any(|key| !object.contains_key(*key)) {
        return Err(INVALID.into());
    }
    Ok(())
}

fn canonical_time(value: &Value, key: &str) -> Result<DateTime<Utc>> {
    let text = value[key].as_str().ok_or(INVALID)?;
    if text.len() > 80 || text.contains('.') {
        return Err(INVALID.into());
    }
    let parsed = DateTime::parse_from_rfc3339(text).map_err(|_| INVALID)?;
    if parsed.timestamp_subsec_nanos() != 0
        || parsed
            .with_timezone(&Utc)
            .to_rfc3339_opts(SecondsFormat::Secs, true)
            != text
    {
        return Err(INVALID.into());
    }
    Ok(parsed.with_timezone(&Utc))
}

fn parse_time(text: &str) -> Result<DateTime<Utc>> {
    canonical_time(&json!({"at":text}), "at")
}

fn next_time(at: DateTime<Utc>, interval: i64) -> Result<String> {
    Ok(at
        .checked_add_signed(chrono::Duration::minutes(interval))
        .ok_or("巡检时间超出支持范围")?
        .to_rfc3339_opts(SecondsFormat::Secs, true))
}

fn canonical_nodes(input: &Value) -> Result<Value> {
    let nodes = input.as_array().ok_or("巡检节点必须为列表")?;
    if nodes.is_empty() || nodes.len() > MAX_NODES {
        return Err(format!("每个巡检任务需要1至{MAX_NODES}个节点"));
    }
    let mut output = Vec::with_capacity(nodes.len());
    for node in nodes {
        exact(node, &["id", "kind", "dependsOn"])?;
        let node_id = text(node, "id", 80, true)?;
        if node_id.chars().any(char::is_control) {
            return Err("巡检节点标识含非法字符".into());
        }
        let kind = choice(node, "kind", &KINDS)?;
        let dependencies = node["dependsOn"].as_array().ok_or("节点依赖必须为列表")?;
        if dependencies.len() > MAX_NODES {
            return Err("节点依赖过多".into());
        }
        let mut seen = BTreeSet::new();
        let mut canonical = Vec::with_capacity(dependencies.len());
        for dependency in dependencies {
            let dependency = dependency.as_str().ok_or("节点依赖必须为节点标识")?;
            if dependency.is_empty()
                || dependency.chars().count() > 80
                || dependency.chars().any(char::is_control)
                || dependency == node_id
                || !seen.insert(dependency)
            {
                return Err("节点依赖重复、无效或引用自身".into());
            }
            canonical.push(json!(dependency));
        }
        output.push(json!({"id":node_id,"kind":kind,"dependsOn":canonical}));
    }
    let value = json!(output);
    validate_dag(&value)?;
    Ok(value)
}

fn validate_dag(nodes: &Value) -> Result<Vec<usize>> {
    let nodes = nodes.as_array().ok_or(INVALID)?;
    if nodes.is_empty() || nodes.len() > MAX_NODES {
        return Err(INVALID.into());
    }
    let mut indexes = BTreeMap::new();
    for (index, node) in nodes.iter().enumerate() {
        exact(node, &["id", "kind", "dependsOn"])?;
        let node_id = node["id"].as_str().ok_or(INVALID)?;
        if node_id.is_empty()
            || node_id.chars().count() > 80
            || node_id.chars().any(char::is_control)
            || !KINDS.contains(&node["kind"].as_str().ok_or(INVALID)?)
            || indexes.insert(node_id, index).is_some()
        {
            return Err("巡检节点标识重复或节点类型不受支持".into());
        }
    }
    for node in nodes {
        let dependencies = node["dependsOn"].as_array().ok_or(INVALID)?;
        let mut seen = BTreeSet::new();
        for dependency in dependencies {
            let dependency = dependency.as_str().ok_or(INVALID)?;
            if !indexes.contains_key(dependency)
                || dependency == node["id"].as_str().ok_or(INVALID)?
                || !seen.insert(dependency)
            {
                return Err("节点依赖不存在、重复或引用自身".into());
            }
        }
    }
    fn visit(
        index: usize,
        nodes: &[Value],
        indexes: &BTreeMap<&str, usize>,
        marks: &mut [u8],
        order: &mut Vec<usize>,
    ) -> Result<()> {
        match marks[index] {
            2 => return Ok(()),
            1 => return Err("巡检节点依赖必须是无环 DAG".into()),
            _ => {}
        }
        marks[index] = 1;
        for dependency in nodes[index]["dependsOn"].as_array().ok_or(INVALID)? {
            let dependency = dependency.as_str().ok_or(INVALID)?;
            visit(
                *indexes.get(dependency).ok_or(INVALID)?,
                nodes,
                indexes,
                marks,
                order,
            )?;
        }
        marks[index] = 2;
        order.push(index);
        Ok(())
    }
    let mut marks = vec![0; nodes.len()];
    let mut order = Vec::with_capacity(nodes.len());
    for index in 0..nodes.len() {
        visit(index, nodes, &indexes, &mut marks, &mut order)?;
    }
    Ok(order)
}

fn create_fingerprint(name: &str, interval: i64, nodes: &Value) -> String {
    fingerprint(&json!({"name":name,"intervalMinutes":interval,"nodes":nodes}))
}

fn validate_report(report: &Value) -> Result<()> {
    exact(
        report,
        &[
            "id",
            "ranAt",
            "nodes",
            "modelCalls",
            "modelFeeMinor",
            "networkRequests",
            "publicationWrites",
        ],
    )?;
    Uuid::parse_str(report["id"].as_str().ok_or(INVALID)?).map_err(|_| INVALID)?;
    canonical_time(report, "ranAt")?;
    if report["modelCalls"] != 0
        || report["modelFeeMinor"] != 0
        || report["networkRequests"] != 0
        || report["publicationWrites"] != 0
    {
        return Err("只读巡检报告的副作用计数必须全部为0".into());
    }
    let nodes = report["nodes"].as_array().ok_or(INVALID)?;
    if nodes.is_empty() || nodes.len() > MAX_NODES {
        return Err(INVALID.into());
    }
    for node in nodes {
        exact(node, &["nodeId", "kind", "reminders", "omitted"])?;
        if !KINDS.contains(&node["kind"].as_str().ok_or(INVALID)?)
            || node["omitted"].as_u64().is_none()
        {
            return Err(INVALID.into());
        }
        let reminders = node["reminders"].as_array().ok_or(INVALID)?;
        if reminders.len() > MAX_REMINDERS_PER_NODE {
            return Err(INVALID.into());
        }
        for reminder in reminders {
            exact(reminder, &["kind", "title", "detail", "due", "refs"])?;
            if reminder["title"].as_str().is_none()
                || reminder["detail"].as_str().is_none()
                || reminder["due"].as_bool().is_none()
            {
                return Err(INVALID.into());
            }
            let refs = reminder["refs"].as_object().ok_or(INVALID)?;
            if refs.len() > 20
                || refs.values().any(|value| {
                    !(value.is_string() || value.is_boolean() || value.is_i64() || value.is_null())
                })
            {
                return Err(INVALID.into());
            }
        }
    }
    Ok(())
}

fn validate_task(c: &Connection, task: &Value) -> Result<()> {
    if task.to_string().len() > MAX_TASK_BYTES {
        return Err("巡检任务与报告超过2.1MB，请检查备份".into());
    }
    exact(
        task,
        &[
            "format",
            "id",
            "brandId",
            "requestId",
            "createFingerprint",
            "name",
            "intervalMinutes",
            "nodes",
            "status",
            "revision",
            "createdAt",
            "updatedAt",
            "nextRunAt",
            "lastRunAt",
            "totalRuns",
            "missedRuns",
            "reports",
        ],
    )?;
    if task["format"] != FORMAT {
        return Err("巡检任务格式不支持".into());
    }
    for key in ["id", "brandId", "requestId"] {
        let value = task[key].as_str().ok_or(INVALID)?;
        if Uuid::parse_str(value).map_err(|_| INVALID)?.to_string() != value {
            return Err("巡检任务标识不规范".into());
        }
    }
    brand(c, task["brandId"].as_str().ok_or(INVALID)?)?;
    let name = text(task, "name", 100, true)?;
    if task["name"] != name {
        return Err("巡检任务名称不规范".into());
    }
    let interval = number(task, "intervalMinutes")?;
    if !(15..=525_600).contains(&interval) {
        return Err("巡检间隔必须为15分钟至365天".into());
    }
    validate_dag(&task["nodes"])?;
    if task["createFingerprint"].as_str().ok_or(INVALID)?
        != create_fingerprint(&name, interval, &task["nodes"])
    {
        return Err("巡检任务创建摘要不一致".into());
    }
    choice(task, "status", &["active", "paused", "cancelled"])?;
    number(task, "revision")?;
    let created = canonical_time(task, "createdAt")?;
    let updated = canonical_time(task, "updatedAt")?;
    let next = canonical_time(task, "nextRunAt")?;
    if updated < created || next < created {
        return Err("巡检任务时间顺序无效".into());
    }
    if !task["lastRunAt"].is_null() {
        let last = canonical_time(task, "lastRunAt")?;
        if last < created || last > updated {
            return Err("巡检任务最近运行时间无效".into());
        }
    }
    let total_runs = task["totalRuns"].as_u64().ok_or(INVALID)?;
    if task["missedRuns"].as_u64().is_none() || (total_runs == 0) != task["lastRunAt"].is_null() {
        return Err(INVALID.into());
    }
    let reports = task["reports"].as_array().ok_or(INVALID)?;
    if reports.len() > MAX_REPORTS || reports.len() as u64 > total_runs {
        return Err("巡检报告数量无效".into());
    }
    let mut previous = None;
    for report in reports {
        validate_report(report)?;
        let ran_at = canonical_time(report, "ranAt")?;
        if ran_at < created || ran_at > updated || previous.is_some_and(|time| ran_at < time) {
            return Err("巡检报告时间顺序无效".into());
        }
        previous = Some(ran_at);
    }
    if let Some(last) = reports.last() {
        if task["lastRunAt"] != last["ranAt"] {
            return Err("巡检最近报告与运行时间不一致".into());
        }
    } else if total_runs > 0 {
        return Err("巡检历史报告缺失".into());
    }
    Ok(())
}

fn read_task(c: &Connection, brand_id: &str, task_id: &str) -> Result<Value> {
    let row = c
        .query_row(
            "SELECT dedup,data FROM m_automation_tasks WHERE id=?1 AND brand_id=?2",
            params![task_id, brand_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(db_error)?
        .ok_or("记录不存在或不属于当前品牌")?;
    if row.1.len() > MAX_TASK_BYTES {
        return Err(INVALID.into());
    }
    let task = parse_data(row.1)?;
    if task["id"] != task_id || task["brandId"] != brand_id || task["requestId"] != row.0 {
        return Err("巡检任务存储身份不一致".into());
    }
    validate_task(c, &task)?;
    Ok(task)
}

fn write_task(c: &Connection, task: &Value) -> Result<()> {
    validate_task(c, task)?;
    let changed = c
        .execute(
            "UPDATE m_automation_tasks SET data=?1 WHERE id=?2 AND brand_id=?3 AND dedup=?4",
            params![
                task.to_string(),
                task["id"].as_str(),
                task["brandId"].as_str(),
                task["requestId"].as_str()
            ],
        )
        .map_err(db_error)?;
    if changed != 1 {
        return Err(INVALID.into());
    }
    Ok(())
}

fn request(c: &Connection, brand_id: &str, payload: &Value, fields: &[&str]) -> Result<()> {
    exact(payload, fields)?;
    if payload["brandId"] != brand_id {
        return Err(INVALID.into());
    }
    brand(c, brand_id)?;
    Ok(())
}

fn mutation(c: &Connection, brand_id: &str, payload: &Value) -> Result<Value> {
    request(
        c,
        brand_id,
        payload,
        &["brandId", "taskId", "expectedRevision"],
    )?;
    let task = read_task(c, brand_id, &guid(payload, "taskId")?)?;
    if number(&task, "revision")? != number(payload, "expectedRevision")? {
        return Err("任务已被其他操作修改，请重新加载后再操作".into());
    }
    Ok(task)
}

fn changed_at(task: &mut Value, at: String) -> Result<()> {
    let previous = canonical_time(task, "updatedAt")?;
    let requested = parse_time(&at)?;
    let effective = if requested < previous {
        previous.to_rfc3339_opts(SecondsFormat::Secs, true)
    } else {
        at
    };
    task["revision"] = json!(number(task, "revision")?
        .checked_add(1)
        .ok_or("任务版本超出范围")?);
    task["updatedAt"] = json!(effective);
    Ok(())
}

fn changed(task: &mut Value) -> Result<()> {
    changed_at(task, now())
}

pub(super) fn create(c: &Connection, brand_id: &str, payload: &Value) -> Result<Value> {
    request(
        c,
        brand_id,
        payload,
        &["brandId", "requestId", "name", "intervalMinutes", "nodes"],
    )?;
    let request_id = guid(payload, "requestId")?;
    if Uuid::parse_str(&request_id)
        .map_err(|_| INVALID)?
        .to_string()
        != request_id
    {
        return Err("请求标识须使用规范 UUID".into());
    }
    let name = text(payload, "name", 100, true)?;
    let interval = number(payload, "intervalMinutes")?;
    if !(15..=525_600).contains(&interval) {
        return Err("巡检间隔不得少于15分钟，且不得超过365天".into());
    }
    let nodes = canonical_nodes(&payload["nodes"])?;
    let create_fingerprint = create_fingerprint(&name, interval, &nodes);
    let prior = c
        .query_row(
            "SELECT id FROM m_automation_tasks WHERE brand_id=?1 AND dedup=?2",
            params![brand_id, request_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(db_error)?;
    if let Some(task_id) = prior {
        let mut task = read_task(c, brand_id, &task_id)?;
        if task["createFingerprint"] != create_fingerprint {
            return Err("此请求标识已用于不同巡检内容，请重新加载原任务".into());
        }
        task["alreadyExists"] = json!(true);
        return Ok(task);
    }
    let count: i64 = c
        .query_row(
            "SELECT count(*) FROM m_automation_tasks WHERE brand_id=?1",
            [brand_id],
            |row| row.get(0),
        )
        .map_err(db_error)?;
    if count >= MAX_TASKS as i64 {
        return Err("每个品牌最多保存50个巡检任务".into());
    }
    let created_at = parse_time(&now())?;
    let task = json!({
        "format":FORMAT,
        "id":id(),
        "brandId":brand_id,
        "requestId":request_id,
        "createFingerprint":create_fingerprint,
        "name":name,
        "intervalMinutes":interval,
        "nodes":nodes,
        "status":"active",
        "revision":1,
        "createdAt":created_at.to_rfc3339_opts(SecondsFormat::Secs,true),
        "updatedAt":created_at.to_rfc3339_opts(SecondsFormat::Secs,true),
        "nextRunAt":next_time(created_at,interval)?,
        "lastRunAt":Value::Null,
        "totalRuns":0,
        "missedRuns":0,
        "reports":[]
    });
    validate_task(c, &task)?;
    c.execute(
        "INSERT INTO m_automation_tasks(id,brand_id,dedup,data)VALUES(?1,?2,?3,?4)",
        params![task["id"].as_str(), brand_id, request_id, task.to_string()],
    )
    .map_err(db_error)?;
    let mut task = task;
    task["alreadyExists"] = json!(false);
    Ok(task)
}

pub(super) fn list(c: &Connection, brand_id: &str, payload: &Value) -> Result<Value> {
    request(c, brand_id, payload, &["brandId"])?;
    let mut statement = c
        .prepare("SELECT id FROM m_automation_tasks WHERE brand_id=?1 ORDER BY rowid")
        .map_err(db_error)?;
    let rows = statement
        .query_map([brand_id], |row| row.get::<_, String>(0))
        .map_err(db_error)?;
    let mut tasks = Vec::new();
    for row in rows {
        if tasks.len() >= MAX_TASKS {
            return Err("每个品牌最多保存50个巡检任务，请检查备份".into());
        }
        tasks.push(read_task(c, brand_id, &row.map_err(db_error)?)?);
    }
    tasks.sort_by(|left, right| {
        right["createdAt"]
            .as_str()
            .cmp(&left["createdAt"].as_str())
            .then_with(|| right["id"].as_str().cmp(&left["id"].as_str()))
    });
    Ok(json!({"tasks":tasks}))
}

pub(super) fn pause(c: &Connection, brand_id: &str, payload: &Value) -> Result<Value> {
    let mut task = mutation(c, brand_id, payload)?;
    match task["status"].as_str() {
        Some("paused") => return Ok(task),
        Some("active") => {}
        _ => return Err("已取消任务不能暂停".into()),
    }
    task["status"] = json!("paused");
    changed(&mut task)?;
    write_task(c, &task)?;
    Ok(task)
}

pub(super) fn resume(c: &Connection, brand_id: &str, payload: &Value) -> Result<Value> {
    let mut task = mutation(c, brand_id, payload)?;
    match task["status"].as_str() {
        Some("active") => return Ok(task),
        Some("paused") => {}
        _ => return Err("已取消任务不能恢复".into()),
    }
    let at = parse_time(&now())?;
    let next = canonical_time(&task, "nextRunAt")?;
    task["status"] = json!("active");
    if next <= at {
        task["nextRunAt"] = json!(next_time(at, number(&task, "intervalMinutes")?)?);
    }
    changed(&mut task)?;
    write_task(c, &task)?;
    Ok(task)
}

pub(super) fn cancel(c: &Connection, brand_id: &str, payload: &Value) -> Result<Value> {
    let mut task = mutation(c, brand_id, payload)?;
    if task["status"] == "cancelled" {
        return Ok(task);
    }
    task["status"] = json!("cancelled");
    changed(&mut task)?;
    write_task(c, &task)?;
    Ok(task)
}

fn reminder(kind: &str, title: String, detail: String, refs: Value) -> Value {
    json!({"kind":kind,"title":title,"detail":detail,"due":true,"refs":refs})
}

fn fact_expiry_scan(c: &Connection, brand_id: &str, at: DateTime<Utc>) -> Result<Vec<Value>> {
    let mut statement = c
        .prepare("SELECT id,data FROM m_products WHERE brand_id=?1 ORDER BY id")
        .map_err(db_error)?;
    let rows = statement
        .query_map([brand_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(db_error)?;
    let today = at.date_naive();
    let soon = today + chrono::Duration::days(7);
    let mut reminders = Vec::new();
    for row in rows {
        let (product_id, data) = row.map_err(db_error)?;
        let product = parse_data(data)?;
        for fact in product["facts"].as_array().ok_or(INVALID)? {
            if !fact_is_approved(fact) {
                continue;
            }
            let Some(date) = fact["validUntil"].as_str() else {
                continue;
            };
            let expiry = NaiveDate::parse_from_str(date, "%Y-%m-%d").map_err(|_| INVALID)?;
            if expiry > soon {
                continue;
            }
            let expired = expiry <= today;
            reminders.push(reminder(
                if expired { "fact_expired" } else { "fact_expiring" },
                if expired { "事实已到期，请复核" } else { "事实将在7天内到期" }.into(),
                format!("商品“{}”的事实“{}”有效期为{}；仅提醒人工复核，不自动修改资料。", product["name"].as_str().unwrap_or("未命名商品"), fact["label"].as_str().unwrap_or("未命名事实"), date),
                json!({"productId":product_id,"factId":fact["id"],"validUntil":date,"expired":expired}),
            ));
        }
    }
    Ok(reminders)
}

fn publication_readback_due(
    c: &Connection,
    brand_id: &str,
    at: DateTime<Utc>,
) -> Result<Vec<Value>> {
    let mut statement = c
        .prepare("SELECT id,data FROM m_publications WHERE brand_id=?1 ORDER BY id")
        .map_err(db_error)?;
    let rows = statement
        .query_map([brand_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(db_error)?;
    let mut reminders = Vec::new();
    for row in rows {
        let (publication_id, data) = row.map_err(db_error)?;
        let publication = parse_data(data)?;
        let published = canonical_time(&publication, "publishedAt")?;
        if at.signed_duration_since(published) < chrono::Duration::hours(24)
            || publication["url"].as_str().is_none_or(str::is_empty)
        {
            continue;
        }
        reminders.push(reminder(
            "publication_readback_due",
            "发布记录已到人工回读时间".into(),
            "已标记为待回读；本次没有请求 URL，也没有改写发布记录。请由主 Agent 或界面显式执行真实回读。".into(),
            json!({"publicationId":publication_id,"draftId":publication["draftId"],"channel":publication["channel"],"publishedAt":publication["publishedAt"],"urlPresent":true}),
        ));
    }
    Ok(reminders)
}

fn visibility_retest_reminder(c: &Connection, brand_id: &str) -> Result<Vec<Value>> {
    let mut statement = c
        .prepare("SELECT id,data FROM m_campaigns WHERE brand_id=?1 ORDER BY id")
        .map_err(db_error)?;
    let rows = statement
        .query_map([brand_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(db_error)?;
    let mut campaigns = Vec::new();
    for row in rows {
        campaigns.push(parse_data(row.map_err(db_error)?.1)?);
    }
    let mut observations = c
        .prepare("SELECT data FROM m_observations WHERE brand_id=?1")
        .map_err(db_error)?;
    let rows = observations
        .query_map([brand_id], |row| row.get::<_, String>(0))
        .map_err(db_error)?;
    let mut slots = BTreeMap::<String, BTreeSet<String>>::new();
    for row in rows {
        let observation = parse_data(row.map_err(db_error)?)?;
        let Some(campaign_id) = observation["assessment"]["campaignId"].as_str() else {
            continue;
        };
        let slot = json!([
            observation["questionId"],
            observation["platform"],
            observation["phase"],
            observation["assessment"]["repetition"]
        ])
        .to_string();
        slots.entry(campaign_id.into()).or_default().insert(slot);
    }
    let mut reminders = Vec::new();
    for campaign in campaigns {
        let campaign_id = campaign["id"].as_str().ok_or(INVALID)?;
        let expected = campaign["questions"].as_array().ok_or(INVALID)?.len()
            * campaign["platforms"].as_array().ok_or(INVALID)?.len()
            * number(&campaign, "repetitions")? as usize
            * 2;
        let recorded = slots
            .get(campaign_id)
            .map_or(0, BTreeSet::len)
            .min(expected);
        if recorded >= expected {
            continue;
        }
        let mode = experiments::evidence_mode(&campaign)?;
        reminders.push(reminder(
            "visibility_retest_due",
            "固定测量仍有缺失样本".into(),
            format!("计划“{}”已有{recorded}/{expected}个唯一槽位样本（{mode}）。请人工补录或复测；本提醒不生成观测，也不推断效果提升。", campaign["name"].as_str().unwrap_or("未命名计划")),
            json!({"campaignId":campaign_id,"evidenceMode":mode,"expectedSamples":expected as i64,"recordedSamples":recorded as i64,"missingSamples":(expected-recorded) as i64}),
        ));
    }
    Ok(reminders)
}

fn inspect_node(
    c: &Connection,
    brand_id: &str,
    kind: &str,
    at: DateTime<Utc>,
) -> Result<Vec<Value>> {
    match kind {
        "fact_expiry_scan" => fact_expiry_scan(c, brand_id, at),
        "publication_readback_due" => publication_readback_due(c, brand_id, at),
        "visibility_retest_reminder" => visibility_retest_reminder(c, brand_id),
        _ => Err("禁止执行不在只读白名单中的节点".into()),
    }
}

fn inspect(c: &Connection, task: &Value, at: DateTime<Utc>) -> Result<Value> {
    let brand_id = task["brandId"].as_str().ok_or(INVALID)?;
    let nodes = task["nodes"].as_array().ok_or(INVALID)?;
    let order = validate_dag(&task["nodes"])?;
    let mut results = Vec::with_capacity(nodes.len());
    for index in order {
        let node = &nodes[index];
        let kind = node["kind"].as_str().ok_or(INVALID)?;
        let reminders = inspect_node(c, brand_id, kind, at)?;
        let omitted = reminders.len().saturating_sub(MAX_REMINDERS_PER_NODE);
        results.push(json!({
            "nodeId":node["id"],
            "kind":kind,
            "reminders":reminders.into_iter().take(MAX_REMINDERS_PER_NODE).collect::<Vec<_>>(),
            "omitted":omitted
        }));
    }
    Ok(json!({
        "id":id(),
        "ranAt":at.to_rfc3339_opts(SecondsFormat::Secs,true),
        "nodes":results,
        "modelCalls":0,
        "modelFeeMinor":0,
        "networkRequests":0,
        "publicationWrites":0
    }))
}

pub(super) fn run_due(c: &Connection, brand_id: &str, payload: &Value) -> Result<Value> {
    let allowed = if payload.get("taskId").is_some() {
        &["brandId", "taskId"][..]
    } else {
        &["brandId"][..]
    };
    request(c, brand_id, payload, allowed)?;
    run_due_at(c, brand_id, payload["taskId"].as_str(), &now())
}

/// Deterministic entry point for tests and an explicit application scheduler.
/// At most one report per due task is produced; missed intervals are never replayed.
pub(super) fn run_due_at(
    c: &Connection,
    brand_id: &str,
    task_id: Option<&str>,
    at: &str,
) -> Result<Value> {
    let at = parse_time(at)?;
    brand(c, brand_id)?;
    let ids = if let Some(task_id) = task_id {
        vec![guid(&json!({"taskId":task_id}), "taskId")?]
    } else {
        let mut statement = c
            .prepare("SELECT id FROM m_automation_tasks WHERE brand_id=?1 ORDER BY rowid")
            .map_err(db_error)?;
        let rows = statement
            .query_map([brand_id], |row| row.get::<_, String>(0))
            .map_err(db_error)?;
        let mut ids = Vec::new();
        for row in rows {
            ids.push(row.map_err(db_error)?);
        }
        ids
    };
    let mut tasks = Vec::new();
    let mut reports = Vec::new();
    let mut skipped = 0usize;
    for task_id in ids {
        let mut task = read_task(c, brand_id, &task_id)?;
        let next = canonical_time(&task, "nextRunAt")?;
        if task["status"] != "active" || next > at {
            skipped += 1;
            continue;
        }
        let interval = number(&task, "intervalMinutes")?;
        let missed = at.signed_duration_since(next).num_seconds().max(0)
            / chrono::Duration::minutes(interval).num_seconds();
        let report = inspect(c, &task, at)?;
        task["lastRunAt"] = report["ranAt"].clone();
        task["nextRunAt"] = json!(next_time(at, interval)?);
        task["totalRuns"] = json!(task["totalRuns"].as_u64().ok_or(INVALID)? + 1);
        task["missedRuns"] = json!(task["missedRuns"].as_u64().ok_or(INVALID)? + missed as u64);
        task["reports"]
            .as_array_mut()
            .ok_or(INVALID)?
            .push(report.clone());
        let report_count = task["reports"].as_array().ok_or(INVALID)?.len();
        if report_count > MAX_REPORTS {
            task["reports"]
                .as_array_mut()
                .ok_or(INVALID)?
                .drain(0..report_count - MAX_REPORTS);
        }
        changed_at(
            &mut task,
            report["ranAt"].as_str().ok_or(INVALID)?.to_owned(),
        )?;
        write_task(c, &task)?;
        tasks.push(task);
        reports.push(report);
    }
    Ok(json!({
        "tasks":tasks,
        "reports":reports,
        "skipped":skipped,
        "modelCalls":0,
        "modelFeeMinor":0,
        "networkRequests":0,
        "publicationWrites":0
    }))
}

pub(super) fn validate_database(c: &Connection) -> Result<()> {
    let mut statement = c
        .prepare("SELECT id,brand_id FROM m_automation_tasks")
        .map_err(db_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(db_error)?;
    let mut counts = BTreeMap::<String, usize>::new();
    for row in rows {
        let (task_id, brand_id) = row.map_err(db_error)?;
        read_task(c, &brand_id, &task_id)?;
        let count = counts.entry(brand_id).or_default();
        *count += 1;
        if *count > MAX_TASKS {
            return Err("每个品牌最多保存50个巡检任务".into());
        }
    }
    Ok(())
}
