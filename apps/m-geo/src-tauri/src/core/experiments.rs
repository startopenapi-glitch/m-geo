//! Immutable plans keep consumer-question measurements comparable and duplicate-safe.
use super::*;
const MAX_CAMPAIGN_BYTES: usize = 200_000;
pub(super) const TABLE_SQL:&str="CREATE TABLE IF NOT EXISTS m_campaigns(id TEXT PRIMARY KEY,brand_id TEXT NOT NULL,data TEXT NOT NULL,FOREIGN KEY(brand_id) REFERENCES m_brands(id));";
fn count(v: &Value, key: &str, max: i64) -> Result<i64> {
    let n = number(v, key)?;
    if n > max {
        return Err(format!("{key} 超出范围"));
    }
    Ok(n)
}
fn platforms(p: &Value) -> Result<Value> {
    let a = p["platforms"].as_array().ok_or("请选择测试平台")?;
    let mut seen = HashSet::new();
    if a.is_empty() || a.len() > 4 {
        return Err("请选择1至4个平台".into());
    }
    for v in a {
        let s = v.as_str().ok_or("测试平台格式错误")?;
        if !["doubao", "yuanbao", "qianwen", "baidu"].contains(&s) || !seen.insert(s) {
            return Err("测试平台无效或重复".into());
        }
    }
    Ok(json!(a))
}
pub(super) fn evidence_mode(plan: &Value) -> Result<&str> {
    match plan.get("evidenceMode") {
        None => Ok("unspecified"),
        Some(Value::String(s))
            if ["recorded", "simulated", "unspecified"].contains(&s.as_str()) =>
        {
            Ok(s)
        }
        _ => Err("样本性质必须是人工实测、模拟或历史未分类".into()),
    }
}
pub(super) fn create(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    let name = text(p, "name", 100, true)?;
    let mode = evidence_mode(p)?;
    let reps = count(p, "repetitions", 10)?;
    let platforms = platforms(p)?;
    let ids = p["questionIds"].as_array().ok_or("请选择固定问题")?;
    if ids.is_empty() || ids.len() > 100 {
        return Err("每组选择1至100个已确认非品牌问题".into());
    }
    let mut seen = HashSet::new();
    let mut questions = vec![];
    let br = brand(c, b)?;
    for qid in ids {
        let qid = qid.as_str().ok_or("问题标识错误")?;
        if !seen.insert(qid) {
            return Err("测试问题重复".into());
        }
        let q = load(c, "m_questions", b, qid)?;
        let prod = load(
            c,
            "m_products",
            b,
            q["productId"].as_str().ok_or("问题商品缺失")?,
        )?;
        let txt = q["text"].as_str().ok_or("问题内容错误")?;
        if q["confirmed"] != true
            || q["branded"] != false
            || mentions_brand(txt, br["name"].as_str().unwrap_or(""))
            || mentions_brand(txt, prod["name"].as_str().unwrap_or(""))
            || mentions_brand(txt, prod["sku"].as_str().unwrap_or(""))
        {
            return Err("基线计划只接受已确认且不包含品牌、商品名称或 SKU 的问题".into());
        }
        questions.push(json!({"id":q["id"],"productId":q["productId"],"text":q["text"]}));
    }
    let v = json!({"id":id(),"brandId":b,"name":name,"questions":questions,"platforms":platforms,"modelLabel":text(p,"modelLabel",200,true)?,"searchMode":text(p,"searchMode",200,true)?,"context":text(p,"context",500,true)?,"repetitions":reps,"createdAt":now(),"evidenceMode":mode});
    if v.to_string().len() > MAX_CAMPAIGN_BYTES {
        return Err("测试计划超过200KB，请减少或精简问题后重试".into());
    }
    c.execute(
        "INSERT INTO m_campaigns(id,brand_id,data)VALUES(?1,?2,?3)",
        params![v["id"].as_str(), b, v.to_string()],
    )
    .map_err(db_error)?;
    Ok(v)
}
pub(super) fn assessment(c: &Connection, b: &str, o: &Value, historical: bool) -> Result<Value> {
    let plan = if o["assessment"]["campaignId"].as_str().is_some() {
        Some(load(
            c,
            "m_campaigns",
            b,
            &guid(&o["assessment"], "campaignId")?,
        )?)
    } else {
        None
    };
    let result = assessment_with_plan(o, plan.as_ref())?;
    if !historical && result["campaignId"].as_str().is_some() {
        checked_question(
            c,
            b,
            o["productId"].as_str().ok_or("商品缺失")?,
            o["questionId"].as_str().ok_or("问题缺失")?,
        )?;
    }
    Ok(result)
}
pub(super) fn assessment_with_plan(o: &Value, plan: Option<&Value>) -> Result<Value> {
    let Some(a) = o.get("assessment") else {
        return Ok(Value::Null);
    };
    imports::keys(
        a,
        &[
            "campaignId",
            "repetition",
            "context",
            "recommendation",
            "rank",
            "competitors",
        ],
    )?;
    let context = text(a, "context", 500, true)?;
    let repetition = count(a, "repetition", 10)?;
    let recommendation = choice(a, "recommendation", &["unknown", "yes", "no"])?;
    let rank = match a.get("rank") {
        Some(Value::Null) => Value::Null,
        Some(_) => json!(count(a, "rank", 100)?),
        None => return Err("推荐顺位字段缺失".into()),
    };
    if (!rank.is_null() && recommendation != "yes")
        || (recommendation == "yes" && o["mentioned"] != true)
    {
        return Err("推荐与提及/顺位矛盾，请重新核对".into());
    }
    let campaign_id = match a.get("campaignId") {
        Some(Value::Null) => Value::Null,
        Some(_) => json!(guid(a, "campaignId")?),
        None => return Err("测试计划字段缺失".into()),
    };
    if let Some(cid) = campaign_id.as_str() {
        let plan = plan.ok_or("测试计划缺失")?;
        if plan["id"] != cid {
            return Err("测试计划归属错误".into());
        }
        let q = plan["questions"]
            .as_array()
            .ok_or("计划问题损坏")?
            .iter()
            .find(|q| q["id"] == o["questionId"] && q["productId"] == o["productId"])
            .ok_or("问题不在此固定测试计划中")?;
        let real_api = o["surface"] == "api"
            && o["evidenceMode"] == "recorded"
            && o["sourceKind"] == "real_api"
            && o["transport"] == "real_api";
        if repetition > number(plan, "repetitions")?
            || (o["surface"] != "consumer" && !real_api)
            || (real_api && evidence_mode(plan)? != "recorded")
            || !plan["platforms"]
                .as_array()
                .ok_or("计划平台损坏")?
                .contains(&o["platform"])
            || o["modelLabel"] != plan["modelLabel"]
            || o["searchMode"] != plan["searchMode"]
            || context != plan["context"].as_str().unwrap_or("")
            || o["querySnapshot"] != q["text"]
            || o["branded"] != false
        {
            return Err("测试条件与固定计划不一致，请使用原问题、消费者入口及相同条件".into());
        }
    }
    Ok(
        json!({"campaignId":campaign_id,"repetition":repetition,"context":context,"recommendation":recommendation,"rank":rank,"competitors":text(a,"competitors",2000,false)?}),
    )
}
pub(super) fn check_slot(c: &Connection, b: &str, v: &Value) -> Result<()> {
    if v["assessment"]["campaignId"].is_null() {
        return Ok(());
    }
    let exists:i64=c.query_row("SELECT count(*) FROM m_observations WHERE brand_id=?1 AND question_id=?2 AND json_extract(data,'$.assessment.campaignId')=?3 AND json_extract(data,'$.platform')=?4 AND json_extract(data,'$.phase')=?5 AND json_extract(data,'$.assessment.repetition')=?6",params![b,v["questionId"].as_str(),v["assessment"]["campaignId"].as_str(),v["platform"].as_str(),v["phase"].as_str(),v["assessment"]["repetition"].as_i64()],|r|r.get(0)).map_err(db_error)?;
    if exists > 0 {
        return Err(
            "这个问题、平台、阶段与轮次已有样本，请选择下一轮；不同回答不能覆盖旧证据".into(),
        );
    }
    Ok(())
}
pub(super) fn validate(c: &Connection) -> Result<()> {
    let mut st = c
        .prepare("SELECT id,brand_id,data FROM m_campaigns")
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
        let (id, b, s) = row.map_err(db_error)?;
        if s.len() > MAX_CAMPAIGN_BYTES {
            return Err("测试计划过大".into());
        }
        let v = parse_data(s)?;
        imports::keys(
            &v,
            &[
                "id",
                "brandId",
                "name",
                "questions",
                "platforms",
                "modelLabel",
                "searchMode",
                "context",
                "repetitions",
                "createdAt",
                "evidenceMode",
            ],
        )?;
        if guid(&v, "id")? != id || guid(&v, "brandId")? != b {
            return Err("测试计划身份错误".into());
        }
        brand(c, &b)?;
        evidence_mode(&v)?;
        text(&v, "name", 100, true)?;
        text(&v, "modelLabel", 200, true)?;
        text(&v, "searchMode", 200, true)?;
        text(&v, "context", 500, true)?;
        date_time(&v, "createdAt")?;
        count(&v, "repetitions", 10)?;
        platforms(&v)?;
        let qs = v["questions"].as_array().ok_or("计划问题损坏")?;
        if qs.is_empty() || qs.len() > 100 {
            return Err("计划问题数无效".into());
        }
        let mut seen = HashSet::new();
        for q in qs {
            imports::keys(q, &["id", "productId", "text"])?;
            let qid = guid(q, "id")?;
            let pid = guid(q, "productId")?;
            let existing = load(c, "m_questions", &b, &qid)?;
            if !seen.insert(qid)
                || existing["productId"] != pid
                || existing["text"] != text(q, "text", 1000, true)?
            {
                return Err("计划问题归属或原文不一致".into());
            }
        }
    }
    let mut st = c
        .prepare("SELECT data FROM m_observations")
        .map_err(db_error)?;
    let mut seen = HashSet::new();
    for row in st
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(db_error)?
    {
        let v = parse_data(row.map_err(db_error)?)?;
        if !v["assessment"]["campaignId"].is_null() {
            let key = json!([
                v["brandId"],
                v["questionId"],
                v["assessment"]["campaignId"],
                v["platform"],
                v["phase"],
                v["assessment"]["repetition"]
            ])
            .to_string();
            if !seen.insert(key) {
                return Err("测试计划样本轮次重复".into());
            }
        }
    }
    Ok(())
}
