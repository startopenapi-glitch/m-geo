//! Local evidence summaries; no causal or real-platform impact inference.
use super::*;
fn cell(v: &str) -> String {
    let risky = v
        .chars()
        .next()
        .is_some_and(|c| c.is_whitespace() || c.is_control() || "=+-@".contains(c));
    format!(
        "\"{}{}\"",
        if risky { "'" } else { "" },
        v.replace('"', "\"\"")
    )
}
fn stats(rows: &[&Value]) -> Value {
    let assessed = rows
        .iter()
        .filter(|v| v["assessment"]["recommendation"] != "unknown")
        .count();
    let yes = rows
        .iter()
        .filter(|v| v["assessment"]["recommendation"] == "yes")
        .count();
    let ranked = rows
        .iter()
        .filter(|v| v["assessment"]["rank"].as_u64().is_some())
        .count();
    let top3 = rows
        .iter()
        .filter(|v| v["assessment"]["rank"].as_u64().is_some_and(|n| n <= 3))
        .count();
    json!({"samples":rows.len(),"assessed":assessed,"recommended":yes,"unknown":rows.len()-assessed,"recommendationRate":if assessed>0 {json!(yes as f64 / assessed as f64)}else{Value::Null},"ranked":ranked,"top3":top3})
}
pub(super) fn load_source(c: &Connection, b: &str, cid: &str) -> Result<(Value, Vec<Value>)> {
    let plan = load(c, "m_campaigns", b, cid)?;
    let qs = plan["questions"].as_array().ok_or("计划问题损坏")?;
    let platforms = plan["platforms"].as_array().ok_or("计划平台损坏")?;
    let reps = number(&plan, "repetitions")?;
    if qs.is_empty()
        || qs.len() > 100
        || platforms.is_empty()
        || platforms.len() > 4
        || !(1..=10).contains(&reps)
    {
        return Err("计划范围无效".into());
    }
    if plan["id"] != cid || plan["brandId"] != b {
        return Err("计划归属不一致".into());
    }
    let mut seen_questions = HashSet::new();
    let mut seen_platforms = HashSet::new();
    for q in qs {
        if !seen_questions.insert(q["id"].to_string())
            || q["id"].as_str().is_none()
            || q["productId"].as_str().is_none()
            || q["text"].as_str().is_none()
        {
            return Err("计划问题无效或重复".into());
        }
    }
    for platform in platforms {
        let name = platform.as_str().ok_or("计划平台无效")?;
        if !["doubao", "yuanbao", "qianwen", "baidu"].contains(&name)
            || !seen_platforms.insert(name)
        {
            return Err("计划平台无效或重复".into());
        }
    }
    for field in ["modelLabel", "searchMode", "context"] {
        text(&plan, field, 500, true)?;
    }
    let expected = qs.len() * platforms.len() * reps as usize;
    let mut st=c.prepare("SELECT data FROM m_observations WHERE brand_id=?1 AND json_extract(data,'$.assessment.campaignId')=?2 LIMIT 8001").map_err(db_error)?;
    let mut observations = vec![];
    // Reserve room for raw JSON, escaped CSV and package serialization.
    let mut input_bytes = plan.to_string().len();
    let mut slots = std::collections::BTreeMap::new();
    for row in st
        .query_map(params![b, cid], |r| r.get::<_, String>(0))
        .map_err(db_error)?
    {
        let raw = row.map_err(db_error)?;
        input_bytes = input_bytes.checked_add(raw.len()).ok_or("报告证据过大")?;
        if input_bytes > 4 * 1024 * 1024 {
            return Err("报告证据超过本地准备预算，请缩小固定测试计划（ZIP 上限16MB）".into());
        }
        let v = parse_data(raw)?;
        if observations.len() >= expected * 2 || v["brandId"] != b {
            return Err("计划样本过多或归属不一致".into());
        }
        experiments::assessment(c, b, &v, true)?;
        let phase = v["phase"].as_str().ok_or("样本阶段缺失")?;
        if !["baseline", "followup"].contains(&phase) {
            return Err("样本阶段无效".into());
        }
        let key = json!([
            v["questionId"],
            v["platform"],
            v["assessment"]["repetition"],
            phase
        ])
        .to_string();
        if slots.insert(key, observations.len()).is_some() {
            return Err("计划样本轮次重复".into());
        }
        observations.push(v);
    }
    Ok((plan, observations))
}
pub(super) fn prepare(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    let cid = guid(p, "campaignId")?;
    let (plan, observations) = load_source(c, b, &cid)?;
    let summary = summarize(&plan, &observations, true)?;
    let csv = csv(&plan, &observations, true)?;
    let evidence = json!({"format":"m-geo-experiment-evidence-v1","summary":summary,"observations":observations});
    let markdown = markdown(&summary, true)?;
    let evidence_text = serde_json::to_string_pretty(&evidence).map_err(|e| e.to_string())?;
    if markdown.len() + csv.len() + evidence_text.len() > 16 * 1024 * 1024 {
        return Err("报告ZIP内容超过16MB".into());
    }
    Ok(
        json!({"summary":summary,"fileName":"experiment-report.zip","files":[{"path":"report.md","content":markdown},{"path":"evidence.json","content":evidence_text},{"path":"observations.csv","content":csv}]}),
    )
}
pub(super) const CAVEAT: &str = "本地计算仅代表已记录样本，不证明真实平台影响或因果改善。未知推荐不计为否；缺失顺位不计为前三之外。推荐变化仅采用两阶段都已核对的配对。所有样本与配对分开列示。";
pub(super) fn mode_label(mode: &str) -> &'static str {
    match mode {
        "simulated" => "模拟样本：仅用于工作流演练，不代表真实平台结果",
        "recorded" => "人工实测记录：由用户录入，需核对原始证据",
        _ => "历史性质未分类：不能作为已确认的真实平台实测",
    }
}
pub(super) fn summarize(plan: &Value, observations: &[Value], current: bool) -> Result<Value> {
    let qs = plan["questions"].as_array().ok_or("计划问题损坏")?;
    let platforms = plan["platforms"].as_array().ok_or("计划平台损坏")?;
    let reps = number(plan, "repetitions")?;
    let expected = qs.len() * platforms.len() * reps as usize;
    let mut slots = std::collections::BTreeMap::new();
    for (i, v) in observations.iter().enumerate() {
        let key = json!([
            v["questionId"],
            v["platform"],
            v["assessment"]["repetition"],
            v["phase"]
        ])
        .to_string();
        if slots.insert(key, i).is_some() {
            return Err("计划样本轮次重复".into());
        }
    }
    let mut missing = vec![];
    let mut cohorts = vec![];
    for platform in platforms {
        let mut pairs = 0;
        let mut assessed = 0;
        let mut byes = 0;
        let mut fyes = 0;
        for q in qs {
            for repetition in 1..=reps {
                let mut pair = vec![];
                for phase in ["baseline", "followup"] {
                    let key = json!([q["id"], platform, repetition, phase]).to_string();
                    if let Some(i) = slots.get(&key) {
                        pair.push(&observations[*i])
                    } else {
                        missing.push(json!({"questionId":q["id"],"questionText":q["text"],"platform":platform,"repetition":repetition,"phase":phase}));
                    }
                }
                if pair.len() == 2 {
                    pairs += 1;
                    if pair
                        .iter()
                        .all(|v| v["assessment"]["recommendation"] != "unknown")
                    {
                        assessed += 1;
                        byes += usize::from(pair[0]["assessment"]["recommendation"] == "yes");
                        fyes += usize::from(pair[1]["assessment"]["recommendation"] == "yes");
                    }
                }
            }
        }
        let baseline = observations
            .iter()
            .filter(|v| v["platform"] == *platform && v["phase"] == "baseline")
            .collect::<Vec<_>>();
        let followup = observations
            .iter()
            .filter(|v| v["platform"] == *platform && v["phase"] == "followup")
            .collect::<Vec<_>>();
        cohorts.push(json!({"platform":platform,"expectedPerPhase":qs.len()*reps as usize,"baseline":stats(&baseline),"followup":stats(&followup),"matchedPairs":pairs,"matchedAssessedPairs":assessed,"matchedBaselineRecommended":byes,"matchedFollowupRecommended":fyes,"unmatchedSamples":baseline.len()+followup.len()-pairs*2,"deltaPercentagePoints":if assessed>0{json!((fyes as f64-byes as f64)*100.0/assessed as f64)}else{Value::Null}}));
    }
    let mut hosts = std::collections::BTreeMap::<String, usize>::new();
    for v in observations {
        if let Some(url) = v["citedUrl"].as_str().and_then(|s| url::Url::parse(s).ok()) {
            if let Some(host) = url.host_str() {
                *hosts.entry(host.to_lowercase()).or_default() += 1;
            }
        }
    }
    let baseline = observations
        .iter()
        .filter(|v| v["phase"] == "baseline")
        .count();
    let mut summary = json!({"campaign":plan,"expectedPerPhase":expected,"baselineCount":baseline,"followupCount":observations.len()-baseline,"missingCount":missing.len(),"missing":missing,"platforms":cohorts,"citationHosts":hosts});
    if current {
        summary["evidenceMode"] = json!(experiments::evidence_mode(plan)?);
        summary["evidenceLabel"] = json!(mode_label(experiments::evidence_mode(plan)?));
    }
    Ok(summary)
}
pub(super) fn csv(plan: &Value, observations: &[Value], current: bool) -> Result<String> {
    let mode = experiments::evidence_mode(plan)?;
    let mut csv = String::from(if current {
        "evidenceMode,id,question,platform,phase,repetition,recommendation,rank,citedUrl,answer\r\n"
    } else {
        "id,question,platform,phase,repetition,recommendation,rank,citedUrl,answer\r\n"
    });
    for v in observations {
        let mut values = vec![
            v["id"].clone(),
            v["querySnapshot"].clone(),
            v["platform"].clone(),
            v["phase"].clone(),
            v["assessment"]["repetition"].clone(),
            v["assessment"]["recommendation"].clone(),
            v["assessment"]["rank"].clone(),
            v["citedUrl"].clone(),
            v["answer"].clone(),
        ];
        if current {
            values.insert(0, json!(mode));
        }
        csv.push_str(
            &values
                .iter()
                .map(|v| {
                    cell(&v.as_str().map(str::to_owned).unwrap_or_else(|| {
                        if v.is_null() {
                            String::new()
                        } else {
                            v.to_string()
                        }
                    }))
                })
                .collect::<Vec<_>>()
                .join(","),
        );
        csv.push_str("\r\n");
    }
    Ok(csv)
}
pub(super) fn markdown_prefix(summary: &Value, current: bool) -> String {
    format!(
        "# 固定实验本地证据报告\n\n{}{CAVEAT}\n\n条件、覆盖和分平台分母：\n\n```json\n",
        if current {
            format!("{}\n\n", summary["evidenceLabel"].as_str().unwrap_or(""))
        } else {
            String::new()
        }
    )
}
pub(super) fn markdown(summary: &Value, current: bool) -> Result<String> {
    Ok(format!(
        "{}{}\n```\n",
        markdown_prefix(summary, current),
        serde_json::to_string_pretty(summary).map_err(|_| "报告序列化失败")?
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn csv_neutralizes_and_preserves() {
        assert_eq!(cell(" =SUM(1)\n\"x\""), "\"' =SUM(1)\n\"\"x\"\"\"");
    }
    #[test]
    fn unknown_and_missing_rank_are_not_negative() {
        let a = json!({"assessment":{"recommendation":"unknown","rank":null}});
        let b = json!({"assessment":{"recommendation":"yes","rank":null}});
        let s = stats(&[&a, &b]);
        assert_eq!(s["assessed"], 1);
        assert_eq!(s["unknown"], 1);
        assert_eq!(s["recommended"], 1);
        assert_eq!(s["ranked"], 0);
    }
}
#[cfg(test)]
mod evidence_tests {
    use super::*;
    const B: &str = "11111111-1111-4111-8111-111111111111";
    const C: &str = "22222222-2222-4222-8222-222222222222";
    fn fixture(reps: usize, questions: usize) -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE TABLE m_campaigns(id TEXT,brand_id TEXT,data TEXT);CREATE TABLE m_observations(brand_id TEXT,data TEXT);").unwrap();
        let qs = (0..questions)
            .map(|i| json!({"id":format!("q{i}"),"productId":"p","text":format!("虚构问题{i}")}))
            .collect::<Vec<_>>();
        let plan = json!({"id":C,"brandId":B,"name":"虚构实验","questions":qs,"platforms":["doubao","yuanbao","qianwen","baidu"],"modelLabel":"fixture","searchMode":"off","context":"fixture","repetitions":reps});
        c.execute(
            "INSERT INTO m_campaigns VALUES(?1,?2,?3)",
            params![C, B, plan.to_string()],
        )
        .unwrap();
        c
    }
    fn add(c: &Connection, q: usize, platform: &str, rep: usize, phase: &str, rec: &str) {
        let v = json!({"id":format!("fixture-{q}-{platform}-{rep}-{phase}"),"brandId":B,"questionId":format!("q{q}"),"productId":"p","querySnapshot":format!("虚构问题{q}"),"platform":platform,"phase":phase,"surface":"consumer","modelLabel":"fixture","searchMode":"off","branded":false,"mentioned":true,"answer":"=虚构回答\n\"证据\"","citedUrl":"https://Evidence.Example/path","assessmentRevision":3,"assessment":{"campaignId":C,"repetition":rep,"context":"fixture","recommendation":rec,"rank":null,"competitors":""}});
        c.execute(
            "INSERT INTO m_observations VALUES(?1,?2)",
            params![B, v.to_string()],
        )
        .unwrap();
    }
    fn report(c: &Connection) -> Value {
        prepare(c, B, &json!({"campaignId":C})).unwrap()
    }
    #[test]
    fn matched_cohorts_preserve_unknown_unmatched_and_evidence() {
        let c = fixture(3, 1);
        add(&c, 0, "doubao", 1, "baseline", "no");
        add(&c, 0, "doubao", 1, "followup", "yes");
        add(&c, 0, "doubao", 2, "followup", "yes");
        add(&c, 0, "yuanbao", 1, "baseline", "unknown");
        add(&c, 0, "yuanbao", 1, "followup", "no");
        let r = report(&c);
        let s = &r["summary"];
        assert_eq!(s["missingCount"], 19);
        let d = &s["platforms"][0];
        assert_eq!(d["matchedAssessedPairs"], 1);
        assert_eq!(d["unmatchedSamples"], 1);
        assert_eq!(d["deltaPercentagePoints"], 100.0);
        assert_eq!(s["platforms"][1]["matchedAssessedPairs"], 0);
        assert!(s["platforms"][1]["deltaPercentagePoints"].is_null());
        assert_eq!(s["citationHosts"]["evidence.example"], 5);
        let evidence: Value =
            serde_json::from_str(r["files"][1]["content"].as_str().unwrap()).unwrap();
        assert_eq!(evidence["observations"][0]["assessmentRevision"], 3);
        assert!(r["files"][2]["content"]
            .as_str()
            .unwrap()
            .contains("'=虚构回答\n\"\"证据\"\""));
        assert!(prepare(&c, "other", &json!({"campaignId":C})).is_err());
    }
    #[test]
    fn rejects_changed_conditions_and_duplicate_slots() {
        let c = fixture(1, 1);
        add(&c, 0, "doubao", 1, "baseline", "yes");
        add(&c, 0, "doubao", 1, "baseline", "no");
        assert!(prepare(&c, B, &json!({"campaignId":C})).is_err());
        c.execute("DELETE FROM m_observations", []).unwrap();
        add(&c, 0, "doubao", 1, "baseline", "yes");
        c.execute(
            "UPDATE m_observations SET data=json_set(data,'$.modelLabel','changed')",
            [],
        )
        .unwrap();
        assert!(prepare(&c, B, &json!({"campaignId":C})).is_err());
    }
    #[test]
    fn unrelated_and_cross_brand_rows_are_excluded_and_overflow_fails() {
        let c = fixture(1, 1);
        add(&c, 0, "doubao", 1, "baseline", "yes");
        c.execute(
            "UPDATE m_observations SET data=json_set(data,'$.assessment.campaignId',NULL)",
            [],
        )
        .unwrap();
        add(&c, 0, "yuanbao", 1, "followup", "yes");
        c.execute("UPDATE m_observations SET brand_id='other' WHERE json_extract(data,'$.platform')='yuanbao'",[]).unwrap();
        assert_eq!(report(&c)["summary"]["baselineCount"], 0);
        assert_eq!(report(&c)["summary"]["followupCount"], 0);
        c.execute("DELETE FROM m_observations", []).unwrap();
        add(&c, 0, "doubao", 1, "baseline", "yes");
        for _ in 0..8 {
            c.execute(
                "INSERT INTO m_observations SELECT brand_id,data FROM m_observations LIMIT 1",
                [],
            )
            .unwrap();
        }
        assert!(prepare(&c, B, &json!({"campaignId":C})).is_err());
    }
    #[test]
    fn oversized_raw_evidence_fails_before_serialization() {
        let c = fixture(1, 1);
        add(&c, 0, "doubao", 1, "baseline", "yes");
        c.execute(
            "UPDATE m_observations SET data=json_set(data,'$.answer',?1)",
            params!["x".repeat(4 * 1024 * 1024)],
        )
        .unwrap();
        assert!(prepare(&c, B, &json!({"campaignId":C}))
            .unwrap_err()
            .contains("预算"));
    }
    #[test]
    fn database_coverage_exceeds_snapshot_limit() {
        let c = fixture(10, 100);
        for q in 0..100 {
            for platform in ["doubao", "yuanbao", "qianwen", "baidu"] {
                for rep in 1..=10 {
                    add(&c, q, platform, rep, "baseline", "unknown");
                    if q < 50 {
                        add(&c, q, platform, rep, "followup", "no");
                    }
                }
            }
        }
        let r = report(&c);
        assert_eq!(r["summary"]["baselineCount"], 4000);
        assert_eq!(r["summary"]["followupCount"], 2000);
        assert_eq!(r["summary"]["missingCount"], 2000);
    }
}
