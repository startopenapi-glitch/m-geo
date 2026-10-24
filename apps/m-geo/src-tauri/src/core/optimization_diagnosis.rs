//! Deterministic local findings from privacy-reduced frozen evidence.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
const FORMAT: &str = "m-geo-optimization-diagnosis-v1";
const MAX_BYTES: usize = 400_000;
const SNAPSHOT_KEYS: &[&str] = &[
    "format",
    "brandId",
    "campaign",
    "evidenceMode",
    "phase",
    "asOf",
    "products",
    "observations",
    "metrics",
    "findings",
    "fingerprint",
];
fn budget(v: &Value) -> Result<()> {
    if v.to_string().len() > MAX_BYTES {
        return Err(
            "诊断快照超过400,000字节，请减少固定计划的问题、轮次或关联内容后重新诊断；不会截断证据"
                .into(),
        );
    }
    Ok(())
}
fn exact(v: &Value, keys: &[&str]) -> Result<()> {
    imports::keys(v, keys)?;
    if v.as_object().is_none_or(|o| o.len() != keys.len()) {
        return Err("诊断快照字段缺失，请重新诊断或检查备份".into());
    }
    Ok(())
}
fn string<'a>(v: &'a Value, k: &str, max: usize, required: bool) -> Result<&'a str> {
    raw(v, k, max, required)?;
    v[k].as_str()
        .ok_or_else(|| format!("诊断文字字段 {k} 无效"))
}
fn enum_value<'a>(v: &'a Value, k: &str, values: &[&str]) -> Result<&'a str> {
    let s = string(v, k, 50, true)?;
    if !values.contains(&s) {
        return Err(format!("诊断选项 {k} 无效"));
    }
    Ok(s)
}
fn identity(v: &Value, k: &str) -> Result<String> {
    let s = guid(v, k)?;
    if v[k] != s {
        return Err("诊断标识含多余空格".into());
    }
    Ok(s)
}
fn date(v: &Value, k: &str) -> Result<()> {
    let s = string(v, k, 10, true)?;
    let d =
        NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| "诊断日期必须为有效 YYYY-MM-DD")?;
    if s.len() != 10 || d.format("%Y-%m-%d").to_string() != s {
        return Err("诊断日期必须为有效 YYYY-MM-DD".into());
    }
    Ok(())
}
fn rows<'a>(v: &'a Value, k: &str) -> Result<&'a Vec<Value>> {
    v[k].as_array().ok_or_else(|| format!("诊断列表 {k} 无效"))
}
fn sorted(rows: &[Value]) -> Result<()> {
    let mut previous: Option<&str> = None;
    for v in rows {
        let current = string(v, "id", 100, true)?;
        if previous.is_some_and(|p| p >= current) {
            return Err("诊断记录须按标识排序且不可重复".into());
        }
        previous = Some(current);
    }
    Ok(())
}
fn validate_inputs(v: &Value) -> Result<()> {
    budget(v)?;
    exact(v, SNAPSHOT_KEYS)?;
    if v["format"] != FORMAT {
        return Err("诊断快照格式不支持".into());
    }
    let b = identity(v, "brandId")?;
    enum_value(v, "phase", &["baseline", "followup"])?;
    date(v, "asOf")?;
    let campaign = &v["campaign"];
    let mut campaign_keys = vec![
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
    ];
    if campaign.get("evidenceMode").is_some() {
        campaign_keys.push("evidenceMode");
    }
    exact(campaign, &campaign_keys)?;
    identity(campaign, "id")?;
    if identity(campaign, "brandId")? != b {
        return Err("诊断计划不属于当前品牌".into());
    }
    if v["evidenceMode"] != experiments::evidence_mode(campaign)? {
        return Err("诊断样本性质与固定计划不一致".into());
    }
    string(campaign, "name", 100, true)?;
    string(campaign, "modelLabel", 200, true)?;
    string(campaign, "searchMode", 200, true)?;
    string(campaign, "context", 500, true)?;
    date_time(campaign, "createdAt")?;
    let reps = number(campaign, "repetitions")?;
    let platforms = rows(campaign, "platforms")?;
    let questions = rows(campaign, "questions")?;
    if !(1..=10).contains(&reps)
        || platforms.is_empty()
        || platforms.len() > 4
        || questions.is_empty()
        || questions.len() > 100
    {
        return Err("诊断固定计划范围无效".into());
    }
    let mut platform_ids = HashSet::new();
    for platform in platforms {
        let s = platform.as_str().ok_or("诊断平台无效")?;
        if !["doubao", "yuanbao", "qianwen", "baidu"].contains(&s) || !platform_ids.insert(s) {
            return Err("诊断平台无效或重复".into());
        }
    }
    let mut question_products = BTreeMap::new();
    let mut needed = BTreeSet::new();
    for q in questions {
        exact(q, &["id", "productId", "text"])?;
        let qid = identity(q, "id")?;
        let pid = identity(q, "productId")?;
        string(q, "text", 1000, true)?;
        needed.insert(pid.clone());
        if question_products.insert(qid, pid).is_some() {
            return Err("诊断固定问题重复".into());
        }
    }
    let products = rows(v, "products")?;
    sorted(products)?;
    let mut found = BTreeSet::new();
    let mut draft_ids = HashSet::new();
    for p in products {
        exact(
            p,
            &[
                "id",
                "name",
                "sku",
                "revision",
                "confirmed",
                "facts",
                "drafts",
            ],
        )?;
        found.insert(identity(p, "id")?);
        string(p, "name", 200, true)?;
        string(p, "sku", 100, true)?;
        number(p, "revision")?;
        boolean(p, "confirmed")?;
        let facts = rows(p, "facts")?;
        if facts.len() > 100 {
            return Err("诊断商品事实超过100条".into());
        }
        sorted(facts)?;
        for f in facts {
            exact(f, &["id", "hasSource", "validUntil", "unknown"])?;
            let fid = string(f, "id", 100, true)?;
            if fid.trim() != fid {
                return Err("诊断事实标识无效".into());
            }
            if let Some(key) = fid.strip_prefix("commerce:") {
                if !commerce::FIELDS.iter().any(|(k, _)| *k == key) {
                    return Err("诊断购买字段无效".into());
                }
            }
            boolean(f, "hasSource")?;
            if boolean(f, "unknown")? && fid != "commerce:availability" {
                return Err("诊断未知库存标识无效".into());
            }
            if !f["validUntil"].is_null() {
                date(f, "validUntil")?;
            }
        }
        let drafts = rows(p, "drafts")?;
        sorted(drafts)?;
        for d in drafts {
            exact(d, &["id", "kind", "revision", "sourceRevision", "reviewed"])?;
            if !draft_ids.insert(identity(d, "id")?) {
                return Err("诊断内容标识重复".into());
            }
            enum_value(d, "kind", &["description", "faq", "guide", "comparison"])?;
            number(d, "revision")?;
            number(d, "sourceRevision")?;
            boolean(d, "reviewed")?;
        }
    }
    if found != needed {
        return Err("诊断商品与固定问题归属不一致".into());
    }
    let observations = rows(v, "observations")?;
    if observations.len() > questions.len() * platforms.len() * reps as usize * 2 {
        return Err("诊断样本数量超出固定计划".into());
    }
    sorted(observations)?;
    let mut slots = HashSet::new();
    for o in observations {
        exact(
            o,
            &[
                "id",
                "productId",
                "questionId",
                "platform",
                "phase",
                "repetition",
                "assessmentRevision",
                "mentioned",
                "accuracy",
                "recommendation",
                "rank",
                "cited",
            ],
        )?;
        identity(o, "id")?;
        let pid = identity(o, "productId")?;
        let qid = identity(o, "questionId")?;
        if question_products.get(&qid) != Some(&pid) || !platforms.contains(&o["platform"]) {
            return Err("诊断样本问题、商品或平台不属于计划".into());
        }
        enum_value(o, "phase", &["baseline", "followup"])?;
        let rep = number(o, "repetition")?;
        if rep > reps {
            return Err("诊断样本轮次超出计划".into());
        }
        number(o, "assessmentRevision")?;
        let mentioned = boolean(o, "mentioned")?;
        boolean(o, "cited")?;
        enum_value(o, "accuracy", &["unknown", "correct", "incorrect"])?;
        let recommendation = enum_value(o, "recommendation", &["unknown", "yes", "no"])?;
        if recommendation == "yes" && !mentioned {
            return Err("诊断推荐与提及矛盾".into());
        }
        if !o["rank"].is_null() && (recommendation != "yes" || number(o, "rank")? > 100) {
            return Err("诊断推荐顺位无效".into());
        }
        let slot = json!([qid, o["platform"], o["phase"], rep]).to_string();
        if !slots.insert(slot) {
            return Err("诊断样本轮次重复".into());
        }
    }
    Ok(())
}
pub(super) fn prepare(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    let phase = enum_value(p, "phase", &["baseline", "followup"])?;
    let report = reports::prepare(c, b, p)?;
    let campaign = &report["summary"]["campaign"];
    let evidence_file = rows(&report, "files")?
        .iter()
        .find(|f| f["path"] == "evidence.json")
        .ok_or("固定计划证据缺失")?;
    let evidence: Value = serde_json::from_str(
        evidence_file["content"]
            .as_str()
            .ok_or("固定计划证据格式无效")?,
    )
    .map_err(|_| "固定计划证据格式无效")?;
    let ids = rows(campaign, "questions")?
        .iter()
        .map(|q| q["productId"].as_str().ok_or("固定计划商品标识缺失"))
        .collect::<std::result::Result<BTreeSet<_>, _>>()?;
    let mut products = vec![];
    for pid in ids {
        let product = load(c, "m_products", b, pid)?;
        if product["id"] != pid || product["brandId"] != b {
            return Err("诊断商品身份与品牌不一致".into());
        }
        let mut facts = vec![];
        for f in rows(&product, "facts")? {
            let visibility = enum_value(f, "visibility", &["public", "private"])?;
            if visibility == "public" {
                let source = string(f, "source", 2000, false)?;
                facts.push(json!({"id":f["id"],"hasSource":!source.trim().is_empty(),"validUntil":f["validUntil"],"unknown":f["id"]=="commerce:availability" && f["value"]=="Unknown"}));
            }
        }
        facts.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
        let mut st = c.prepare("SELECT id,brand_id,product_id,data FROM m_drafts WHERE brand_id=?1 AND product_id=?2").map_err(db_error)?;
        let mut drafts = vec![];
        for row in st
            .query_map(params![b, pid], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })
            .map_err(db_error)?
        {
            let (did, db_brand, db_product, data) = row.map_err(db_error)?;
            let d = parse_data(data)?;
            if d["id"] != did || d["brandId"] != db_brand || d["productId"] != db_product {
                return Err("诊断内容归属不一致".into());
            }
            drafts.push(json!({"id":d["id"],"kind":d["kind"],"revision":d["revision"],"sourceRevision":d["sourceRevision"],"reviewed":!d["reviewedAt"].is_null()}));
            // Stop an oversized metadata collection without silently truncating it.
            budget(&json!({"products":products,"drafts":drafts}))?;
        }
        drafts.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
        products.push(json!({"id":product["id"],"name":product["name"],"sku":product["sku"],"revision":product["revision"],"confirmed":!product["confirmedAt"].is_null(),"facts":facts,"drafts":drafts}));
        budget(&json!(products))?;
    }
    let mut observations = vec![];
    for o in rows(&evidence, "observations")? {
        observations.push(json!({"id":o["id"],"productId":o["productId"],"questionId":o["questionId"],"platform":o["platform"],"phase":o["phase"],"repetition":o["assessment"]["repetition"],"assessmentRevision":o.get("assessmentRevision").cloned().unwrap_or(json!(1)),"mentioned":o["mentioned"],"accuracy":o["accuracy"],"recommendation":o["assessment"]["recommendation"],"rank":o["assessment"]["rank"],"cited":!string(o,"citedUrl",2000,false)?.trim().is_empty()}));
    }
    observations.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    recompute(
        &json!({"format":FORMAT,"brandId":b,"campaign":campaign,"evidenceMode":report["summary"]["evidenceMode"],"phase":phase,"asOf":Utc::now().date_naive().format("%Y-%m-%d").to_string(),"products":products,"observations":observations,"metrics":null,"findings":null,"fingerprint":null}),
    )
}
fn stats(rows: &[&Value]) -> Value {
    let count = |predicate: fn(&Value) -> bool| rows.iter().filter(|o| predicate(o)).count();
    let assessed = count(|o| o["recommendation"] != "unknown");
    json!({"samples":rows.len(),"assessed":assessed,"recommended":count(|o|o["recommendation"]=="yes"),"unknown":rows.len()-assessed,"ranked":count(|o|o["rank"].as_u64().is_some()),"top3":count(|o|o["rank"].as_u64().is_some_and(|n|n<=3)),"cited":count(|o|o["cited"]==true),"incorrect":count(|o|o["accuracy"]=="incorrect")})
}
fn metrics(v: &Value) -> Value {
    let obs = v["observations"]
        .as_array()
        .expect("validated observations");
    let baseline = obs
        .iter()
        .filter(|o| o["phase"] == "baseline")
        .collect::<Vec<_>>();
    let followup = obs
        .iter()
        .filter(|o| o["phase"] == "followup")
        .collect::<Vec<_>>();
    let mut slots = BTreeMap::<String, [Option<&Value>; 2]>::new();
    for o in obs {
        let key = json!([o["questionId"], o["platform"], o["repetition"]]).to_string();
        slots.entry(key).or_default()[usize::from(o["phase"] == "followup")] = Some(o);
    }
    let (mut pairs, mut assessed, mut byes, mut fyes) = (0usize, 0usize, 0usize, 0usize);
    for pair in slots.values() {
        if let [Some(b), Some(f)] = pair {
            pairs += 1;
            if b["recommendation"] != "unknown" && f["recommendation"] != "unknown" {
                assessed += 1;
                byes += usize::from(b["recommendation"] == "yes");
                fyes += usize::from(f["recommendation"] == "yes");
            }
        }
    }
    let c = &v["campaign"];
    json!({"expectedPerPhase":c["questions"].as_array().unwrap().len()*c["platforms"].as_array().unwrap().len()*c["repetitions"].as_u64().unwrap() as usize,"baseline":stats(&baseline),"followup":stats(&followup),"matchedPairs":pairs,"matchedAssessedPairs":assessed,"matchedBaselineRecommended":byes,"matchedFollowupRecommended":fyes,"deltaPercentagePoints":if assessed>0 {json!((fyes as f64-byes as f64)*100.0/assessed as f64)} else {Value::Null}})
}
struct Finding<'a> {
    product: &'a Value,
    all: Vec<Value>,
}
impl Finding<'_> {
    fn add(
        &mut self,
        code: &str,
        priority: &str,
        title: &str,
        detail: String,
        section: &str,
        refs: Vec<String>,
    ) {
        let refs = refs.into_iter().collect::<BTreeSet<_>>();
        self.all.push(json!({"key":format!("{code}:{}",self.product["id"].as_str().unwrap()),"code":code,"productId":self.product["id"],"priority":priority,"title":title,"detail":detail,"section":section,"evidenceIds":refs}));
    }
}
fn findings(v: &Value) -> Value {
    let mut all = vec![];
    let campaign = &v["campaign"];
    for p in v["products"].as_array().unwrap() {
        let mut f = Finding {
            product: p,
            all: vec![],
        };
        let facts = p["facts"].as_array().unwrap();
        let mut missing = vec![];
        let mut refs = vec![];
        for (key, label) in commerce::FIELDS {
            let fid = format!("commerce:{key}");
            let fact = facts.iter().find(|fact| fact["id"] == fid);
            if fact.is_none_or(|fact| fact["hasSource"] != true || fact["unknown"] == true) {
                missing.push(*label);
                if fact.is_some() {
                    refs.push(fid);
                }
            }
        }
        if !missing.is_empty() {
            f.add(
                "facts_missing",
                "high",
                "购买资料缺失或依据待补充",
                format!(
                    "请补充有依据的公开字段：{}。未知库存需核对后填写。",
                    missing.join("、")
                ),
                "products",
                refs,
            );
        }
        let expired = facts
            .iter()
            .filter(|fact| {
                fact["validUntil"]
                    .as_str()
                    .is_some_and(|d| d < v["asOf"].as_str().unwrap())
                    || (fact["validUntil"].is_null()
                        && ["commerce:price", "commerce:availability"]
                            .contains(&fact["id"].as_str().unwrap()))
            })
            .map(|fact| fact["id"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        if !expired.is_empty() {
            f.add(
                "facts_expired",
                "high",
                "公开事实有效期待更新",
                format!(
                    "截至 {}，有公开事实已过期，或价格、库存缺少有效期；请核对依据并更新。",
                    v["asOf"].as_str().unwrap()
                ),
                "products",
                expired,
            );
        }
        if p["confirmed"] == false {
            f.add(
                "facts_unconfirmed",
                "high",
                "当前商品事实尚未确认",
                "请核对并确认当前版本公开事实。".into(),
                "products",
                vec![],
            );
        }
        let descriptions = p["drafts"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|d| d["kind"] == "description")
            .collect::<Vec<_>>();
        if !descriptions
            .iter()
            .any(|d| d["reviewed"] == true && d["sourceRevision"] == p["revision"])
        {
            f.add(
                "description_pending",
                "normal",
                "当前版本商品描述待复核",
                "请复核与当前商品版本一致的商品描述。".into(),
                "content",
                descriptions
                    .iter()
                    .map(|d| d["id"].as_str().unwrap().to_owned())
                    .collect(),
            );
        }
        let selected = v["observations"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|o| o["productId"] == p["id"] && o["phase"] == v["phase"])
            .collect::<Vec<_>>();
        let expected = campaign["questions"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|q| q["productId"] == p["id"])
            .count()
            * campaign["platforms"].as_array().unwrap().len()
            * campaign["repetitions"].as_u64().unwrap() as usize;
        let ids = |observations: &[&Value]| {
            observations
                .iter()
                .map(|o| o["id"].as_str().unwrap().to_owned())
                .collect::<Vec<_>>()
        };
        if selected.len() < expected {
            f.add(
                "samples_missing",
                "normal",
                "所选阶段样本尚未齐全",
                format!(
                    "预期 {expected} 条，已记录 {} 条，缺失 {} 条；请按固定条件补充记录。",
                    selected.len(),
                    expected - selected.len()
                ),
                "visibility",
                ids(&selected),
            );
        }
        let pending = selected
            .iter()
            .copied()
            .filter(|o| o["recommendation"] == "unknown" || o["accuracy"] == "unknown")
            .collect::<Vec<_>>();
        if !pending.is_empty() {
            f.add(
                "assessments_pending",
                "normal",
                "记录样本仍待人工核对",
                "有样本的推荐或准确性尚未核对；未知项不计为否。".into(),
                "visibility",
                ids(&pending),
            );
        }
        let incorrect = selected
            .iter()
            .copied()
            .filter(|o| o["accuracy"] == "incorrect")
            .collect::<Vec<_>>();
        if !incorrect.is_empty() {
            f.add(
                "answer_incorrect",
                "high",
                "记录答案存在不准确项",
                "请核对原始回答与商品事实；此记录不证明问题由现有内容造成。".into(),
                "visibility",
                ids(&incorrect),
            );
        }
        let complete = expected > 0 && selected.len() == expected;
        if complete && selected.iter().all(|o| o["recommendation"] == "no") {
            f.add(
                "recommendation_gap",
                "normal",
                "完整记录样本未出现推荐",
                "请检查问题意图与商品相关性，并在相同条件下复测；调整不保证获得推荐。".into(),
                "visibility",
                ids(&selected),
            );
        }
        if complete && selected.iter().all(|o| o["cited"] == false) {
            f.add(
                "citations_absent",
                "normal",
                "完整记录样本均无引用",
                "所记录样本未附引用；这不证明商品或内容未被收录，请核对原始证据并按相同条件复测。"
                    .into(),
                "visibility",
                ids(&selected),
            );
        }
        all.extend(f.all);
    }
    all.sort_by(|a, b| a["key"].as_str().cmp(&b["key"].as_str()));
    json!(all)
}
pub(super) fn recompute(v: &Value) -> Result<Value> {
    validate_inputs(v)?;
    let mut result = v.clone();
    result["metrics"] = metrics(v);
    result["findings"] = findings(v);
    result.as_object_mut().unwrap().remove("fingerprint");
    let hash = fingerprint(&result);
    result["fingerprint"] = json!(hash);
    budget(&result)?;
    Ok(result)
}
pub(super) fn validate_snapshot(v: &Value) -> Result<()> {
    if recompute(v)? != *v {
        return Err(
            "诊断快照指标、问题清单或指纹不一致，请检查备份；历史记录不会按当前资料重写".into(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    const B: &str = "11111111-1111-4111-8111-111111111111";
    const C: &str = "22222222-2222-4222-8222-222222222222";
    const P: &str = "33333333-3333-4333-8333-333333333333";
    const Q: &str = "44444444-4444-4444-8444-444444444444";
    const D: &str = "55555555-5555-4555-8555-555555555555";
    fn fixture() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE m_campaigns(id TEXT,brand_id TEXT,data TEXT);CREATE TABLE m_products(id TEXT,brand_id TEXT,data TEXT);CREATE TABLE m_drafts(id TEXT,brand_id TEXT,product_id TEXT,data TEXT);CREATE TABLE m_observations(brand_id TEXT,data TEXT);").unwrap();
        let campaign = json!({"id":C,"brandId":B,"name":"虚构实验","questions":[{"id":Q,"productId":P,"text":"适用什么场景？"}],"platforms":["doubao"],"modelLabel":"fixture","searchMode":"off","context":"fixture","repetitions":2,"createdAt":"2026-01-01T00:00:00Z","evidenceMode":"simulated"});
        let facts = commerce::FIELDS.iter().map(|(k,_)| json!({"id":format!("commerce:{k}"),"visibility":"public","source":"SECRET_SOURCE","value":if *k=="availability" {"InStock"} else {"SECRET_VALUE"},"validUntil":"2030-01-01"})).collect::<Vec<_>>();
        let product = json!({"id":P,"brandId":B,"name":"虚构商品","sku":"FICTION-7","revision":3,"confirmedAt":"2026-01-01T00:00:00Z","facts":facts,"sourceText":"SECRET_SOURCE_TEXT","privateNotes":"SECRET_PRIVATE_NOTES"});
        c.execute(
            "INSERT INTO m_campaigns VALUES(?1,?2,?3)",
            params![C, B, campaign.to_string()],
        )
        .unwrap();
        c.execute(
            "INSERT INTO m_products VALUES(?1,?2,?3)",
            params![P, B, product.to_string()],
        )
        .unwrap();
        let draft = json!({"id":D,"brandId":B,"productId":P,"kind":"description","revision":1,"sourceRevision":3,"reviewedAt":"2026-01-02T00:00:00Z","body":"SECRET_DRAFT"});
        c.execute(
            "INSERT INTO m_drafts VALUES(?1,?2,?3,?4)",
            params![D, B, P, draft.to_string()],
        )
        .unwrap();
        c
    }
    fn add(c: &Connection, phase: &str, rep: i64, rec: &str, accuracy: &str, cited: bool) {
        let oid = format!(
            "00000000-0000-4000-8000-{:012}",
            rep + if phase == "followup" { 100 } else { 0 }
        );
        let v = json!({"id":oid,"brandId":B,"questionId":Q,"productId":P,"querySnapshot":"适用什么场景？","platform":"doubao","phase":phase,"surface":"consumer","modelLabel":"fixture","searchMode":"off","branded":false,"mentioned":true,"accuracy":accuracy,"answer":"SECRET_ANSWER","citedUrl":if cited {"https://evidence.example/SECRET_PATH"} else {""},"assessment":{"campaignId":C,"repetition":rep,"context":"fixture","recommendation":rec,"rank":null,"competitors":"SECRET_COMPETITORS"}});
        c.execute(
            "INSERT INTO m_observations VALUES(?1,?2)",
            params![B, v.to_string()],
        )
        .unwrap();
    }
    fn diagnosis(c: &Connection) -> Value {
        prepare(c, B, &json!({"campaignId":C,"phase":"baseline"})).unwrap()
    }
    fn has(v: &Value, code: &str) -> bool {
        v["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["code"] == code)
    }
    fn product(c: &Connection) -> Value {
        load(c, "m_products", B, P).unwrap()
    }
    fn save_product(c: &Connection, v: &Value) {
        c.execute("UPDATE m_products SET data=?1", [v.to_string()])
            .unwrap();
    }
    #[test]
    fn public_source_missing_and_private_fields_do_not_satisfy_facts() {
        let c = fixture();
        let mut p = product(&c);
        p["facts"][0]["visibility"] = json!("private");
        p["facts"][1]["source"] = json!("");
        p["facts"][2]["value"] = json!("Unknown");
        save_product(&c, &p);
        let v = diagnosis(&c);
        let f = v["findings"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["code"] == "facts_missing")
            .unwrap();
        assert!(f["detail"].as_str().unwrap().contains("价格"));
        assert!(f["detail"].as_str().unwrap().contains("币种"));
        assert!(f["detail"].as_str().unwrap().contains("库存状态"));
        assert_eq!(
            f["evidenceIds"],
            json!(["commerce:availability", "commerce:currency"])
        );
        assert_eq!(v["products"][0]["facts"].as_array().unwrap().len(), 8);
        assert_eq!(f["key"], format!("facts_missing:{P}"));
    }
    #[test]
    fn expiration_uses_stored_asof_and_missing_required_dates() {
        let c = fixture();
        let mut v = diagnosis(&c);
        v["asOf"] = json!("2025-01-01");
        let fs = v["products"][0]["facts"].as_array_mut().unwrap();
        fs.iter_mut().find(|f| f["id"] == "commerce:price").unwrap()["validUntil"] = Value::Null;
        fs.iter_mut()
            .find(|f| f["id"] == "commerce:currency")
            .unwrap()["validUntil"] = json!("2024-12-31");
        fs.iter_mut()
            .find(|f| f["id"] == "commerce:availability")
            .unwrap()["validUntil"] = json!("2025-01-01");
        let result = recompute(&v).unwrap();
        let f = result["findings"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["code"] == "facts_expired")
            .unwrap();
        assert_eq!(
            f["evidenceIds"],
            json!(["commerce:currency", "commerce:price"])
        );
        validate_snapshot(&result).unwrap();
        assert_eq!(recompute(&result).unwrap(), result);
    }
    #[test]
    fn only_reviewed_current_description_satisfies_content() {
        let c = fixture();
        assert!(!has(&diagnosis(&c), "description_pending"));
        c.execute(
            "UPDATE m_drafts SET data=json_set(data,'$.sourceRevision',2)",
            [],
        )
        .unwrap();
        assert!(has(&diagnosis(&c), "description_pending"));
        c.execute(
            "UPDATE m_drafts SET data=json_set(data,'$.sourceRevision',3,'$.kind','faq')",
            [],
        )
        .unwrap();
        assert!(has(&diagnosis(&c), "description_pending"));
        c.execute(
            "UPDATE m_drafts SET data=json_set(data,'$.kind','description','$.reviewedAt',NULL)",
            [],
        )
        .unwrap();
        assert!(has(&diagnosis(&c), "description_pending"));
    }
    #[test]
    fn incomplete_unknown_and_missing_rank_are_not_negative() {
        let c = fixture();
        add(&c, "baseline", 1, "no", "correct", false);
        let incomplete = diagnosis(&c);
        assert!(has(&incomplete, "samples_missing"));
        assert!(!has(&incomplete, "recommendation_gap"));
        assert!(!has(&incomplete, "citations_absent"));
        add(&c, "baseline", 2, "unknown", "unknown", false);
        let v = diagnosis(&c);
        assert!(!has(&v, "samples_missing"));
        assert!(!has(&v, "recommendation_gap"));
        assert!(has(&v, "assessments_pending"));
        assert!(has(&v, "citations_absent"));
        assert_eq!(
            v["metrics"]["baseline"],
            json!({"samples":2,"assessed":1,"recommended":0,"unknown":1,"ranked":0,"top3":0,"cited":0,"incorrect":0})
        );
    }
    #[test]
    fn complete_no_recommendations_and_incorrect_answers_create_targeted_findings() {
        let c = fixture();
        add(&c, "baseline", 1, "no", "incorrect", true);
        add(&c, "baseline", 2, "no", "correct", false);
        let v = diagnosis(&c);
        assert!(has(&v, "recommendation_gap"));
        assert!(has(&v, "answer_incorrect"));
        assert!(!has(&v, "citations_absent"));
        assert_eq!(v["metrics"]["baseline"]["incorrect"], 1);
        let mut p = product(&c);
        p["confirmedAt"] = Value::Null;
        save_product(&c, &p);
        assert!(has(&diagnosis(&c), "facts_unconfirmed"));
    }
    #[test]
    fn both_phases_are_frozen_and_only_matched_assessed_pairs_drive_delta() {
        let c = fixture();
        add(&c, "baseline", 1, "no", "correct", true);
        add(&c, "baseline", 2, "unknown", "correct", false);
        add(&c, "followup", 1, "yes", "correct", true);
        add(&c, "followup", 2, "no", "incorrect", false);
        let v = diagnosis(&c);
        assert_eq!(v["observations"].as_array().unwrap().len(), 4);
        assert_eq!(v["metrics"]["matchedPairs"], 2);
        assert_eq!(v["metrics"]["matchedAssessedPairs"], 1);
        assert_eq!(v["metrics"]["matchedBaselineRecommended"], 0);
        assert_eq!(v["metrics"]["matchedFollowupRecommended"], 1);
        assert_eq!(v["metrics"]["deltaPercentagePoints"], 100.0);
        assert!(!has(&v, "answer_incorrect"));
        let followup = prepare(&c, B, &json!({"campaignId":C,"phase":"followup"})).unwrap();
        assert!(has(&followup, "answer_incorrect"));
        assert_eq!(followup["metrics"], v["metrics"]);
    }
    #[test]
    fn compact_snapshot_does_not_export_content_sources_or_private_facts() {
        let c = fixture();
        let mut p = product(&c);
        p["facts"].as_array_mut().unwrap().push(json!({"id":"SECRET_PRIVATE_FACT","visibility":"private","source":"SECRET","value":"SECRET"}));
        save_product(&c, &p);
        add(&c, "baseline", 1, "yes", "correct", true);
        let v = diagnosis(&c);
        assert!(!v.to_string().contains("SECRET"));
        assert_eq!(v["observations"][0]["assessmentRevision"], 1);
        assert_eq!(v["observations"][0]["cited"], true);
        validate_snapshot(&v).unwrap();
    }
    #[test]
    fn frozen_history_and_hash_survive_current_changes_but_detect_tampering() {
        let c = fixture();
        add(&c, "baseline", 1, "yes", "correct", true);
        let v = diagnosis(&c);
        let before = c.total_changes();
        let again = diagnosis(&c);
        assert_eq!(before, c.total_changes());
        assert_eq!(v, again);
        let mut p = product(&c);
        p["name"] = json!("已更名");
        p["revision"] = json!(4);
        save_product(&c, &p);
        c.execute(
            "UPDATE m_observations SET data=json_set(data,'$.accuracy','incorrect')",
            [],
        )
        .unwrap();
        validate_snapshot(&v).unwrap();
        assert_eq!(recompute(&v).unwrap(), v);
        for key in ["fingerprint", "metrics", "findings"] {
            let mut bad = v.clone();
            bad[key] = Value::Null;
            assert!(validate_snapshot(&bad).is_err(), "{key}");
        }
        let mut bad = v.clone();
        bad["products"][0]["revision"] = json!(999);
        assert!(validate_snapshot(&bad).is_err());
        let mut bad = v.clone();
        bad["products"][0]["sourceText"] = json!("private");
        assert!(recompute(&bad).is_err());
        let mut bad = v.clone();
        bad["asOf"] = json!("2025-02-30");
        assert!(recompute(&bad).is_err());
    }
    #[test]
    fn invalid_frozen_relationships_sorting_and_slots_are_rejected() {
        let c = fixture();
        add(&c, "baseline", 1, "yes", "correct", true);
        let v = diagnosis(&c);
        let mut bad = v.clone();
        bad["observations"][0]["productId"] = json!(C);
        assert!(recompute(&bad).is_err());
        let mut bad = v.clone();
        bad["observations"][0]["recommendation"] = json!("no");
        bad["observations"][0]["rank"] = json!(1);
        assert!(recompute(&bad).is_err());
        let mut bad = v.clone();
        bad["observations"][0]["mentioned"] = json!(false);
        assert!(recompute(&bad).is_err());
        let mut bad = v.clone();
        let mut duplicate = bad["observations"][0].clone();
        duplicate["id"] = json!(D);
        bad["observations"].as_array_mut().unwrap().push(duplicate);
        assert!(recompute(&bad).is_err());
        let mut bad = v.clone();
        bad["products"][0]["facts"]
            .as_array_mut()
            .unwrap()
            .reverse();
        assert!(recompute(&bad).is_err());
        let mut bad = v.clone();
        bad["evidenceMode"] = json!("recorded");
        assert!(recompute(&bad).is_err());
    }
    #[test]
    fn cross_brand_changed_conditions_and_oversize_are_rejected() {
        let c = fixture();
        assert!(prepare(&c, C, &json!({"campaignId":C,"phase":"baseline"})).is_err());
        add(&c, "baseline", 1, "yes", "correct", true);
        c.execute(
            "UPDATE m_observations SET data=json_set(data,'$.modelLabel','changed')",
            [],
        )
        .unwrap();
        assert!(prepare(&c, B, &json!({"campaignId":C,"phase":"baseline"})).is_err());
        c.execute("DELETE FROM m_observations", []).unwrap();
        let mut v = diagnosis(&c);
        v["products"][0]["name"] = json!("x".repeat(400_000));
        assert!(recompute(&v).unwrap_err().contains("400"));
        c.execute(
            "UPDATE m_products SET data=json_set(data,'$.brandId',?1)",
            [C],
        )
        .unwrap();
        assert!(prepare(&c, B, &json!({"campaignId":C,"phase":"baseline"})).is_err());
    }
}
