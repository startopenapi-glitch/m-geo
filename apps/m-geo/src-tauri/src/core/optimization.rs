//! Durable, evidence-linked local optimization plans. The caller owns the transaction.
use super::*;
const MAX_PLAN_BYTES: usize = 1_500_000;
const MAX_CHECKPOINTS: usize = 8;
const MAX_ACTIONS: usize = 1000;
pub(super) const TABLE_SQL: &str = "CREATE TABLE IF NOT EXISTS m_optimization_plans(id TEXT PRIMARY KEY,brand_id TEXT NOT NULL,request_id TEXT NOT NULL,data TEXT NOT NULL,UNIQUE(brand_id,request_id),FOREIGN KEY(brand_id) REFERENCES m_brands(id));";

fn exact(v: &Value, fields: &[&str]) -> Result<()> {
    imports::keys(v, fields)?;
    if v.as_object().ok_or("优化方案必须为对象")?.len() != fields.len() {
        return Err("优化方案字段缺失，请检查备份".into());
    }
    Ok(())
}
fn canonical_id(v: &Value, k: &str) -> Result<String> {
    Ok(Uuid::parse_str(&guid(v, k)?)
        .map_err(|_| "对象标识错误")?
        .to_string())
}
fn hash_field(v: &Value, k: &str) -> Result<String> {
    let s = text(v, k, 64, true)?;
    if s.len() != 64
        || !s
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(format!("{k} 摘要无效，请重新诊断"));
    }
    Ok(s)
}
fn canonical_time(v: &Value, k: &str) -> Result<String> {
    let t = date_time(v, k)?;
    if v[k] != t {
        return Err("优化方案时间格式不规范".into());
    }
    Ok(t)
}
fn create_hash(name: &str, campaign_id: &str, diagnosis_hash: &str) -> String {
    fingerprint(
        &json!({"name":name,"campaignId":campaign_id,"diagnosisFingerprint":diagnosis_hash}),
    )
}
fn action(f: &Value, at: &str) -> Value {
    let mut a = f.clone();
    a["status"] = json!("open");
    a["note"] = json!("");
    a["updatedAt"] = json!(at);
    a
}
fn first_findings(v: &Value) -> Result<Vec<(Value, String)>> {
    let mut seen = HashSet::new();
    let mut out = vec![];
    for cp in v["checkpoints"].as_array().ok_or("优化方案检查点损坏")? {
        for f in cp["diagnosis"]["findings"]
            .as_array()
            .ok_or("优化方案诊断损坏")?
        {
            let key = f["key"].as_str().ok_or("优化行动缺少标识")?;
            if seen.insert(key.to_owned()) {
                out.push((
                    f.clone(),
                    cp["capturedAt"]
                        .as_str()
                        .ok_or("检查点时间错误")?
                        .to_owned(),
                ));
            }
        }
    }
    Ok(out)
}
fn validate_plan(c: &Connection, v: &Value) -> Result<()> {
    if v.to_string().len() > MAX_PLAN_BYTES {
        return Err("优化方案超过1.5MB，请减少检查点或精简备注".into());
    }
    exact(
        v,
        &[
            "format",
            "id",
            "brandId",
            "requestId",
            "createFingerprint",
            "name",
            "campaignId",
            "evidenceMode",
            "revision",
            "createdAt",
            "updatedAt",
            "checkpoints",
            "actions",
        ],
    )?;
    if v["format"] != "m-geo-optimization-plan-v1" {
        return Err("优化方案格式不支持".into());
    }
    for key in ["id", "brandId", "requestId", "campaignId"] {
        if v[key] != canonical_id(v, key)? {
            return Err("优化方案标识不规范".into());
        }
    }
    let b = v["brandId"].as_str().ok_or("品牌缺失")?;
    brand(c, b)?;
    let name = text(v, "name", 100, true)?;
    if v["name"] != name {
        return Err("优化方案名称不规范".into());
    }
    let mode = experiments::evidence_mode(v)?;
    let rev = number(v, "revision")?;
    let created = canonical_time(v, "createdAt")?;
    let updated = canonical_time(v, "updatedAt")?;
    if updated < created {
        return Err("优化方案更新时间早于创建时间".into());
    }
    let cps = v["checkpoints"].as_array().ok_or("检查点列表缺失")?;
    if cps.is_empty() || cps.len() > MAX_CHECKPOINTS || rev < cps.len() as i64 {
        return Err("优化方案最多8个检查点且版本不得少于检查点数".into());
    }
    let stored_campaign = load(
        c,
        "m_campaigns",
        b,
        v["campaignId"].as_str().ok_or("测试计划缺失")?,
    )?;
    let mut ids = HashSet::new();
    let mut last = created.clone();
    let mut last_hash = None;
    for (i, cp) in cps.iter().enumerate() {
        exact(cp, &["id", "capturedAt", "diagnosis"])?;
        let cid = canonical_id(cp, "id")?;
        if cp["id"] != cid || !ids.insert(cid) {
            return Err("检查点标识重复或无效".into());
        }
        let at = canonical_time(cp, "capturedAt")?;
        if at < last || at > updated || (i == 0 && at != created) {
            return Err("检查点时间顺序错误".into());
        }
        last = at;
        let d = &cp["diagnosis"];
        optimization_diagnosis::validate_snapshot(d)?;
        if d["brandId"] != b
            || d["campaign"]["id"] != v["campaignId"]
            || d["campaign"] != stored_campaign
            || d["evidenceMode"] != mode
            || d["phase"] != if i == 0 { "baseline" } else { "followup" }
        {
            return Err("历史诊断的品牌、测试计划、样本性质或阶段不一致".into());
        }
        if last_hash.as_ref() == Some(&d["fingerprint"]) {
            return Err("相邻检查点诊断没有变化".into());
        }
        last_hash = Some(d["fingerprint"].clone());
        for prod in d["products"].as_array().ok_or("诊断商品缺失")? {
            load(
                c,
                "m_products",
                b,
                prod["id"].as_str().ok_or("诊断商品标识缺失")?,
            )?;
        }
        for q in d["campaign"]["questions"]
            .as_array()
            .ok_or("固定问题缺失")?
        {
            let current = load(c, "m_questions", b, q["id"].as_str().ok_or("问题标识缺失")?)?;
            if current["productId"] != q["productId"] {
                return Err("历史问题归属商品不一致".into());
            }
        }
    }
    let original_hash = create_hash(
        &name,
        v["campaignId"].as_str().ok_or("测试计划缺失")?,
        cps[0]["diagnosis"]["fingerprint"]
            .as_str()
            .ok_or("初始诊断摘要缺失")?,
    );
    if hash_field(v, "createFingerprint")? != original_hash
        || v["createFingerprint"] != original_hash
    {
        return Err("优化方案创建摘要不一致".into());
    }
    let actions = v["actions"].as_array().ok_or("行动列表缺失")?;
    let first = first_findings(v)?;
    if actions.len() > MAX_ACTIONS || first.len() > MAX_ACTIONS || actions.len() != first.len() {
        return Err("优化行动须匹配历史诊断且最多1000项".into());
    }
    for (a, (finding, first_at)) in actions.iter().zip(first) {
        exact(
            a,
            &[
                "key",
                "code",
                "productId",
                "title",
                "detail",
                "section",
                "priority",
                "evidenceIds",
                "status",
                "note",
                "updatedAt",
            ],
        )?;
        for field in [
            "key",
            "code",
            "productId",
            "title",
            "detail",
            "section",
            "priority",
            "evidenceIds",
        ] {
            if a[field] != finding[field] {
                return Err("优化行动必须保留首次发现的说明和证据".into());
            }
        }
        let status = choice(a, "status", &["open", "in_progress", "done", "dismissed"])?;
        if a["status"] != status {
            return Err("优化行动状态不规范".into());
        }
        let note = raw(a, "note", 1000, false)?;
        if a["note"] != note {
            return Err("优化行动备注字段缺失".into());
        }
        if ["done", "dismissed"].contains(&status.as_str()) && note.trim().is_empty() {
            return Err("完成或忽略行动时请填写说明".into());
        }
        let at = canonical_time(a, "updatedAt")?;
        if at < first_at || at > updated {
            return Err("优化行动更新时间错误".into());
        }
    }
    Ok(())
}
fn read_plan(c: &Connection, b: &str, pid: &str) -> Result<Value> {
    let row = c
        .query_row(
            "SELECT request_id,data FROM m_optimization_plans WHERE id=?1 AND brand_id=?2",
            params![pid, b],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(db_error)?
        .ok_or("记录不存在或不属于当前品牌")?;
    if row.1.len() > MAX_PLAN_BYTES {
        return Err("优化方案超过1.5MB，请检查备份".into());
    }
    let v = parse_data(row.1)?;
    if v["id"] != pid || v["brandId"] != b || v["requestId"] != row.0 {
        return Err("优化方案存储身份不一致".into());
    }
    validate_plan(c, &v)?;
    Ok(v)
}
fn save(c: &Connection, v: &Value) -> Result<()> {
    validate_plan(c, v)?;
    super::update(c, "m_optimization_plans", v)
}
fn expected(v: &Value, p: &Value) -> Result<()> {
    if number(v, "revision")? != number(p, "expectedRevision")? {
        return Err("方案已被其他操作修改，请重新加载方案后再操作".into());
    }
    Ok(())
}
fn changed(v: &mut Value) -> Result<String> {
    v["revision"] = json!(number(v, "revision")?
        .checked_add(1)
        .ok_or("方案版本超出范围")?);
    let at = now();
    v["updatedAt"] = json!(at);
    Ok(at)
}
pub(super) fn create(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    let name = text(p, "name", 100, true)?;
    let campaign_id = canonical_id(p, "campaignId")?;
    let diagnosis_hash = hash_field(p, "diagnosisFingerprint")?;
    let request_id = canonical_id(p, "requestId")?;
    let input_hash = create_hash(&name, &campaign_id, &diagnosis_hash);
    let prior = c
        .query_row(
            "SELECT id FROM m_optimization_plans WHERE brand_id=?1 AND request_id=?2",
            params![b, request_id],
            |r| r.get::<_, String>(0),
        )
        .optional()
        .map_err(db_error)?;
    // Reconcile the original request before capacity checks or mutable live diagnosis work.
    if let Some(pid) = prior {
        let mut v = read_plan(c, b, &pid)?;
        if v["createFingerprint"] != input_hash {
            return Err("此请求标识已用于不同内容，请重新加载原方案".into());
        }
        v["alreadyExists"] = json!(true);
        return Ok(v);
    }
    let count: i64 = c
        .query_row(
            "SELECT count(*) FROM m_optimization_plans WHERE brand_id=?1",
            [b],
            |r| r.get(0),
        )
        .map_err(db_error)?;
    if count >= 50 {
        return Err("每个品牌最多50个优化方案，请使用已有方案".into());
    }
    let d = optimization_diagnosis::prepare(
        c,
        b,
        &json!({"campaignId":campaign_id,"phase":"baseline"}),
    )?;
    if d["fingerprint"] != diagnosis_hash {
        return Err("资料或样本已变化，请重新诊断后保存方案".into());
    }
    let at = now();
    let actions = d["findings"]
        .as_array()
        .ok_or("诊断结果缺少行动")?
        .iter()
        .map(|f| action(f, &at))
        .collect::<Vec<_>>();
    let mut v = json!({"format":"m-geo-optimization-plan-v1","id":id(),"brandId":b,"requestId":request_id,"createFingerprint":input_hash,"name":name,"campaignId":campaign_id,"evidenceMode":d["evidenceMode"],"revision":1,"createdAt":at,"updatedAt":at,"checkpoints":[{"id":id(),"capturedAt":at,"diagnosis":d}],"actions":actions});
    validate_plan(c, &v)?;
    c.execute(
        "INSERT INTO m_optimization_plans(id,brand_id,request_id,data)VALUES(?1,?2,?3,?4)",
        params![v["id"].as_str(), b, v["requestId"].as_str(), v.to_string()],
    )
    .map_err(db_error)?;
    v["alreadyExists"] = json!(false);
    Ok(v)
}
pub(super) fn list(c: &Connection, b: &str, _: &Value) -> Result<Value> {
    let mut st = c
        .prepare("SELECT id FROM m_optimization_plans WHERE brand_id=?1 ORDER BY rowid")
        .map_err(db_error)?;
    let rows = st
        .query_map([b], |r| r.get::<_, String>(0))
        .map_err(db_error)?;
    let mut out = vec![];
    for row in rows {
        if out.len() >= 50 {
            return Err("每个品牌最多50个优化方案，请检查备份".into());
        }
        let v = read_plan(c, b, &row.map_err(db_error)?)?;
        out.push(json!({"id":v["id"],"requestId":v["requestId"],"name":v["name"],"campaignId":v["campaignId"],"evidenceMode":v["evidenceMode"],"revision":v["revision"],"createdAt":v["createdAt"],"updatedAt":v["updatedAt"],"checkpointCount":v["checkpoints"].as_array().ok_or("检查点损坏")?.len(),"actionCount":v["actions"].as_array().ok_or("行动列表损坏")?.len(),"doneCount":v["actions"].as_array().ok_or("行动列表损坏")?.iter().filter(|a|a["status"]=="done").count()}));
    }
    Ok(json!(out))
}
pub(super) fn get(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    read_plan(c, b, &canonical_id(p, "planId")?)
}
pub(super) fn update(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    let mut v = get(c, b, p)?;
    expected(&v, p)?;
    let key = text(p, "actionKey", 200, true)?;
    let status = choice(p, "status", &["open", "in_progress", "done", "dismissed"])?;
    let note = raw(p, "note", 1000, false)?;
    if ["done", "dismissed"].contains(&status.as_str()) && note.trim().is_empty() {
        return Err("完成或忽略行动时请填写说明（不代表已验证改善）".into());
    }
    let at = changed(&mut v)?;
    let a = v["actions"]
        .as_array_mut()
        .ok_or("行动列表损坏")?
        .iter_mut()
        .find(|a| a["key"] == key)
        .ok_or("行动不存在，请重新加载方案")?;
    a["status"] = json!(status);
    a["note"] = json!(note);
    a["updatedAt"] = json!(at);
    save(c, &v)?;
    Ok(v)
}
pub(super) fn check(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    let mut v = get(c, b, p)?;
    expected(&v, p)?;
    if v["checkpoints"].as_array().ok_or("检查点损坏")?.len() >= MAX_CHECKPOINTS {
        return Err("每个方案最多8个检查点，请新建方案继续记录".into());
    }
    let d = optimization_diagnosis::prepare(
        c,
        b,
        &json!({"campaignId":v["campaignId"],"phase":"followup"}),
    )?;
    if v["checkpoints"]
        .as_array()
        .and_then(|a| a.last())
        .ok_or("检查点缺失")?["diagnosis"]["fingerprint"]
        == d["fingerprint"]
    {
        return Err("诊断没有变化，未增加检查点；请记录新样本或修改资料后再复测".into());
    }
    let at = changed(&mut v)?;
    let existing = v["actions"]
        .as_array()
        .ok_or("行动列表损坏")?
        .iter()
        .map(|a| a["key"].as_str().unwrap_or("").to_owned())
        .collect::<HashSet<_>>();
    for finding in d["findings"].as_array().ok_or("诊断行动缺失")? {
        if !existing.contains(finding["key"].as_str().ok_or("行动标识缺失")?) {
            v["actions"]
                .as_array_mut()
                .ok_or("行动列表损坏")?
                .push(action(finding, &at));
        }
    }
    v["checkpoints"]
        .as_array_mut()
        .ok_or("检查点损坏")?
        .push(json!({"id":id(),"capturedAt":at,"diagnosis":d}));
    save(c, &v)?;
    Ok(v)
}
fn csv_field(s: &str) -> String {
    let guard = s
        .chars()
        .next()
        .is_some_and(|ch| ch.is_whitespace() || ch.is_control() || "=+-@".contains(ch));
    format!(
        "\"{}{}\"",
        if guard { "'" } else { "" },
        s.replace('"', "\"\"")
    )
}
fn md(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\\', "\\\\")
        .replace('|', "\\|")
        .replace('[', "\\[")
        .replace(']', "\\]")
        .replace('*', "\\*")
        .replace('_', "\\_")
        .replace('#', "\\#")
        .replace('`', "\\`")
        .replace('!', "\\!")
        .replace('\r', "")
        .replace('\n', "<br>")
}
fn evidence_label(mode: &str) -> &'static str {
    match mode {
        "recorded" => "人工实测记录",
        "simulated" => "模拟数据（非真实平台表现）",
        _ => "历史未分类（不得视为真实实测）",
    }
}
fn frozen_target<'a>(cps: &'a [Value], finding: &Value) -> Option<&'a Value> {
    cps.iter().find(|cp| {
        cp["diagnosis"]["findings"]
            .as_array()
            .is_some_and(|fs| fs.iter().any(|f| f["key"] == finding["key"]))
    })?["diagnosis"]["products"]
        .as_array()?
        .iter()
        .find(|p| p["id"] == finding["productId"])
}
fn target_md(product: Option<&Value>, product_id: &Value) -> String {
    match product {
        Some(p) => format!(
            "商品：{}；SKU：{}；ID：{}",
            md(p["name"].as_str().unwrap_or("")),
            md(p["sku"].as_str().unwrap_or("")),
            md(p["id"].as_str().unwrap_or(""))
        ),
        None => format!(
            "固定测量目标；ID：{}",
            md(product_id.as_str().unwrap_or("非单一商品"))
        ),
    }
}
pub(super) fn prepare_export(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    let v = get(c, b, p)?;
    let cps = v["checkpoints"].as_array().ok_or("检查点缺失")?;
    let actions = v["actions"].as_array().ok_or("行动缺失")?;
    let mode = v["evidenceMode"].as_str().ok_or("样本性质缺失")?;
    let label = evidence_label(mode);
    let latest = &cps.last().ok_or("检查点缺失")?["diagnosis"];
    let detected = latest["findings"]
        .as_array()
        .ok_or("诊断发现缺失")?
        .iter()
        .map(|f| f["key"].as_str().unwrap_or(""))
        .collect::<HashSet<_>>();
    let mut report=format!("# 优化方案：{}\n\n样本性质：{}。\n\n方案版本：{}；检查点：{}；行动：{}。\n\n人工状态为自述：done 表示已处理、等待验证，dismissed 表示人工忽略；均不代表 GEO 成功。当前检测与人工状态独立展示。样本差异不证明因果关系，也不承诺收录、排名或推荐。备注为用户输入，未经系统事实核验。\n\n## 冻结检查点\n",md(v["name"].as_str().ok_or("名称缺失")?),label,v["revision"],cps.len(),actions.len());
    for (i, cp) in cps.iter().enumerate() {
        let d = &cp["diagnosis"];
        let m = &d["metrics"];
        report.push_str(&format!("\n### 检查点 {}（{}）\n\n样本性质：{}；截取时间：{}；诊断日期：{}。\n\n每阶段计划样本：{}。\n\n| 阶段 | 样本 | 推荐已评估 | 推荐是 | 推荐未知 | 有顺位 | 前三 | 有引用 | 不准确 |\n| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n",i+1,d["phase"].as_str().unwrap_or(""),label,cp["capturedAt"].as_str().unwrap_or(""),d["asOf"].as_str().unwrap_or(""),m["expectedPerPhase"]));
        for phase in ["baseline", "followup"] {
            let n = &m[phase];
            report.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
                phase,
                n["samples"],
                n["assessed"],
                n["recommended"],
                n["unknown"],
                n["ranked"],
                n["top3"],
                n["cited"],
                n["incorrect"]
            ));
        }
        report.push_str(&format!("\n匹配样本对：{}；双方推荐已评估的匹配分母：{}；匹配基线推荐是：{}；匹配复测推荐是：{}；差异百分点：{}。未知不按否计，缺失顺位不按前三以外计。每个快照包含截取时已有的两个阶段，创建时间不能证明测量时间。\n",m["matchedPairs"],m["matchedAssessedPairs"],m["matchedBaselineRecommended"],m["matchedFollowupRecommended"],if m["deltaPercentagePoints"].is_null(){"不可计算".to_owned()}else{m["deltaPercentagePoints"].to_string()}));
    }
    report.push_str("\n## 当前检测\n\n");
    for f in latest["findings"].as_array().ok_or("诊断发现缺失")? {
        report.push_str(&format!(
            "- {}：{}；首次冻结目标：{}\n",
            md(f["title"].as_str().unwrap_or("")),
            md(f["detail"].as_str().unwrap_or("")),
            target_md(frozen_target(cps, f), &f["productId"])
        ));
    }
    if detected.is_empty() {
        report.push_str("当前规则未检测到问题；不代表平台表现已验证成功。\n");
    }
    report.push_str("\n## 人工行动状态（用户自述）\n\n");
    let mut csv="key,code,productId,productName,productSku,title,detail,section,priority,evidenceIds,status,note,updatedAt,latestDetected\n".to_owned();
    for a in actions {
        let active = detected.contains(a["key"].as_str().unwrap_or(""));
        report.push_str(&format!("### {}\n\n人工状态：{}；当前检测：{}。\n\n首次说明：{}\n\n首次证据：{}\n\n用户输入备注：\n\n{}\n\n",md(a["title"].as_str().unwrap_or("")),a["status"].as_str().unwrap_or(""),if active{"仍检测到"}else{"本次未检测到"},md(a["detail"].as_str().unwrap_or("")),md(&a["evidenceIds"].to_string()),md(a["note"].as_str().unwrap_or(""))));
        let target = frozen_target(cps, a);
        report.push_str(&format!(
            "首次冻结目标：{}\n\n",
            target_md(target, &a["productId"])
        ));
        let current = latest["products"]
            .as_array()
            .and_then(|ps| ps.iter().find(|p| p["id"] == a["productId"]));
        if target_md(current, &a["productId"]) != target_md(target, &a["productId"]) {
            report.push_str(&format!(
                "最新检查点目标：{}\n\n",
                target_md(current, &a["productId"])
            ));
        }
        let fields = [
            a["key"].as_str().unwrap_or("").to_owned(),
            a["code"].as_str().unwrap_or("").to_owned(),
            a["productId"].as_str().unwrap_or("").to_owned(),
            target
                .and_then(|p| p["name"].as_str())
                .unwrap_or("")
                .to_owned(),
            target
                .and_then(|p| p["sku"].as_str())
                .unwrap_or("")
                .to_owned(),
            a["title"].as_str().unwrap_or("").to_owned(),
            a["detail"].as_str().unwrap_or("").to_owned(),
            a["section"].as_str().unwrap_or("").to_owned(),
            a["priority"].as_str().unwrap_or("").to_owned(),
            a["evidenceIds"].to_string(),
            a["status"].as_str().unwrap_or("").to_owned(),
            a["note"].as_str().unwrap_or("").to_owned(),
            a["updatedAt"].as_str().unwrap_or("").to_owned(),
            active.to_string(),
        ];
        csv.push_str(
            &fields
                .iter()
                .map(|s| csv_field(s))
                .collect::<Vec<_>>()
                .join(","),
        );
        csv.push('\n');
    }
    let plan_json = serde_json::to_string_pretty(&v).map_err(|_| "优化方案无法导出")?;
    if report.len() + plan_json.len() + csv.len() > 16 * 1024 * 1024 {
        return Err("导出内容超过16MiB，请减少方案内容".into());
    }
    Ok(
        json!({"summary":{"id":v["id"],"name":v["name"],"evidenceMode":v["evidenceMode"],"checkpointCount":cps.len(),"actionCount":actions.len(),"doneCount":actions.iter().filter(|a|a["status"]=="done").count()},"fileName":"optimization-plan.zip","files":[{"path":"report.md","content":report},{"path":"plan.json","content":plan_json},{"path":"actions.csv","content":csv}]}),
    )
}
pub(super) fn validate_database(c: &Connection) -> Result<()> {
    let mut st = c
        .prepare("SELECT id,brand_id,request_id,data FROM m_optimization_plans")
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
        let (pid, b, rid, data) = row.map_err(db_error)?;
        if data.len() > MAX_PLAN_BYTES {
            return Err("优化方案超过1.5MB，请检查备份".into());
        }
        let v = parse_data(data)?;
        if v["id"] != pid || v["brandId"] != b || v["requestId"] != rid {
            return Err("优化方案存储身份不一致".into());
        }
        let n = counts.entry(b).or_insert(0);
        *n += 1;
        if *n > 50 {
            return Err("每个品牌最多50个优化方案".into());
        }
        validate_plan(c, &v)?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn db() -> (Connection, String) {
        let mut c = Connection::open_in_memory().unwrap();
        initialize(&mut c).unwrap();
        c.execute_batch(TABLE_SQL).unwrap();
        let b = handle(&mut c, "brand.create", json!({"name":"虚构优化验收"})).unwrap();
        (c, b["id"].as_str().unwrap().to_owned())
    }
    fn fixture() -> (Connection, String, Value, Value) {
        let (mut c, b) = db();
        let product=handle(&mut c,"product.save",json!({"brandId":b,"name":"虚构杯","sku":"OPT-1","category":"测试","officialUrl":"","sourceText":"RAW_SECRET","privateNotes":"NOTE_SECRET","facts":[{"id":"secret","label":"内部","value":"PRIVATE_SECRET","source":"PRIVATE_SOURCE","visibility":"private","validUntil":null}]})).unwrap();
        let q=handle(&mut c,"question.save",json!({"brandId":b,"productId":product["id"],"text":"通勤应该怎样选择水杯？","intent":"selection","origin":"customer","source":"测试","branded":false,"confirmed":true})).unwrap();
        let campaign=handle(&mut c,"campaign.create",json!({"brandId":b,"name":"模拟优化组","questionIds":[q["id"]],"platforms":["doubao"],"modelLabel":"本地模拟","searchMode":"无请求","context":"模拟","repetitions":1,"evidenceMode":"simulated"})).unwrap();
        (c, b, product, campaign)
    }
    fn request(c: &Connection, b: &str, campaign: &Value) -> Value {
        let d = optimization_diagnosis::prepare(
            c,
            b,
            &json!({"campaignId":campaign["id"],"phase":"baseline"}),
        )
        .unwrap();
        json!({"name":"优化测试","campaignId":campaign["id"],"diagnosisFingerprint":d["fingerprint"],"requestId":id()})
    }
    fn create_plan(c: &Connection, b: &str, campaign: &Value) -> Value {
        create(c, b, &request(c, b, campaign)).unwrap()
    }
    fn store(c: &Connection, v: &Value) {
        c.execute(
            "UPDATE m_optimization_plans SET data=?1 WHERE id=?2",
            params![v.to_string(), v["id"].as_str()],
        )
        .unwrap();
    }
    fn observation(c: &mut Connection, b: &str, campaign: &Value) {
        let q = &campaign["questions"][0];
        handle(c,"observation.add",json!({"brandId":b,"productId":q["productId"],"questionId":q["id"],"platform":"doubao","surface":"consumer","phase":"followup","modelLabel":campaign["modelLabel"],"searchMode":campaign["searchMode"],"testedAt":"2026-10-23T00:00:00Z","answer":"ANSWER_SECRET","mentioned":true,"accuracy":"incorrect","citedUrl":"","evidenceNote":"模拟","assessment":{"campaignId":campaign["id"],"repetition":1,"context":campaign["context"],"recommendation":"no","rank":null,"competitors":"COMPETITOR_SECRET"}})).unwrap();
    }
    #[test]
    fn canonical_replay_precedes_current_diagnosis_and_capacity() {
        let (c, b, product, campaign) = fixture();
        let mut input = request(&c, &b, &campaign);
        input["name"] = json!("  优化测试  ");
        let plan = create(&c, &b, &input).unwrap();
        assert_eq!(plan["name"], "优化测试");
        assert_eq!(plan["alreadyExists"], false);
        assert!(get(&c, &b, &json!({"planId":plan["id"]}))
            .unwrap()
            .get("alreadyExists")
            .is_none());
        for _ in 1..50 {
            let mut other = input.clone();
            other["requestId"] = json!(id());
            create(&c, &b, &other).unwrap();
        }
        c.execute("UPDATE m_products SET data=json_set(data,'$.name','改名后','$.revision',2) WHERE id=?1",[product["id"].as_str().unwrap()]).unwrap();
        let replay = create(&c, &b, &input).unwrap();
        assert_eq!(replay["id"], plan["id"]);
        assert_eq!(replay["alreadyExists"], true);
        let mut changed = input.clone();
        changed["name"] = json!("不同名称");
        assert!(create(&c, &b, &changed).unwrap_err().contains("请求"));
        changed = input.clone();
        changed["requestId"] = json!(id());
        assert!(create(&c, &b, &changed).unwrap_err().contains("50"));
        let listed = list(&c, &b, &json!({})).unwrap();
        assert_eq!(listed.as_array().unwrap().len(), 50);
        assert_eq!(listed[0]["requestId"], input["requestId"]);
        validate_database(&c).unwrap();
    }
    #[test]
    fn stale_preview_and_cross_brand_are_rejected() {
        let (c, b, product, campaign) = fixture();
        let input = request(&c, &b, &campaign);
        c.execute(
            "UPDATE m_products SET data=json_set(data,'$.revision',2) WHERE id=?1",
            [product["id"].as_str().unwrap()],
        )
        .unwrap();
        assert!(create(&c, &b, &input).unwrap_err().contains("重新诊断"));
        assert_eq!(list(&c, &b, &json!({})).unwrap(), json!([]));
        let mut refreshed = request(&c, &b, &campaign);
        refreshed["requestId"] = input["requestId"].clone();
        let plan = create(&c, &b, &refreshed).unwrap();
        assert_eq!(plan["requestId"], input["requestId"]);
        assert_eq!(
            list(&c, &b, &json!({})).unwrap().as_array().unwrap().len(),
            1
        );
        assert!(create(&c, &b, &input).unwrap_err().contains("请求"));
        assert_eq!(create(&c, &b, &refreshed).unwrap()["id"], plan["id"]);
        let other = id();
        assert!(get(&c, &other, &json!({"planId":plan["id"]})).is_err());
        assert_eq!(list(&c, &other, &json!({})).unwrap(), json!([]));
        assert!(create(&c, &other, &request(&c, &b, &campaign)).is_err());
    }
    #[test]
    fn action_revision_note_guards_and_manual_reload_after_lost_reply() {
        let (c, b, _, campaign) = fixture();
        let plan = create_plan(&c, &b, &campaign);
        let key = plan["actions"][0]["key"].clone();
        let mut input = json!({"planId":plan["id"],"expectedRevision":1,"actionKey":key,"status":"done","note":""});
        assert!(update(&c, &b, &input).is_err());
        input["note"] = json!("人工作业，尚待复测");
        let updated = update(&c, &b, &input).unwrap();
        assert_eq!(updated["revision"], 2);
        assert_eq!(updated["actions"][0]["status"], "done");
        assert!(update(&c, &b, &input).unwrap_err().contains("重新加载"));
        let reloaded = get(&c, &b, &json!({"planId":plan["id"]})).unwrap();
        assert_eq!(updated, reloaded);
        input["expectedRevision"] = json!(2);
        input["note"] = json!("字".repeat(1001));
        assert!(update(&c, &b, &input).is_err());
        input["note"] = json!("说明");
        input["status"] = json!("dismissed");
        assert!(update(&c, &b, &input).is_ok());
    }
    #[test]
    fn recheck_appends_frozen_actions_without_resolving_and_rejects_unchanged() {
        let (mut c, b, product, campaign) = fixture();
        let plan = create_plan(&c, &b, &campaign);
        let done=update(&c,&b,&json!({"planId":plan["id"],"expectedRevision":1,"actionKey":plan["actions"][0]["key"],"status":"done","note":"人工处理"})).unwrap();
        observation(&mut c, &b, &campaign);
        let next = check(&c, &b, &json!({"planId":plan["id"],"expectedRevision":2})).unwrap();
        assert_eq!(next["checkpoints"].as_array().unwrap().len(), 2);
        assert_eq!(next["actions"][0], done["actions"][0]);
        assert!(next["actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["code"] == "answer_incorrect"));
        assert!(
            check(&c, &b, &json!({"planId":plan["id"],"expectedRevision":3}))
                .unwrap_err()
                .contains("没有变化")
        );
        for rev in 4..=9 {
            c.execute(
                "UPDATE m_products SET data=json_set(data,'$.revision',?1) WHERE id=?2",
                params![rev, product["id"].as_str()],
            )
            .unwrap();
            check(
                &c,
                &b,
                &json!({"planId":plan["id"],"expectedRevision":rev-1}),
            )
            .unwrap();
        }
        assert!(
            check(&c, &b, &json!({"planId":plan["id"],"expectedRevision":9}))
                .unwrap_err()
                .contains("8")
        );
        validate_database(&c).unwrap();
    }
    #[test]
    fn exports_guard_formulas_preserve_notes_and_exclude_private_sources() {
        let (mut c, b, _, campaign) = fixture();
        let plan = create_plan(&c, &b, &campaign);
        observation(&mut c, &b, &campaign);
        let plan = check(&c, &b, &json!({"planId":plan["id"],"expectedRevision":1})).unwrap();
        let note = " =SUM(1,2)\n\"人工说明\"";
        let plan=update(&c,&b,&json!({"planId":plan["id"],"expectedRevision":2,"actionKey":plan["actions"][0]["key"],"status":"done","note":note})).unwrap();
        let out = prepare_export(&c, &b, &json!({"planId":plan["id"]})).unwrap();
        let files = out["files"].as_array().unwrap();
        assert_eq!(files.len(), 3);
        let parsed: Value = serde_json::from_str(files[1]["content"].as_str().unwrap()).unwrap();
        assert_eq!(parsed, plan);
        let mut rdr = ::csv::Reader::from_reader(files[2]["content"].as_str().unwrap().as_bytes());
        let header = rdr.headers().unwrap().clone();
        let i = header.iter().position(|s| s == "note").unwrap();
        assert_eq!(
            rdr.records().next().unwrap().unwrap().get(i),
            Some(format!("'{note}").as_str())
        );
        let serialized = out.to_string();
        for secret in [
            "RAW_SECRET",
            "NOTE_SECRET",
            "PRIVATE_SECRET",
            "PRIVATE_SOURCE",
            "ANSWER_SECRET",
            "COMPETITOR_SECRET",
        ] {
            assert!(!serialized.contains(secret), "{secret}");
        }
        assert!(files[0]["content"].as_str().unwrap().contains("因果"));
        assert_eq!(
            out,
            prepare_export(&c, &b, &json!({"planId":plan["id"]})).unwrap()
        );
    }
    #[test]
    fn final_fix_exports_first_occurrence_targets_after_rename_with_safe_text() {
        let (mut c, b, product, campaign) = fixture();
        let second = handle(&mut c,"product.save",json!({"brandId":b,"name":"=第二杯\n[目标]","sku":"@SKU-B","category":"测试","officialUrl":"","sourceText":"SECOND_RAW_SECRET","privateNotes":"SECOND_NOTE_SECRET","facts":[]})).unwrap();
        let q=handle(&mut c,"question.save",json!({"brandId":b,"productId":second["id"],"text":"怎样选择另一水杯？","intent":"selection","origin":"customer","source":"测试","branded":false,"confirmed":true})).unwrap();
        let campaign=handle(&mut c,"campaign.create",json!({"brandId":b,"name":"双商品模拟","questionIds":[campaign["questions"][0]["id"],q["id"]],"platforms":["doubao"],"modelLabel":"本地模拟","searchMode":"无请求","context":"模拟","repetitions":1,"evidenceMode":"simulated"})).unwrap();
        let plan = create_plan(&c, &b, &campaign);
        c.execute("UPDATE m_products SET data=json_set(data,'$.name','最新杯名','$.sku','NEW-A','$.revision',2) WHERE id=?1",[product["id"].as_str().unwrap()]).unwrap();
        observation(&mut c, &b, &campaign);
        let plan = check(&c, &b, &json!({"planId":plan["id"],"expectedRevision":1})).unwrap();
        let plan=update(&c,&b,&json!({"planId":plan["id"],"expectedRevision":2,"actionKey":format!("facts_unconfirmed:{}",product["id"].as_str().unwrap()),"status":"done","note":"人工确认，等待验证"})).unwrap();
        let out = prepare_export(&c, &b, &json!({"planId":plan["id"]})).unwrap();
        let report = out["files"][0]["content"].as_str().unwrap();
        assert!(report.contains("首次冻结目标：商品：虚构杯；SKU：OPT-1"));
        assert!(report.contains("首次冻结目标：商品：=第二杯<br>\\[目标\\]；SKU：@SKU-B"));
        assert!(report.contains("最新检查点目标：商品：最新杯名；SKU：NEW-A"));
        assert!(report.contains("人工状态：done；当前检测：仍检测到"));
        let csv = out["files"][2]["content"].as_str().unwrap();
        let mut reader = ::csv::Reader::from_reader(csv.as_bytes());
        let headers = reader.headers().unwrap().clone();
        let at = |name: &str| headers.iter().position(|v| v == name).unwrap();
        let rows = reader.records().map(|v| v.unwrap()).collect::<Vec<_>>();
        let first = rows
            .iter()
            .find(|r| {
                r.get(at("key"))
                    == Some(
                        format!("facts_unconfirmed:{}", product["id"].as_str().unwrap()).as_str(),
                    )
            })
            .unwrap();
        assert_eq!(first.get(at("productName")), Some("虚构杯"));
        assert_eq!(first.get(at("productSku")), Some("OPT-1"));
        assert_eq!(first.get(at("status")), Some("done"));
        assert_eq!(first.get(at("latestDetected")), Some("true"));
        let newly_added = rows
            .iter()
            .find(|r| r.get(at("code")) == Some("answer_incorrect"))
            .unwrap();
        assert_eq!(newly_added.get(at("productName")), Some("最新杯名"));
        assert_eq!(newly_added.get(at("productSku")), Some("NEW-A"));
        let second_row = rows
            .iter()
            .find(|r| r.get(at("productId")) == second["id"].as_str())
            .unwrap();
        assert_eq!(second_row.get(at("productName")), Some("'=第二杯\n[目标]"));
        assert_eq!(second_row.get(at("productSku")), Some("'@SKU-B"));
        for secret in [
            "RAW_SECRET",
            "NOTE_SECRET",
            "PRIVATE_SECRET",
            "PRIVATE_SOURCE",
            "SECOND_RAW_SECRET",
            "SECOND_NOTE_SECRET",
        ] {
            assert!(!out.to_string().contains(secret), "{secret}");
        }
    }
    #[test]
    fn validates_historical_inputs_and_rejects_tampered_actions_hashes_and_identity() {
        let (c, b, product, campaign) = fixture();
        let mut plan = create_plan(&c, &b, &campaign);
        plan.as_object_mut().unwrap().remove("alreadyExists");
        c.execute("UPDATE m_products SET data=json_set(data,'$.name','历史改名','$.revision',8) WHERE id=?1",[product["id"].as_str().unwrap()]).unwrap();
        validate_database(&c).unwrap();
        for path in ["action", "snapshot", "extra", "request", "time"] {
            let mut bad = plan.clone();
            match path {
                "action" => bad["actions"][0]["title"] = json!("篡改"),
                "snapshot" => {
                    bad["checkpoints"][0]["diagnosis"]["metrics"]["baseline"]["samples"] =
                        json!(999)
                }
                "extra" => bad["secret"] = json!("hidden"),
                "request" => bad["requestId"] = json!(id()),
                _ => bad["updatedAt"] = json!("2000-01-01T00:00:00Z"),
            };
            store(&c, &bad);
            assert!(validate_database(&c).is_err(), "{path}");
            store(&c, &plan);
        }
        assert!(get(&c, &b, &json!({"planId":plan["id"]})).is_ok());
    }
    #[test]
    fn candidate_action_and_storage_bounds_are_checked_before_write() {
        let (c, b, _, campaign) = fixture();
        let mut plan = create_plan(&c, &b, &campaign);
        plan.as_object_mut().unwrap().remove("alreadyExists");
        let original = plan.clone();
        let action = plan["actions"][0].clone();
        plan["actions"] = json!(vec![action; 1001]);
        assert!(validate_plan(&c, &plan).is_err());
        plan = original.clone();
        plan["actions"][0]["note"] = json!("大".repeat(600_000));
        assert!(validate_plan(&c, &plan).is_err());
        assert_eq!(
            get(&c, &b, &json!({"planId":original["id"]})).unwrap(),
            original
        );
    }
    #[test]
    fn csv_cells_guard_formula_and_controls_without_losing_content() {
        for cell in [
            "=1+1",
            "+cmd",
            "-2",
            "@SUM(A1)",
            " leading",
            "\tcontrol",
            "\nline",
            "\rcontrol",
            "\u{7f}control",
        ] {
            let encoded = csv_field(cell);
            let input = format!("value\n{encoded}\n");
            let mut r = ::csv::Reader::from_reader(input.as_bytes());
            assert_eq!(
                r.records().next().unwrap().unwrap().get(0),
                Some(format!("'{cell}").as_str())
            );
        }
        let encoded = csv_field("普通,\"说明\"\n第二行");
        let input = format!("value\n{encoded}\n");
        let mut r = ::csv::Reader::from_reader(input.as_bytes());
        assert_eq!(
            r.records().next().unwrap().unwrap().get(0),
            Some("普通,\"说明\"\n第二行")
        );
    }
    #[test]
    fn large_valid_checkpoints_reject_candidate_size_before_sql_write() {
        let (mut c, b, product, _) = fixture();
        let mut questions = vec![];
        let fs=(0..100).map(|i|json!({"id":format!("custom-{i:03}-{}","x".repeat(85)),"label":"公开参数","value":"虚构","source":"测试来源","visibility":"public","validUntil":null})).collect::<Vec<_>>();
        for i in 0..20 {
            let p=handle(&mut c,"product.save",json!({"brandId":b,"name":format!("大规模虚构商品{i}"),"sku":format!("LARGE-{i}"),"facts":fs})).unwrap();
            let q=handle(&mut c,"question.save",json!({"brandId":b,"productId":p["id"],"text":format!("如何选择场景{i}的装备？"),"intent":"selection","origin":"customer","source":"测试","branded":false,"confirmed":true})).unwrap();
            questions.push(q["id"].clone());
        }
        let campaign=handle(&mut c,"campaign.create",json!({"brandId":b,"name":"大快照容量验收","questionIds":questions,"platforms":["doubao"],"modelLabel":"本地模拟","searchMode":"无请求","context":"模拟","repetitions":1,"evidenceMode":"simulated"})).unwrap();
        let mut plan = create_plan(&c, &b, &campaign);
        plan.as_object_mut().unwrap().remove("alreadyExists");
        let pid = campaign["questions"][0]["productId"].as_str().unwrap();
        let mut rejected = false;
        for rev in 2..=8 {
            c.execute(
                "UPDATE m_products SET data=json_set(data,'$.revision',?1) WHERE id=?2",
                params![rev, pid],
            )
            .unwrap();
            let before = get(&c, &b, &json!({"planId":plan["id"]})).unwrap();
            match check(
                &c,
                &b,
                &json!({"planId":plan["id"],"expectedRevision":plan["revision"]}),
            ) {
                Ok(next) => plan = next,
                Err(e) => {
                    assert!(e.contains("1.5MB"), "{e}");
                    assert_eq!(get(&c, &b, &json!({"planId":plan["id"]})).unwrap(), before);
                    rejected = true;
                    break;
                }
            }
        }
        assert!(rejected, "合法单个快照累积时应触发方案字节限制");
        // Mutable unrelated products have no bearing on frozen plan validation.
        c.execute(
            "UPDATE m_products SET data=json_set(data,'$.name','无关改名') WHERE id=?1",
            [product["id"].as_str().unwrap()],
        )
        .unwrap();
        validate_database(&c).unwrap();
    }
    #[test]
    fn stale_revision_rejects_before_recomputing_mutated_live_inputs() {
        let (c, b, product, campaign) = fixture();
        let plan = create_plan(&c, &b, &campaign);
        let current=update(&c,&b,&json!({"planId":plan["id"],"expectedRevision":1,"actionKey":plan["actions"][0]["key"],"status":"in_progress","note":"处理"})).unwrap();
        c.execute(
            "UPDATE m_products SET data=json_set(data,'$.revision',0) WHERE id=?1",
            [product["id"].as_str().unwrap()],
        )
        .unwrap();
        assert!(
            check(&c, &b, &json!({"planId":plan["id"],"expectedRevision":1}))
                .unwrap_err()
                .contains("重新加载")
        );
        assert_eq!(get(&c, &b, &json!({"planId":plan["id"]})).unwrap(), current);
    }
    #[test]
    fn stored_create_hash_must_be_exact_canonical_hex() {
        let (c, b, _, campaign) = fixture();
        let mut plan = create_plan(&c, &b, &campaign);
        plan.as_object_mut().unwrap().remove("alreadyExists");
        plan["createFingerprint"] =
            json!(format!(" {} ", plan["createFingerprint"].as_str().unwrap()));
        store(&c, &plan);
        assert!(validate_database(&c).is_err());
    }
    #[test]
    fn empty_plans_are_readable_and_validate() {
        let (c, b) = db();
        assert_eq!(list(&c, &b, &json!({})).unwrap(), json!([]));
        validate_database(&c).unwrap();
    }
}
