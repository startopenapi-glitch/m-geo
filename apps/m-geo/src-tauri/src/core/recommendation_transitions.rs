//! Read-only fixed-plan transitions; raw evidence is available only through guarded local lookup.
use super::*;
use std::collections::BTreeMap;
const FORMAT: &str = "m-geo-recommendation-transitions-v1";
const STALE: &str = "计划证据或当前商品标签已变化，请重新计算推荐变化";
const METRICS: [&str; 16] = [
    "total",
    "baselineSamples",
    "followupSamples",
    "paired",
    "assessedPairs",
    "gained",
    "lost",
    "retainedYes",
    "retainedNo",
    "unknownPair",
    "baselineOnly",
    "followupOnly",
    "missingBoth",
    "baselineRecommended",
    "followupRecommended",
    "deltaPercentagePoints",
];
fn canonical_id(p: &Value, k: &str) -> Result<String> {
    let s = guid(p, k)?;
    if p[k] != s || Uuid::parse_str(&s).map_err(|_| "标识无效")?.to_string() != s {
        return Err(format!("对象标识 {k} 不规范"));
    }
    Ok(s)
}
fn payload(p: &Value, fields: &[&str]) -> Result<String> {
    experiment_evidence::shape(p, fields, &[])?;
    canonical_id(p, "brandId")?;
    canonical_id(p, "campaignId")
}
fn source(c: &Connection, b: &str, cid: &str) -> Result<(Value, Vec<Value>, Vec<Value>)> {
    let (plan, observations) = reports::load_source(c, b, cid)?;
    let expected = experiment_evidence::plan(&plan, b)?;
    experiment_evidence::observations(&plan, &observations, expected)?;
    // Portable historical validation accepts alternative UUID spellings. Transition
    // units and guarded lookups share the stricter canonical identity contract.
    for key in ["id", "brandId"] {
        canonical_id(&plan, key)?;
    }
    for observation in &observations {
        for key in ["id", "brandId", "productId", "questionId"] {
            canonical_id(observation, key)?;
        }
        canonical_id(&observation["assessment"], "campaignId")?;
        if let Some(history) = observation["assessmentHistory"].as_array() {
            for previous in history {
                canonical_id(&previous["assessment"], "campaignId")?;
            }
        }
    }
    let mut products = BTreeMap::new();
    for q in plan["questions"].as_array().ok_or("问题列表无效")? {
        canonical_id(q, "id")?;
        let pid = canonical_id(q, "productId")?;
        if products.contains_key(&pid) {
            continue;
        }
        let prod = load(c, "m_products", b, &pid)?;
        if prod["id"] != pid || prod["brandId"] != b {
            return Err("商品标签归属错误".into());
        }
        let name = raw(&prod, "name", 200, true)?;
        let sku = raw(&prod, "sku", 100, true)?;
        products.insert(pid.clone(), json!({"productId":pid,"name":name,"sku":sku}));
    }
    Ok((plan, observations, products.into_values().collect()))
}
fn side(o: Option<&Value>) -> Value {
    match o {
        None => Value::Null,
        Some(o) => {
            json!({"observationId":o["id"],"assessmentRevision":o.get("assessmentRevision").cloned().unwrap_or(json!(1)),"recommendation":o["assessment"]["recommendation"],"rank":o["assessment"]["rank"],"accuracy":o["accuracy"],"mentioned":o["mentioned"],"cited":o["citedUrl"].as_str().is_some_and(|s|!s.is_empty())})
        }
    }
}
fn transition(b: Option<&Value>, f: Option<&Value>) -> &'static str {
    match (b, f) {
        (None, None) => "missing_both",
        (Some(_), None) => "baseline_only",
        (None, Some(_)) => "followup_only",
        (Some(b), Some(f)) => match (
            b["assessment"]["recommendation"].as_str(),
            f["assessment"]["recommendation"].as_str(),
        ) {
            (Some("no"), Some("yes")) => "gained",
            (Some("yes"), Some("no")) => "lost",
            (Some("yes"), Some("yes")) => "retained_yes",
            (Some("no"), Some("no")) => "retained_no",
            _ => "unknown_pair",
        },
    }
}
fn metrics(rows: &[&Value]) -> Value {
    let mut m = json!({});
    for k in &METRICS[..15] {
        m[*k] = json!(0);
    }
    let mut inc = |k: &str| {
        m[k] = json!(m[k].as_u64().unwrap() + 1);
    };
    for r in rows {
        inc("total");
        let b = !r["baseline"].is_null();
        let f = !r["followup"].is_null();
        if b {
            inc("baselineSamples");
        }
        if f {
            inc("followupSamples");
        }
        if b && f {
            inc("paired");
        }
        let k = match r["transition"].as_str().unwrap() {
            "gained" => "gained",
            "lost" => "lost",
            "retained_yes" => "retainedYes",
            "retained_no" => "retainedNo",
            "unknown_pair" => "unknownPair",
            "baseline_only" => "baselineOnly",
            "followup_only" => "followupOnly",
            _ => "missingBoth",
        };
        inc(k);
        if ["gained", "lost", "retainedYes", "retainedNo"].contains(&k) {
            inc("assessedPairs");
            if r["baseline"]["recommendation"] == "yes" {
                inc("baselineRecommended");
            }
            if r["followup"]["recommendation"] == "yes" {
                inc("followupRecommended");
            }
        }
    }
    let n = m["assessedPairs"].as_u64().unwrap();
    m["deltaPercentagePoints"] = if n == 0 {
        Value::Null
    } else {
        json!(
            (m["followupRecommended"].as_f64().unwrap()
                - m["baselineRecommended"].as_f64().unwrap())
                * 100.0
                / n as f64
        )
    };
    m
}
fn build(plan: &Value, observations: &[Value], products: &[Value], b: &str) -> Result<Value> {
    let mut hashed = observations.to_vec();
    hashed.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    let evidence_source = json!({"campaign":plan,"observations":hashed,"products":products});
    if evidence_source.to_string().len() > 4 * 1024 * 1024 {
        return Err("推荐变化证据与标签超过4MiB来源预算".into());
    }
    let evidence_hash = fingerprint(&evidence_source);
    let mut slots = BTreeMap::new();
    for o in observations {
        slots.insert(
            (
                o["questionId"].as_str().unwrap(),
                o["platform"].as_str().unwrap(),
                o["assessment"]["repetition"].as_u64().unwrap(),
                o["phase"].as_str().unwrap(),
            ),
            o,
        );
    }
    let mut questions = plan["questions"]
        .as_array()
        .unwrap()
        .iter()
        .collect::<Vec<_>>();
    questions.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    let mut platforms = plan["platforms"]
        .as_array()
        .unwrap()
        .iter()
        .collect::<Vec<_>>();
    platforms.sort_by(|a, b| a.as_str().cmp(&b.as_str()));
    let mut rows = vec![];
    for q in &questions {
        for platform in &platforms {
            for rep in 1..=number(plan, "repetitions")? as u64 {
                let key = (q["id"].as_str().unwrap(), platform.as_str().unwrap(), rep);
                let baseline = slots.get(&(key.0, key.1, key.2, "baseline")).copied();
                let followup = slots.get(&(key.0, key.1, key.2, "followup")).copied();
                rows.push(json!({"productId":q["productId"],"questionId":q["id"],"platform":platform,"repetition":rep,"transition":transition(baseline,followup),"baseline":side(baseline),"followup":side(followup)}));
            }
        }
    }
    let product_groups = products
        .iter()
        .map(|p| {
            let mut g = p.clone();
            g["metrics"] = metrics(
                &rows
                    .iter()
                    .filter(|r| r["productId"] == p["productId"])
                    .collect::<Vec<_>>(),
            );
            g
        })
        .collect::<Vec<_>>();
    let question_groups=questions.iter().map(|q|json!({"productId":q["productId"],"questionId":q["id"],"questionText":q["text"],"metrics":metrics(&rows.iter().filter(|r|r["questionId"]==q["id"]).collect::<Vec<_>>())})).collect::<Vec<_>>();
    let platform_groups=platforms.iter().map(|p|json!({"platform":p,"metrics":metrics(&rows.iter().filter(|r|r["platform"]==**p).collect::<Vec<_>>())})).collect::<Vec<_>>();
    let mut report = json!({"format":FORMAT,"brandId":b,"campaign":plan,"evidenceMode":experiments::evidence_mode(plan)?,"evidenceFingerprint":evidence_hash,"summary":metrics(&rows.iter().collect::<Vec<_>>()),"products":product_groups,"questions":question_groups,"platforms":platform_groups,"rows":rows,"caveat":format!("{} 商品名称与SKU为当前标签；报告含业务标识、固定问题和测试条件，并非匿名资料。",reports::CAVEAT)});
    report["reportFingerprint"] = json!(fingerprint(&report));
    if report.to_string().len() > 8 * 1024 * 1024 {
        return Err("推荐变化报告超过8MiB预算".into());
    }
    Ok(report)
}
pub(super) fn prepare(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    let cid = payload(p, &["brandId", "campaignId"])?;
    let (plan, observations, products) = source(c, b, &cid)?;
    build(&plan, &observations, &products, b)
}
fn current(c: &Connection, b: &str, p: &Value, fields: &[&str]) -> Result<(Value, Vec<Value>)> {
    let cid = payload(p, fields)?;
    let hash = p["reportFingerprint"].as_str().ok_or(STALE)?;
    if hash.len() != 64
        || !hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(STALE.into());
    }
    let (plan, observations, products) = source(c, b, &cid)?;
    let report = build(&plan, &observations, &products, b)?;
    if report["reportFingerprint"] != hash {
        return Err(STALE.into());
    }
    Ok((report, observations))
}
pub(super) fn evidence(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    let (report, observations) = current(
        c,
        b,
        p,
        &[
            "brandId",
            "campaignId",
            "reportFingerprint",
            "questionId",
            "platform",
            "repetition",
        ],
    )?;
    canonical_id(p, "questionId")?;
    let platform = p["platform"].as_str().ok_or("平台无效")?;
    if !["doubao", "yuanbao", "qianwen", "baidu"].contains(&platform) {
        return Err("平台无效".into());
    }
    number(p, "repetition")?;
    let row = report["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| {
            r["questionId"] == p["questionId"]
                && r["platform"] == p["platform"]
                && r["repetition"] == p["repetition"]
        })
        .ok_or("配对不在此固定计划中，请重新计算推荐变化")?;
    let original = |side: &str| {
        observations
            .iter()
            .find(|o| o["id"] == row[side]["observationId"])
            .cloned()
            .unwrap_or(Value::Null)
    };
    Ok(
        json!({"reportFingerprint":report["reportFingerprint"],"row":row,"baseline":original("baseline"),"followup":original("followup")}),
    )
}
fn csv_cell(v: &Value) -> String {
    let s = match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        _ => v.to_string(),
    };
    let risky = s
        .chars()
        .next()
        .is_some_and(|c| c.is_control() || c.is_whitespace() || "=+-@".contains(c));
    format!(
        "\"{}{}\"",
        if risky { "'" } else { "" },
        s.replace('"', "\"\"")
    )
}
fn csv(headers: &[&str], rows: Vec<Vec<Value>>) -> String {
    let mut s = headers.join(",");
    s.push_str("\r\n");
    for row in rows {
        s.push_str(&row.iter().map(csv_cell).collect::<Vec<_>>().join(","));
        s.push_str("\r\n");
    }
    s
}
// An indented code block treats every user-controlled line as literal text, including HTML/fences.
fn md_text(s: &str) -> String {
    s.lines()
        .map(|line| {
            format!(
                "    {}\n",
                line.replace('&', "&amp;")
                    .replace('<', "&lt;")
                    .replace('>', "&gt;")
            )
        })
        .collect()
}
pub(super) fn prepare_export(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    let (report, _) = current(c, b, p, &["brandId", "campaignId", "reportFingerprint"])?;
    let group_csv = |group: &str, identity: &[&str]| {
        let mut headers = identity.to_vec();
        headers.extend_from_slice(&METRICS[..16]);
        let rows = report[group]
            .as_array()
            .unwrap()
            .iter()
            .map(|g| {
                let mut row = identity.iter().map(|k| g[*k].clone()).collect::<Vec<_>>();
                row.extend(METRICS[..16].iter().map(|k| g["metrics"][*k].clone()));
                row
            })
            .collect();
        csv(&headers, rows)
    };
    let side_fields = [
        "observationId",
        "assessmentRevision",
        "recommendation",
        "rank",
        "accuracy",
        "mentioned",
        "cited",
    ];
    let mut headers = vec![
        "productId",
        "questionId",
        "platform",
        "repetition",
        "transition",
    ]
    .into_iter()
    .map(str::to_string)
    .collect::<Vec<_>>();
    for side in ["baseline", "followup"] {
        for field in side_fields {
            headers.push(format!("{side}.{field}"));
        }
    }
    let pairs = csv(
        &headers.iter().map(String::as_str).collect::<Vec<_>>(),
        report["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| {
                let mut row = [
                    "productId",
                    "questionId",
                    "platform",
                    "repetition",
                    "transition",
                ]
                .iter()
                .map(|k| r[*k].clone())
                .collect::<Vec<_>>();
                for side in ["baseline", "followup"] {
                    for field in side_fields {
                        row.push(r[side][field].clone());
                    }
                }
                row
            })
            .collect(),
    );
    let markdown=format!("# 逐商品与问题推荐变化\n\n{}\n\n{}\n\n全计划指标（推荐变化分母为 assessedPairs，两期均有且推荐已核对的配对）：\n\n{}\n当前商品标签：\n\n{}\n固定计划与测试条件：\n\n{}",reports::mode_label(report["evidenceMode"].as_str().unwrap()),report["caveat"].as_str().unwrap(),md_text(&serde_json::to_string_pretty(&report["summary"]).map_err(|e|e.to_string())?),md_text(&serde_json::to_string_pretty(&report["products"]).map_err(|e|e.to_string())?),md_text(&serde_json::to_string_pretty(&report["campaign"]).map_err(|e|e.to_string())?));
    let files = json!([{"path":"transition-report.json","content":serde_json::to_string_pretty(&report).map_err(|e|e.to_string())?},{"path":"products.csv","content":group_csv("products",&["productId","name","sku"])},{"path":"questions.csv","content":group_csv("questions",&["productId","questionId","questionText"])},{"path":"pairs.csv","content":pairs},{"path":"report.md","content":markdown}]);
    let bytes = files.as_array().unwrap().iter().try_fold(0usize, |n, f| {
        n.checked_add(f["content"].as_str().unwrap().len())
            .ok_or("导出内容过大")
    })?;
    if bytes > 16 * 1024 * 1024 {
        return Err("推荐变化导出超过16MiB预算".into());
    }
    Ok(json!({"fileName":"recommendation-transitions.zip","files":files}))
}
