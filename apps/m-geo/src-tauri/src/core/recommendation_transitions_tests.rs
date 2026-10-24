use super::*;
const B: &str = "11111111-1111-4111-8111-111111111111";
const C: &str = "22222222-2222-4222-8222-222222222222";
fn uid(n: usize) -> String {
    format!("00000000-0000-4000-8000-{n:012}")
}
fn fixture() -> Connection {
    let mut c = Connection::open_in_memory().unwrap();
    initialize(&mut c).unwrap();
    let b = json!({"id":B,"name":"虚构品牌"});
    c.execute(
        "INSERT INTO m_brands VALUES(?1,?2,?3)",
        params![B, "虚构品牌", b.to_string()],
    )
    .unwrap();
    for n in 1..=2 {
        let p = json!({"id":uid(n),"brandId":B,"name":format!("商品{n}"),"sku":format!("S{n}")});
        c.execute(
            "INSERT INTO m_products VALUES(?1,?2,?3,?4)",
            params![uid(n), B, format!("S{n}"), p.to_string()],
        )
        .unwrap();
        let q = json!({"id":uid(n+10),"brandId":B,"productId":uid(n),"text":"相同虚构问题"});
        c.execute(
            "INSERT INTO m_questions VALUES(?1,?2,?3,?4,?5)",
            params![uid(n + 10), B, uid(n), "相同虚构问题", q.to_string()],
        )
        .unwrap();
    }
    let plan = json!({"id":C,"brandId":B,"name":"虚构计划","createdAt":"2026-10-24T00:00:00Z","questions":[{"id":uid(11),"productId":uid(1),"text":"相同虚构问题"},{"id":uid(12),"productId":uid(2),"text":"相同虚构问题"}],"platforms":["doubao","yuanbao"],"repetitions":2,"modelLabel":"fixture","searchMode":"off","context":"fixture","evidenceMode":"simulated"});
    c.execute(
        "INSERT INTO m_campaigns VALUES(?1,?2,?3)",
        params![C, B, plan.to_string()],
    )
    .unwrap();
    c
}
fn add(c: &Connection, product: usize, platform: &str, rep: usize, phase: &str, rec: &str) {
    let n = 100
        + product * 100
        + usize::from(platform == "yuanbao") * 40
        + rep * 10
        + usize::from(phase == "followup");
    let v = json!({"id":uid(n),"brandId":B,"productId":uid(product),"questionId":uid(product+10),"querySnapshot":"相同虚构问题","branded":false,"platform":platform,"surface":"consumer","phase":phase,"modelLabel":"fixture","searchMode":"off","testedAt":"2026-10-24T00:00:00Z","answer":"RAW_PRIVATE_ANSWER","mentioned":true,"accuracy":"unknown","citedUrl":"https://example.com/PRIVATE_URL","evidenceNote":"PRIVATE_NOTE","assessment":{"campaignId":C,"repetition":rep,"context":"fixture","recommendation":rec,"rank":null,"competitors":"PRIVATE_COMPETITOR"}});
    c.execute(
        "INSERT INTO m_observations VALUES(?1,?2,?3,?4,?5,?6)",
        params![
            uid(n),
            B,
            uid(product),
            uid(product + 10),
            uid(n),
            v.to_string()
        ],
    )
    .unwrap();
}
fn eight() -> Connection {
    let c = fixture();
    for (p, plat, r, b, f) in [
        (1, "doubao", 1, Some("no"), Some("yes")),
        (1, "doubao", 2, Some("yes"), Some("yes")),
        (1, "yuanbao", 1, Some("yes"), None),
        (1, "yuanbao", 2, None, None),
        (2, "doubao", 1, Some("yes"), Some("no")),
        (2, "doubao", 2, Some("no"), Some("no")),
        (2, "yuanbao", 1, Some("unknown"), Some("yes")),
        (2, "yuanbao", 2, None, Some("yes")),
    ] {
        if let Some(x) = b {
            add(&c, p, plat, r, "baseline", x)
        }
        if let Some(x) = f {
            add(&c, p, plat, r, "followup", x)
        }
    }
    c
}
fn report(c: &mut Connection) -> Value {
    handle(
        c,
        "campaign.transitions",
        json!({"brandId":B,"campaignId":C}),
    )
    .unwrap()
}
fn conservation(m: &Value) {
    let n = |k: &str| m[k].as_u64().unwrap();
    assert_eq!(
        n("total"),
        [
            "gained",
            "lost",
            "retainedYes",
            "retainedNo",
            "unknownPair",
            "baselineOnly",
            "followupOnly",
            "missingBoth"
        ]
        .iter()
        .map(|k| n(k))
        .sum::<u64>()
    );
    assert_eq!(n("paired"), n("assessedPairs") + n("unknownPair"));
    assert_eq!(n("baselineSamples"), n("paired") + n("baselineOnly"));
    assert_eq!(n("followupSamples"), n("paired") + n("followupOnly"));
    assert_eq!(n("baselineRecommended"), n("lost") + n("retainedYes"));
    assert_eq!(n("followupRecommended"), n("gained") + n("retainedYes"));
}
#[test]
fn recommendation_transitions_exact_eight_states_and_product_regression() {
    let mut c = eight();
    let before = c.total_changes();
    let r = report(&mut c);
    assert_eq!(c.total_changes(), before);
    let s = &r["summary"];
    assert_eq!(s["total"], 8);
    assert_eq!(s["baselineSamples"], 6);
    assert_eq!(s["followupSamples"], 6);
    assert_eq!(s["paired"], 5);
    assert_eq!(s["assessedPairs"], 4);
    assert_eq!(s["deltaPercentagePoints"], 0.0);
    conservation(s);
    assert_eq!(r["products"][0]["metrics"]["deltaPercentagePoints"], 50.0);
    assert_eq!(r["products"][1]["metrics"]["deltaPercentagePoints"], -50.0);
    for key in ["products", "questions", "platforms"] {
        let groups = r[key].as_array().unwrap();
        assert_eq!(groups.len(), 2);
        assert_eq!(
            groups
                .iter()
                .map(|g| g["metrics"]["total"].as_u64().unwrap())
                .sum::<u64>(),
            8
        );
        for g in groups {
            conservation(&g["metrics"]);
        }
    }
    let states = r["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["transition"].as_str().unwrap())
        .collect::<HashSet<_>>();
    assert_eq!(states.len(), 8);
    for marker in [
        "RAW_PRIVATE",
        "PRIVATE_NOTE",
        "PRIVATE_COMPETITOR",
        "PRIVATE_URL",
        "answer",
        "evidenceNote",
        "competitors",
        "citedUrl",
    ] {
        assert!(!r.to_string().contains(marker), "{marker}");
    }
    assert!(r["rows"][0]["baseline"]["rank"].is_null());
    assert_eq!(r["rows"][0]["baseline"]["assessmentRevision"], 1);
}
fn guarded(r: &Value) -> Value {
    json!({"brandId":B,"campaignId":C,"reportFingerprint":r["reportFingerprint"]})
}
fn lookup(r: &Value, row: usize) -> Value {
    let mut p = guarded(r);
    for k in ["questionId", "platform", "repetition"] {
        p[k] = r["rows"][row][k].clone();
    }
    p
}
#[test]
fn recommendation_transitions_evidence_export_and_stale_guards() {
    let mut c = eight();
    let r = report(&mut c);
    let changes = c.total_changes();
    let e = handle(&mut c, "campaign.transitionEvidence", lookup(&r, 0)).unwrap();
    assert_eq!(e["baseline"]["answer"], "RAW_PRIVATE_ANSWER");
    let z = handle(&mut c, "campaign.transitionExport", guarded(&r)).unwrap();
    assert_eq!(z["fileName"], "recommendation-transitions.zip");
    assert_eq!(z["files"].as_array().unwrap().len(), 5);
    let parsed: Value = serde_json::from_str(z["files"][0]["content"].as_str().unwrap()).unwrap();
    assert_eq!(parsed, r);
    for f in z["files"].as_array().unwrap() {
        for marker in [
            "RAW_PRIVATE",
            "PRIVATE_NOTE",
            "PRIVATE_COMPETITOR",
            "PRIVATE_URL",
        ] {
            assert!(!f["content"].as_str().unwrap().contains(marker));
        }
    }
    assert_eq!(c.total_changes(), changes);
    let oid = e["baseline"]["id"].clone();
    handle(&mut c,"observation.assess",json!({"brandId":B,"id":oid,"expectedRevision":1,"mentioned":true,"accuracy":"correct","citedUrl":"","evidenceNote":"PRIVATE_CORRECTED","recommendation":"yes","rank":null,"competitors":""})).unwrap();
    assert!(handle(&mut c, "campaign.transitionEvidence", lookup(&r, 0))
        .unwrap_err()
        .contains("重新计算"));
    assert!(handle(&mut c, "campaign.transitionExport", guarded(&r)).is_err());
    let r2 = report(&mut c);
    let e = handle(&mut c, "campaign.transitionEvidence", lookup(&r2, 0)).unwrap();
    assert_eq!(e["baseline"]["answer"], "RAW_PRIVATE_ANSWER");
    assert_eq!(e["baseline"]["assessmentRevision"], 2);
    add(&c, 1, "yuanbao", 2, "baseline", "unknown");
    assert!(handle(&mut c, "campaign.transitionExport", guarded(&r2)).is_err());
    let r3 = report(&mut c);
    c.execute(
        "UPDATE m_products SET data=json_set(data,'$.name','renamed') WHERE id=?1",
        params![uid(1)],
    )
    .unwrap();
    assert!(handle(&mut c, "campaign.transitionExport", guarded(&r3)).is_err());
}
#[test]
fn recommendation_transitions_exact_payload_unknowns_and_safe_text() {
    let mut c = fixture();
    let r = report(&mut c);
    assert!(r["summary"]["deltaPercentagePoints"].is_null());
    add(&c, 1, "doubao", 1, "baseline", "unknown");
    add(&c, 1, "doubao", 1, "followup", "yes");
    let r = report(&mut c);
    assert_eq!(r["summary"]["unknownPair"], 1);
    assert!(r["summary"]["deltaPercentagePoints"].is_null());
    for mut p in [
        json!({"brandId":format!(" {B}"),"campaignId":C}),
        json!({"brandId":B,"campaignId":format!("{C} ")}),
        json!({"brandId":B,"campaignId":C,"extra":true}),
    ] {
        assert!(handle(&mut c, "campaign.transitions", p.clone()).is_err());
        p["brandId"] = json!(B);
    }
    for (k, v) in [
        ("platform", json!("doubao ")),
        ("repetition", json!(1.5)),
        ("repetition", json!(3)),
        ("questionId", json!(uid(99))),
    ] {
        let mut p = lookup(&r, 0);
        p[k] = v;
        assert!(handle(&mut c, "campaign.transitionEvidence", p).is_err());
    }
    c.execute(
        "UPDATE m_products SET data=json_set(data,'$.name',?1,'$.sku',?2) WHERE id=?3",
        params!["=SUM(1)\n<script>x</script>", "\t@S", uid(1)],
    )
    .unwrap();
    let r = report(&mut c);
    let z = handle(&mut c, "campaign.transitionExport", guarded(&r)).unwrap();
    assert!(z["files"][1]["content"]
        .as_str()
        .unwrap()
        .contains("\"'=SUM(1)\n<script>x</script>\""));
    assert!(!z["files"][4]["content"]
        .as_str()
        .unwrap()
        .contains("<script>"));
    c.execute(
        "UPDATE m_campaigns SET data=json_remove(data,'$.evidenceMode')",
        [],
    )
    .unwrap();
    let r = report(&mut c);
    assert_eq!(r["evidenceMode"], "unspecified");
    assert!(r["campaign"].get("evidenceMode").is_none());
}
#[test]
fn recommendation_transitions_determinism_brand_isolation_and_missing_side() {
    let mut c = eight();
    let r = report(&mut c);
    let old = handle(
        &mut c,
        "campaign.report",
        json!({"brandId":B,"campaignId":C}),
    )
    .unwrap();
    c.execute_batch("CREATE TEMP TABLE source_copy AS SELECT * FROM m_observations; DELETE FROM m_observations; INSERT INTO m_observations SELECT * FROM source_copy ORDER BY id DESC; DROP TABLE source_copy;").unwrap();
    assert_eq!(report(&mut c), r);
    let old2 = handle(
        &mut c,
        "campaign.report",
        json!({"brandId":B,"campaignId":C}),
    )
    .unwrap();
    assert_ne!(
        old["files"][1]["content"], old2["files"][1]["content"],
        "legacy raw evidence retains database order"
    );
    c.execute(
        "UPDATE m_products SET data=json_set(data,'$.privateNotes','unrelated')",
        [],
    )
    .unwrap();
    assert_eq!(report(&mut c), r);
    let foreign = uid(999);
    let foreign_brand = json!({"id":foreign,"name":"另一品牌"});
    c.execute(
        "INSERT INTO m_brands VALUES(?1,?2,?3)",
        params![foreign, "另一品牌", foreign_brand.to_string()],
    )
    .unwrap();
    let mut p = json!({"brandId":foreign,"campaignId":C});
    assert!(handle(&mut c, "campaign.transitions", p.clone()).is_err());
    p = lookup(&r, 0);
    p["brandId"] = json!(foreign);
    assert!(handle(&mut c, "campaign.transitionEvidence", p).is_err());
    let rows = r["rows"].as_array().unwrap();
    for state in ["missing_both", "baseline_only", "followup_only"] {
        let i = rows.iter().position(|x| x["transition"] == state).unwrap();
        let e = handle(&mut c, "campaign.transitionEvidence", lookup(&r, i)).unwrap();
        assert_eq!(e["baseline"].is_null(), state != "baseline_only");
        assert_eq!(e["followup"].is_null(), state != "followup_only");
    }
}
#[test]
fn recommendation_transitions_rejects_corruption_and_source_budget() {
    for (path, value) in [
        ("$.assessment.recommendation", json!("yes ")),
        ("$.assessment.repetition", json!(3)),
        ("$.platform", json!("doubao ")),
        ("$.productId", json!(uid(2))),
        ("$.accuracy", json!("other")),
        ("$.id", json!("bad")),
        ("$.assessmentRevision", json!(2)),
    ] {
        let mut c = eight();
        c.execute("UPDATE m_observations SET data=json_set(data,?1,json(?2)) WHERE id=(SELECT id FROM m_observations LIMIT 1)",params![path,value.to_string()]).unwrap();
        assert!(
            handle(
                &mut c,
                "campaign.transitions",
                json!({"brandId":B,"campaignId":C})
            )
            .is_err(),
            "{path}"
        );
    }
    let mut c = eight();
    c.execute("UPDATE m_observations SET data=json_set(data,'$.answer',?1) WHERE id=(SELECT id FROM m_observations LIMIT 1)",params!["x".repeat(4*1024*1024)]).unwrap();
    assert!(handle(
        &mut c,
        "campaign.transitions",
        json!({"brandId":B,"campaignId":C})
    )
    .unwrap_err()
    .contains("预算"));
    let mut c = eight();
    c.execute("INSERT INTO m_observations SELECT ?1,brand_id,product_id,question_id,?1,json_set(data,'$.id',?1) FROM m_observations LIMIT 1",params![uid(999)]).unwrap();
    assert!(handle(
        &mut c,
        "campaign.transitions",
        json!({"brandId":B,"campaignId":C})
    )
    .unwrap_err()
    .contains("重复"));
}
#[test]
fn recommendation_transitions_max_4000_units_and_multiple_questions_per_product() {
    let mut c = fixture();
    let mut plan: Value = c
        .query_row("SELECT data FROM m_campaigns", [], |r| {
            r.get::<_, String>(0)
        })
        .map(|s| serde_json::from_str(&s).unwrap())
        .unwrap();
    plan["questions"] = json!((0..100)
        .map(|n| json!({"id":uid(1000+n),"productId":uid(1),"text":format!("虚构问题{n}")}))
        .collect::<Vec<_>>());
    plan["platforms"] = json!(["doubao", "yuanbao", "qianwen", "baidu"]);
    plan["repetitions"] = json!(10);
    c.execute("UPDATE m_campaigns SET data=?1", params![plan.to_string()])
        .unwrap();
    let changes = c.total_changes();
    let r = report(&mut c);
    assert_eq!(r["summary"]["total"], 4000);
    assert_eq!(r["rows"].as_array().unwrap().len(), 4000);
    assert_eq!(r["summary"]["missingBoth"], 4000);
    assert_eq!(r["products"].as_array().unwrap().len(), 1);
    assert_eq!(r["questions"].as_array().unwrap().len(), 100);
    assert_eq!(r["platforms"].as_array().unwrap().len(), 4);
    assert!(r["summary"]["deltaPercentagePoints"].is_null());
    let z = handle(&mut c, "campaign.transitionExport", guarded(&r)).unwrap();
    assert_eq!(z["files"].as_array().unwrap().len(), 5);
    assert_eq!(c.total_changes(), changes);
    plan["questions"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":uid(1101),"productId":uid(1),"text":"overflow"}));
    c.execute("UPDATE m_campaigns SET data=?1", params![plan.to_string()])
        .unwrap();
    assert!(handle(
        &mut c,
        "campaign.transitions",
        json!({"brandId":B,"campaignId":C})
    )
    .is_err());
}
#[test]
fn recommendation_transitions_supports_full_current_product_name_limit() {
    let mut c = fixture();
    c.execute(
        "UPDATE m_products SET data=json_set(data,'$.name',?1) WHERE id=?2",
        params!["虚".repeat(200), uid(1)],
    )
    .unwrap();
    assert_eq!(report(&mut c)["products"][0]["name"], "虚".repeat(200));
}
#[test]
fn recommendation_transitions_source_budget_includes_labels_and_json_container() {
    let mut c = fixture();
    let mut plan: Value = c
        .query_row("SELECT data FROM m_campaigns", [], |r| {
            r.get::<_, String>(0)
        })
        .map(|s| serde_json::from_str(&s).unwrap())
        .unwrap();
    plan["platforms"] = json!(["doubao", "yuanbao", "qianwen", "baidu"]);
    plan["repetitions"] = json!(10);
    c.execute("UPDATE m_campaigns SET data=?1", params![plan.to_string()])
        .unwrap();
    add(&c, 1, "doubao", 1, "baseline", "no");
    let template: Value = c
        .query_row("SELECT data FROM m_observations", [], |r| {
            r.get::<_, String>(0)
        })
        .map(|s| serde_json::from_str(&s).unwrap())
        .unwrap();
    c.execute("DELETE FROM m_observations", []).unwrap();
    let mut index = 0;
    for product in 1..=2 {
        for platform in ["doubao", "yuanbao", "qianwen", "baidu"] {
            for repetition in 1..=10 {
                for phase in ["baseline", "followup"] {
                    let mut o = template.clone();
                    index += 1;
                    o["id"] = json!(uid(2000 + index));
                    o["productId"] = json!(uid(product));
                    o["questionId"] = json!(uid(product + 10));
                    o["platform"] = json!(platform);
                    o["assessment"]["repetition"] = json!(repetition);
                    o["phase"] = json!(phase);
                    o["answer"] = json!("x".repeat(25000));
                    c.execute(
                        "INSERT INTO m_observations VALUES(?1,?2,?3,?4,?1,?5)",
                        params![
                            o["id"].as_str(),
                            B,
                            uid(product),
                            uid(product + 10),
                            o.to_string()
                        ],
                    )
                    .unwrap();
                }
            }
        }
    }
    let sum: usize = c
        .query_row(
            "SELECT sum(length(CAST(data AS BLOB))) FROM m_observations",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let mut extra = 4 * 1024 * 1024 - 1 - plan.to_string().len() - sum;
    for i in 1..=index {
        let add = extra.min(4000);
        if add == 0 {
            break;
        }
        c.execute(
            "UPDATE m_observations SET data=json_set(data,'$.answer',?1) WHERE id=?2",
            params!["x".repeat(25000 + add), uid(2000 + i)],
        )
        .unwrap();
        extra -= add;
    }
    assert_eq!(extra, 0);
    assert!(handle(
        &mut c,
        "campaign.transitions",
        json!({"brandId":B,"campaignId":C})
    )
    .unwrap_err()
    .contains("预算"));
}
#[test]
fn recommendation_transitions_all_unknown_and_exact_public_shapes() {
    let mut c = fixture();
    add(&c, 1, "doubao", 1, "baseline", "unknown");
    add(&c, 1, "doubao", 1, "followup", "unknown");
    let r = report(&mut c);
    assert_eq!(r["summary"]["unknownPair"], 1);
    assert_eq!(r["summary"]["paired"], 1);
    assert_eq!(r["summary"]["assessedPairs"], 0);
    assert!(r["summary"]["deltaPercentagePoints"].is_null());
    let shape = |v: &Value, keys: &[&str]| {
        assert_eq!(
            v.as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<HashSet<_>>(),
            keys.iter().copied().collect::<HashSet<_>>()
        );
    };
    shape(
        &r,
        &[
            "format",
            "brandId",
            "campaign",
            "evidenceMode",
            "evidenceFingerprint",
            "summary",
            "products",
            "questions",
            "platforms",
            "rows",
            "caveat",
            "reportFingerprint",
        ],
    );
    shape(
        &r["rows"][0],
        &[
            "productId",
            "questionId",
            "platform",
            "repetition",
            "transition",
            "baseline",
            "followup",
        ],
    );
    shape(
        &r["rows"][0]["baseline"],
        &[
            "observationId",
            "assessmentRevision",
            "recommendation",
            "rank",
            "accuracy",
            "mentioned",
            "cited",
        ],
    );
    assert_eq!(r["summary"].as_object().unwrap().len(), 16);
    let z = handle(&mut c, "campaign.transitionExport", guarded(&r)).unwrap();
    assert_eq!(
        z["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["path"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec![
            "transition-report.json",
            "products.csv",
            "questions.csv",
            "pairs.csv",
            "report.md"
        ]
    );
}
#[test]
fn recommendation_transitions_rejects_noncanonical_frozen_question_ids_before_reporting() {
    let mut c = fixture();
    c.execute(
        "UPDATE m_campaigns SET data=json_set(data,'$.questions[0].id',?1)",
        params![uid(11).replace('-', "")],
    )
    .unwrap();
    let p = json!({"brandId":B,"campaignId":C});
    assert!(
        handle(&mut c, "campaign.report", p.clone()).is_ok(),
        "legacy source/output policy stays unchanged"
    );
    let result = handle(&mut c, "campaign.transitions", p);
    assert!(
        result.is_err(),
        "a returned unit ID must satisfy the evidence lookup canonical-ID contract"
    );
}
#[test]
fn recommendation_transitions_rejects_noncanonical_observation_ids_before_reporting() {
    for alternate in [
        uid(210).replace('-', ""),
        format!("{{{}}}", uid(210)),
        format!("urn:uuid:{}", uid(210)),
    ] {
        let mut c = fixture();
        add(&c, 1, "doubao", 1, "baseline", "yes");
        c.execute(
            "UPDATE m_observations SET data=json_set(data,'$.id',?1)",
            params![alternate],
        )
        .unwrap();
        let p = json!({"brandId":B,"campaignId":C});
        assert!(handle(&mut c, "campaign.report", p.clone()).is_ok());
        assert!(
            handle(&mut c, "campaign.transitions", p).is_err(),
            "alternative source ID spelling {alternate} must reject"
        );
    }
}
