//! Manual consumer evidence packets; preview is read only and apply is atomic in handle.
use super::*;
fn plan(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    choice(p, "phase", &["baseline", "followup"])?;
    load(c, "m_campaigns", b, &guid(p, "campaignId")?)
}
fn mode(p: &Value) -> &str {
    p["evidenceMode"].as_str().unwrap_or("unspecified")
}
fn slot(v: &Value) -> String {
    json!([v["questionId"], v["platform"], v["repetition"]]).to_string()
}
fn csv(v: &str) -> String {
    let safe = if v.trim_start().starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{v}")
    } else {
        v.into()
    };
    format!("\"{}\"", safe.replace('"', "\"\""))
}
fn occupied(c: &Connection, b: &str, plan: &Value, phase: &Value) -> Result<Vec<Value>> {
    let mut st=c.prepare("SELECT data FROM m_observations WHERE brand_id=?1 AND json_extract(data,'$.assessment.campaignId')=?2").map_err(db_error)?;
    let mut out = vec![];
    for r in st
        .query_map(params![b, plan["id"].as_str()], |r| r.get::<_, String>(0))
        .map_err(db_error)?
    {
        let v = parse_data(r.map_err(db_error)?)?;
        experiments::assessment(c, b, &v, true)?;
        if v["phase"] == *phase {
            out.push(v)
        }
    }
    Ok(out)
}
pub(super) fn prepare(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    let plan = plan(c, b, p)?;
    let occupied = occupied(c, b, &plan, &p["phase"])?;
    let mut rows = vec![];
    let mut lines = vec!["question,platform,repetition,status".to_string()];
    let mut qs = plan["questions"].as_array().ok_or("计划问题损坏")?.clone();
    qs.sort_by_key(|q| q["id"].to_string());
    let mut platforms = plan["platforms"].as_array().ok_or("计划平台损坏")?.clone();
    platforms.sort_by_key(Value::to_string);
    for q in qs {
        for platform in &platforms {
            for rep in 1..=number(&plan, "repetitions")? {
                let row = json!({"questionId":q["id"],"productId":q["productId"],"querySnapshot":q["text"],"platform":platform,"repetition":rep,"testedAt":"","answer":"","mentioned":false,"accuracy":"unknown","citedUrl":"","evidenceNote":"","recommendation":"unknown","rank":null,"competitors":""});
                let done = occupied.iter().any(|v| {
                    v["questionId"] == q["id"]
                        && v["platform"] == *platform
                        && v["assessment"]["repetition"] == rep
                });
                lines.push(format!(
                    "{},{},{},{}",
                    csv(q["text"].as_str().unwrap_or("")),
                    csv(platform.as_str().unwrap_or("")),
                    rep,
                    if done { "completed" } else { "pending" }
                ));
                if !done {
                    rows.push(row)
                }
            }
        }
    }
    let expected = lines.len() - 1;
    if expected > 8000 {
        return Err("采集槽位超过8000".into());
    }
    let remaining = rows.len();
    let evidence_label = match mode(&plan) {
        "recorded" => "人工实测记录",
        "simulated" => "模拟证据：仅验证流程，不代表真实表现",
        _ => "历史性质未分类：尚未核实，不计为人工实测",
    };
    let protocol = json!({
        "format":"m-geo-collection-protocol-v1",
        "brandId":b,
        "campaignId":plan["id"],
        "campaignName":plan["name"],
        "evidenceMode":mode(&plan),
        "evidenceLabel":evidence_label,
        "phase":p["phase"],
        "modelLabel":plan["modelLabel"],
        "searchMode":plan["searchMode"],
        "context":plan["context"],
        "questions":plan["questions"],
        "platforms":plan["platforms"],
        "repetitions":plan["repetitions"],
        "expected":expected,
        "completed":expected-remaining,
        "remaining":remaining,
    });
    let readme = format!(
        "# 本地采集任务 · 尚未发布\n\n实验名称：{}\n\n实验标识：{}\n\n阶段：{}\n\n样本性质：{}\n\n固定模型/版本：{}\n\n固定搜索模式：{}\n\n固定对话条件：{}\n\n任务总数：{}；已完成：{}；待填写：{}。\n\n保持固定条件，填写实际时间与原始回答；空白行不会导入。只有 collection-NNN.json 是录入模板；protocol.json 始终保留实验协议，包含问题、平台和重复次数，不用于导入。全部完成时没有空白录入模板。任务包不导出已有回答，也不证明公开可达、收录或推荐。\n",
        plan["name"].as_str().unwrap_or(""),
        plan["id"].as_str().unwrap_or(""),
        p["phase"].as_str().unwrap_or(""),
        evidence_label,
        plan["modelLabel"].as_str().unwrap_or(""),
        plan["searchMode"].as_str().unwrap_or(""),
        plan["context"].as_str().unwrap_or(""),
        expected, expected-remaining, remaining,
    );
    let mut files = vec![
        json!({"path":"task-list.csv","content":lines.join("\r\n")}),
        json!({"path":"README.md","content":readme}),
    ];
    for (i, batch) in rows.chunks(200).enumerate() {
        files.push(json!({"path":format!("collection-{:03}.json",i+1),"content":serde_json::to_string_pretty(&json!({"format":"m-geo-collection-v1","campaignId":plan["id"],"evidenceMode":mode(&plan),"phase":p["phase"],"modelLabel":plan["modelLabel"],"searchMode":plan["searchMode"],"context":plan["context"],"rows":batch})).map_err(|e|e.to_string())?}));
    }
    files.push(json!({
        "path":"protocol.json",
        "content":serde_json::to_string_pretty(&protocol).map_err(|e| e.to_string())?,
    }));
    if files
        .iter()
        .map(|f| f["content"].as_str().unwrap_or("").len())
        .sum::<usize>()
        > 16 * 1024 * 1024
    {
        return Err("导出超过16MiB".into());
    }
    Ok(
        json!({"summary":{"expected":expected,"completed":expected-remaining,"remaining":remaining,"evidenceMode":mode(&plan)},"files":files,"fileName":"m-geo-collection.zip"}),
    )
}
fn validated(c: &Connection, b: &str, p: &Value) -> Result<(Value, Vec<Value>)> {
    let plan = plan(c, b, p)?;
    let content = p["content"].as_str().ok_or("请提供JSON")?;
    if content.len() > 2 * 1024 * 1024 {
        return Err("JSON超过2MiB".into());
    }
    let packet: Value = serde_json::from_str(content).map_err(|_| "JSON格式错误")?;
    imports::keys(
        &packet,
        &[
            "format",
            "campaignId",
            "evidenceMode",
            "phase",
            "modelLabel",
            "searchMode",
            "context",
            "rows",
        ],
    )?;
    if packet["format"] != "m-geo-collection-v1"
        || packet["campaignId"] != plan["id"]
        || packet["phase"] != p["phase"]
        || packet["evidenceMode"] != mode(&plan)
        || ["modelLabel", "searchMode", "context"]
            .iter()
            .any(|k| packet[k] != plan[k])
    {
        return Err("采集包固定条件或证据模式不一致".into());
    }
    let rows = packet["rows"].as_array().ok_or("缺少采集行")?;
    if rows.is_empty() || rows.len() > 200 {
        return Err("采集行须为1至200行".into());
    }
    let mut seen = HashSet::new();
    let mut statuses = vec![];
    let mut ready = vec![];
    for (i, row) in rows.iter().enumerate() {
        let result = (|| -> Result<Option<Value>> {
            imports::keys(
                row,
                &[
                    "questionId",
                    "productId",
                    "querySnapshot",
                    "platform",
                    "repetition",
                    "testedAt",
                    "answer",
                    "mentioned",
                    "accuracy",
                    "citedUrl",
                    "evidenceNote",
                    "recommendation",
                    "rank",
                    "competitors",
                ],
            )?;
            if !seen.insert(slot(row)) {
                return Err("采集包槽位重复".into());
            }
            let q = plan["questions"]
                .as_array()
                .ok_or("计划损坏")?
                .iter()
                .find(|q| {
                    q["id"] == row["questionId"]
                        && q["productId"] == row["productId"]
                        && q["text"] == row["querySnapshot"]
                })
                .ok_or("问题原文或归属与计划不一致")?;
            load(
                c,
                "m_products",
                b,
                q["productId"].as_str().ok_or("商品缺失")?,
            )?;
            let mut payload = row.clone();
            payload["surface"] = json!("consumer");
            for key in ["phase", "modelLabel", "searchMode"] {
                payload[key] = packet[key].clone()
            }
            payload["assessment"] = json!({"campaignId":plan["id"],"repetition":row["repetition"],"context":plan["context"],"recommendation":row["recommendation"],"rank":row["rank"],"competitors":row["competitors"]});
            let mut pending = payload.clone();
            pending["testedAt"] = json!("2026-01-01T00:00:00Z");
            pending["answer"] = json!("pending");
            let check = observation_value(c, b, &pending)?;
            if check["querySnapshot"] != row["querySnapshot"] {
                return Err("问题原文已变化".into());
            }
            experiments::check_slot(c, b, &check)?;
            let blank = [
                "testedAt",
                "answer",
                "citedUrl",
                "evidenceNote",
                "competitors",
            ]
            .iter()
            .all(|k| row[k] == "")
                && row["mentioned"] == false
                && row["accuracy"] == "unknown"
                && row["recommendation"] == "unknown"
                && row["rank"].is_null()
                && row.get("rank").is_some();
            if blank {
                return Ok(None);
            }
            let v = observation_value(c, b, &payload)?;
            Ok(Some(v))
        })();
        match result {
            Ok(Some(v)) => {
                ready.push(v);
                statuses.push(json!({"row":i+1,"status":"ready"}))
            }
            Ok(None) => statuses.push(json!({"row":i+1,"status":"skipped"})),
            Err(e) => statuses.push(json!({"row":i+1,"status":"error","error":e})),
        }
    }
    let skipped = statuses.iter().filter(|v| v["status"] == "skipped").count();
    let can_apply = !ready.is_empty() && statuses.iter().all(|v| v["status"] != "error");
    let occupied = occupied(c, b, &plan, &p["phase"])?;
    let mut state = vec![];
    for q in plan["questions"].as_array().ok_or("计划问题损坏")? {
        state.push(json!([
            load(c, "m_questions", b, q["id"].as_str().ok_or("问题缺失")?)?,
            load(
                c,
                "m_products",
                b,
                q["productId"].as_str().ok_or("商品缺失")?
            )?
        ]));
    }
    let token = fingerprint(&json!([plan, ready, occupied, state]));
    Ok((
        json!({"canApply":can_apply,"token":token,"readyCount":ready.len(),"skippedCount":skipped,"rows":statuses}),
        ready,
    ))
}
pub(super) fn preview(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    Ok(validated(c, b, p)?.0)
}
pub(super) fn apply(c: &Connection, b: &str, p: &Value) -> Result<Value> {
    let (preview, rows) = validated(c, b, p)?;
    if p["confirm"] != true || preview["canApply"] != true || p["token"] != preview["token"] {
        return Err("预览已失效或尚未确认，请重新预览".into());
    }
    for row in &rows {
        experiments::check_slot(c, b, row)?;
    }
    for row in &rows {
        add_observation(c, b, row)?;
    }
    Ok(json!({"created":rows.len(),"skipped":preview["skippedCount"]}))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Connection, String, Value, Value) {
        let mut c = Connection::open_in_memory().unwrap();
        initialize(&mut c).unwrap();
        let br = handle(&mut c, "brand.create", json!({"name":"虚构品牌"})).unwrap();
        let b = br["id"].as_str().unwrap().to_string();
        let p=save_product(&c,&b,&json!({"name":"虚构商品","sku":"SKU","sourceText":"RAW_SECRET","privateNotes":"PRIVATE_SECRET","facts":[{"id":"public","label":"材质","value":"棉","source":"虚构官方资料","visibility":"public","validUntil":null}]})).unwrap();
        let pid = p["id"].as_str().unwrap();
        handle(
            &mut c,
            "product.confirm",
            json!({"brandId":b,"id":pid,"expectedRevision":p["revision"]}),
        )
        .unwrap();
        let q = json!({"id":id(),"brandId":b,"productId":pid,"text":"=如何选择合适的产品？","confirmed":true,"branded":false,"origin":"customer","source":"虚构验收","intent":"selection"});
        c.execute(
            "INSERT INTO m_questions(id,brand_id,product_id,normalized,data)VALUES(?1,?2,?3,'fixture',?4)",
            params![q["id"].as_str(), b, pid, q.to_string()],
        )
        .unwrap();
        let campaign=experiments::create(&c,&b,&json!({"name":"虚构采集","questionIds":[q["id"]],"platforms":["doubao","yuanbao"],"repetitions":2,"modelLabel":"fixture","searchMode":"off","context":"新对话","evidenceMode":"simulated"})).unwrap();
        let payload = json!({"campaignId":campaign["id"],"phase":"baseline"});
        let exported = prepare(&c, &b, &payload).unwrap();
        let packet: Value =
            serde_json::from_str(exported["files"][2]["content"].as_str().unwrap()).unwrap();
        (c, b, payload, packet)
    }
    fn input(p: &Value, packet: &Value) -> Value {
        let mut v = p.clone();
        v["content"] = json!(packet.to_string());
        v
    }
    fn fill(packet: &mut Value) {
        packet["rows"][0]["testedAt"] = json!("2026-10-23T01:00:00Z");
        packet["rows"][0]["answer"] = json!("  原始\"引号\"\n第二行  ");
    }
    #[test]
    fn collection_packet_pending_and_privacy() {
        let (c, b, p, packet) = fixture();
        let out = prepare(&c, &b, &p).unwrap();
        assert_eq!(out["summary"]["expected"], 4);
        assert!(!out.to_string().contains("SECRET"));
        assert!(out["files"][0]["content"]
            .as_str()
            .unwrap()
            .contains("'=如何"));
        let preview = preview(&c, &b, &input(&p, &packet)).unwrap();
        assert_eq!(preview["canApply"], false);
        assert_eq!(preview["skippedCount"], 4);
        assert!(prepare(&c, &id(), &p).is_err());
    }
    #[test]
    fn collection_frozen_and_row_validation() {
        let (c, b, p, mut packet) = fixture();
        fill(&mut packet);
        for key in [
            "campaignId",
            "phase",
            "modelLabel",
            "searchMode",
            "context",
            "evidenceMode",
        ] {
            let mut bad = packet.clone();
            bad[key] = json!("mismatch");
            assert!(preview(&c, &b, &input(&p, &bad)).is_err(), "{key}");
        }
        for (key, value) in [
            ("testedAt", json!("yesterday")),
            ("answer", json!("")),
            ("recommendation", json!("yes")),
            ("querySnapshot", json!("changed")),
            ("platform", json!("api")),
        ] {
            let mut bad = packet.clone();
            bad["rows"][0][key] = value;
            assert_eq!(
                preview(&c, &b, &input(&p, &bad)).unwrap()["canApply"],
                false,
                "{key}"
            );
        }
        let mut duplicate = packet.clone();
        duplicate["rows"][1] = duplicate["rows"][0].clone();
        assert_eq!(
            preview(&c, &b, &input(&p, &duplicate)).unwrap()["canApply"],
            false
        );
    }
    #[test]
    fn collection_atomic_late_invalid_and_replay() {
        let (c, b, p, mut packet) = fixture();
        fill(&mut packet);
        let ready = preview(&c, &b, &input(&p, &packet)).unwrap();
        let mut bad = packet.clone();
        bad["rows"][3]["answer"] = json!("partial");
        let mut request = input(&p, &bad);
        request["confirm"] = json!(true);
        request["token"] = ready["token"].clone();
        assert!(apply(&c, &b, &request).is_err());
        assert_eq!(
            c.query_row("SELECT count(*) FROM m_observations", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        let mut request = input(&p, &packet);
        request["confirm"] = json!(true);
        request["token"] = ready["token"].clone();
        assert_eq!(apply(&c, &b, &request).unwrap()["created"], 1);
        assert!(apply(&c, &b, &request).is_err());
        let raw: String = c
            .query_row("SELECT data FROM m_observations", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            parse_data(raw).unwrap()["answer"],
            packet["rows"][0]["answer"]
        );
        assert_eq!(prepare(&c, &b, &p).unwrap()["summary"]["remaining"], 3);
        assert_eq!(
            preview(&c, &b, &input(&p, &packet)).unwrap()["canApply"],
            false
        );
    }
    #[test]
    fn collection_stale_product_state() {
        let (c, b, p, mut packet) = fixture();
        fill(&mut packet);
        let mut request = input(&p, &packet);
        request["confirm"] = json!(true);
        request["token"] = preview(&c, &b, &request).unwrap()["token"].clone();
        let pid = packet["rows"][0]["productId"].as_str().unwrap();
        let mut prod = load(&c, "m_products", &b, pid).unwrap();
        prod["revision"] = json!(2);
        c.execute(
            "UPDATE m_products SET data=?1 WHERE id=?2",
            params![prod.to_string(), pid],
        )
        .unwrap();
        assert!(apply(&c, &b, &request).is_err());
    }
    #[test]
    fn collection_packet_partition_and_strict_envelope() {
        let (c, b, p, mut packet) = fixture();
        let mut plan = load(&c, "m_campaigns", &b, p["campaignId"].as_str().unwrap()).unwrap();
        let q = plan["questions"][0].clone();
        plan["questions"] = json!((0..100)
            .map(|_| {
                let mut q = q.clone();
                q["id"] = json!(id());
                q
            })
            .collect::<Vec<_>>());
        plan["platforms"] = json!(["doubao", "yuanbao", "qianwen", "baidu"]);
        plan["repetitions"] = json!(10);
        update(&c, "m_campaigns", &plan).unwrap();
        let out = prepare(&c, &b, &p).unwrap();
        assert_eq!(out["summary"]["expected"], 4000);
        assert_eq!(out["files"].as_array().unwrap().len(), 23);
        for file in out["files"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|f| f["path"].as_str().unwrap().starts_with("collection-"))
        {
            let json: Value = serde_json::from_str(file["content"].as_str().unwrap()).unwrap();
            assert_eq!(json["rows"].as_array().unwrap().len(), 200);
            assert!(json["rows"]
                .as_array()
                .unwrap()
                .iter()
                .all(|r| r["testedAt"] == "" && r["answer"] == ""));
        }
        packet["unexpected"] = json!("field");
        assert!(preview(&c, &b, &input(&p, &packet)).is_err());
    }
    #[test]
    fn collection_legacy_mode_blank_duplicates_and_limits() {
        let (c, b, p, mut packet) = fixture();
        let mut duplicate = packet.clone();
        duplicate["rows"][1] = duplicate["rows"][0].clone();
        assert_eq!(
            preview(&c, &b, &input(&p, &duplicate)).unwrap()["canApply"],
            false
        );
        let mut plan = load(&c, "m_campaigns", &b, p["campaignId"].as_str().unwrap()).unwrap();
        plan.as_object_mut().unwrap().remove("evidenceMode");
        update(&c, "m_campaigns", &plan).unwrap();
        assert_eq!(
            prepare(&c, &b, &p).unwrap()["summary"]["evidenceMode"],
            "unspecified"
        );
        assert!(preview(&c, &b, &input(&p, &packet)).is_err());
        packet["evidenceMode"] = json!("unspecified");
        fill(&mut packet);
        assert_eq!(
            preview(&c, &b, &input(&p, &packet)).unwrap()["canApply"],
            true
        );
        let mut huge = p.clone();
        huge["content"] = json!("x".repeat(2 * 1024 * 1024 + 1));
        assert!(preview(&c, &b, &huge).is_err());
        packet["rows"] = json!([]);
        assert!(preview(&c, &b, &input(&p, &packet)).is_err());
    }

    #[test]
    fn collection_exports_protocol_for_completed_simulated_recorded_and_legacy_phases() {
        for (mode_value, mode_key, label) in [
            (
                Some("simulated"),
                "simulated",
                "模拟证据：仅验证流程，不代表真实表现",
            ),
            (Some("recorded"), "recorded", "人工实测记录"),
            (
                None,
                "unspecified",
                "历史性质未分类：尚未核实，不计为人工实测",
            ),
        ] {
            let (c, b, p, mut packet) = fixture();
            let mut plan = load(&c, "m_campaigns", &b, p["campaignId"].as_str().unwrap()).unwrap();
            if let Some(value) = mode_value {
                plan["evidenceMode"] = json!(value);
            } else {
                plan.as_object_mut().unwrap().remove("evidenceMode");
            }
            update(&c, "m_campaigns", &plan).unwrap();
            packet["evidenceMode"] = json!(mode_key);
            for row in packet["rows"].as_array_mut().unwrap() {
                row["testedAt"] = json!("2026-10-23T01:00:00Z");
                row["answer"] = json!("RAW_EVIDENCE_NOT_FOR_TASK_EXPORT");
            }
            let mut request = input(&p, &packet);
            request["token"] = preview(&c, &b, &request).unwrap()["token"].clone();
            request["confirm"] = json!(true);
            assert_eq!(apply(&c, &b, &request).unwrap()["created"], 4);
            let exported = prepare(&c, &b, &p).unwrap();
            assert_eq!(exported["summary"]["remaining"], 0);
            let files = exported["files"].as_array().unwrap();
            assert!(!files
                .iter()
                .any(|f| f["path"].as_str().unwrap().starts_with("collection-")));
            let protocol_file = files
                .iter()
                .find(|f| f["path"] == "protocol.json")
                .expect("completed packet must retain protocol");
            let protocol: Value =
                serde_json::from_str(protocol_file["content"].as_str().unwrap()).unwrap();
            assert_eq!(protocol["campaignId"], plan["id"]);
            assert_eq!(protocol["campaignName"], "虚构采集");
            assert_eq!(protocol["phase"], "baseline");
            assert_eq!(protocol["modelLabel"], "fixture");
            assert_eq!(protocol["searchMode"], "off");
            assert_eq!(protocol["context"], "新对话");
            assert_eq!(protocol["evidenceMode"], mode_key);
            assert_eq!(protocol["evidenceLabel"], label);
            assert_eq!(protocol["repetitions"], 2);
            assert_eq!(protocol["platforms"], json!(["doubao", "yuanbao"]));
            let readme = files.iter().find(|f| f["path"] == "README.md").unwrap()["content"]
                .as_str()
                .unwrap();
            for expected in [
                plan["id"].as_str().unwrap(),
                "虚构采集",
                "baseline",
                "fixture",
                "off",
                "新对话",
                label,
            ] {
                assert!(
                    readme.contains(expected),
                    "missing protocol value: {expected}"
                );
            }
            for secret in [
                "RAW_EVIDENCE_NOT_FOR_TASK_EXPORT",
                "RAW_SECRET",
                "PRIVATE_SECRET",
            ] {
                assert!(!exported.to_string().contains(secret));
            }
        }
    }
}
