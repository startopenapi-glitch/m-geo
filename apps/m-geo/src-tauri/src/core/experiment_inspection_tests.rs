//! Contract fixtures are canonical stored records, independent of current database state.
use super::*;
const B: &str = "11111111-1111-4111-8111-111111111111";
const C: &str = "22222222-2222-4222-8222-222222222222";
const Q: &str = "33333333-3333-4333-8333-333333333333";
const P: &str = "44444444-4444-4444-8444-444444444444";
const COMMON: &str = "本地计算仅代表已记录样本，不证明真实平台影响或因果改善。未知推荐不计为否；缺失顺位不计为前三之外。推荐变化仅采用两阶段都已核对的配对。所有样本与配对分开列示。";
const LABEL: &str = "模拟样本：仅用于工作流演练，不代表真实平台结果";
fn fixture(legacy: bool) -> (Value, Vec<Value>) {
    let mut plan = json!({"id":C,"brandId":B,"name":"PRIVATE_CAMPAIGN","questions":[{"id":Q,"productId":P,"text":"PRIVATE_QUESTION"}],"platforms":["doubao"],"modelLabel":"fictional","searchMode":"off","context":"PRIVATE_CONTEXT","repetitions":3,"createdAt":"2026-10-23T12:00:00+08:00"});
    if !legacy {
        plan["evidenceMode"] = json!("simulated");
    }
    let observations=[("baseline",1,"unknown"),("followup",1,"yes"),("baseline",2,"no"),("followup",2,"yes"),("baseline",3,"yes")].iter().enumerate().map(|(i,(phase,rep,rec))|json!({"id":format!("55555555-5555-4555-8555-{:012}",i),"brandId":B,"productId":P,"questionId":Q,"querySnapshot":"PRIVATE_QUESTION","branded":false,"platform":"doubao","surface":"consumer","phase":phase,"modelLabel":"fictional","searchMode":"off","testedAt":"2026-10-23T12:00:00Z","answer":"PRIVATE_ANSWER","mentioned":true,"accuracy":"unknown","citedUrl":"https://private-host.example/PRIVATE_URL","evidenceNote":"PRIVATE_NOTE","assessment":{"campaignId":C,"repetition":rep,"context":"PRIVATE_CONTEXT","recommendation":rec,"rank":null,"competitors":"PRIVATE_COMPETITOR"}})).collect::<Vec<_>>();
    let mut summary = json!({"campaign":plan,"expectedPerPhase":3,"baselineCount":3,"followupCount":2,"missingCount":1,"missing":[{"questionId":Q,"questionText":"PRIVATE_QUESTION","platform":"doubao","repetition":3,"phase":"followup"}],"platforms":[{"platform":"doubao","expectedPerPhase":3,"baseline":{"samples":3,"assessed":2,"recommended":1,"unknown":1,"recommendationRate":0.5,"ranked":0,"top3":0},"followup":{"samples":2,"assessed":2,"recommended":2,"unknown":0,"recommendationRate":1.0,"ranked":0,"top3":0},"matchedPairs":2,"matchedAssessedPairs":1,"matchedBaselineRecommended":0,"matchedFollowupRecommended":1,"unmatchedSamples":1,"deltaPercentagePoints":100.0}],"citationHosts":{"private-host.example":5}});
    if !legacy {
        summary["evidenceMode"] = json!("simulated");
        summary["evidenceLabel"] = json!(LABEL);
    }
    (summary, observations)
}
// Test-only encoder follows the public old format; expectations above remain hand calculated.
fn package(summary: &Value, observations: &[Value]) -> Value {
    let current = summary.get("evidenceMode").is_some();
    let evidence = json!({"format":"m-geo-experiment-evidence-v1","summary":summary,"observations":observations});
    let markdown = format!(
        "# 固定实验本地证据报告\n\n{}{COMMON}\n\n条件、覆盖和分平台分母：\n\n```json\n{}\n```\n",
        if current {
            format!("{}\n\n", summary["evidenceLabel"].as_str().unwrap())
        } else {
            String::new()
        },
        serde_json::to_string_pretty(summary).unwrap()
    );
    let mut csv = String::from(if current {
        "evidenceMode,id,question,platform,phase,repetition,recommendation,rank,citedUrl,answer\r\n"
    } else {
        "id,question,platform,phase,repetition,recommendation,rank,citedUrl,answer\r\n"
    });
    for o in observations {
        let mut cells = vec![
            o["id"].clone(),
            o["querySnapshot"].clone(),
            o["platform"].clone(),
            o["phase"].clone(),
            o["assessment"]["repetition"].clone(),
            o["assessment"]["recommendation"].clone(),
            o["assessment"]["rank"].clone(),
            o["citedUrl"].clone(),
            o["answer"].clone(),
        ];
        if current {
            cells.insert(0, summary["evidenceMode"].clone());
        }
        csv.push_str(
            &cells
                .iter()
                .map(|v| {
                    let s = if v.is_null() {
                        String::new()
                    } else {
                        v.as_str()
                            .map(str::to_owned)
                            .unwrap_or_else(|| v.to_string())
                    };
                    let prefix =
                        if s.chars().next().is_some_and(|c| {
                            c.is_whitespace() || c.is_control() || "=+-@".contains(c)
                        }) {
                            "'"
                        } else {
                            ""
                        };
                    format!("\"{prefix}{}\"", s.replace('"', "\"\""))
                })
                .collect::<Vec<_>>()
                .join(","),
        );
        csv.push_str("\r\n");
    }
    let files = json!([{"path":"report.md","content":markdown},{"path":"evidence.json","content":evidence.to_string()},{"path":"observations.csv","content":csv}]);
    json!({"source":{"fileName":"fixture.zip","sha256":"a".repeat(64),"compressedBytes":1024,"uncompressedBytes":files.as_array().unwrap().iter().map(|f|f["content"].as_str().unwrap().len()).sum::<usize>(),"fileCount":3},"files":files})
}
fn inspected(summary: &Value, rows: &[Value]) -> InspectedExperiment {
    inspect_experiment_report(B, &package(summary, rows)).unwrap()
}
#[test]
fn current_and_legacy_profiles_recompute() {
    for legacy in [false, true] {
        let (s, o) = fixture(legacy);
        let i = inspected(&s, &o);
        assert_eq!(i.report["status"], "passed");
        assert_eq!(
            i.report["profile"],
            if legacy { "legacy-v1" } else { "current-v1" }
        );
        assert_eq!(
            i.report["evidenceMode"],
            if legacy { "unspecified" } else { "simulated" }
        );
        assert_eq!(
            i.report["counts"],
            json!({"observations":5,"expectedPerPhase":3,"baseline":3,"followup":2,"missing":1,"reviewedObservations":0,"historyEntries":0})
        );
        assert_eq!(i.report["platforms"], s["platforms"]);
        assert_eq!(i.observations, o);
        assert!(i.report["checks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["status"] == "passed"));
    }
}
fn corrected(rows: &mut [Value]) {
    let v = &mut rows[0];
    let mut h = json!({"revision":1,"supersededAt":"2026-10-22T09:00:00Z"});
    for k in [
        "mentioned",
        "accuracy",
        "citedUrl",
        "evidenceNote",
        "assessment",
    ] {
        h[k] = v[k].clone();
    }
    v["assessmentHistory"] = json!([h]);
    v["assessmentRevision"] = json!(2);
}
#[test]
fn history_all_versions_validated() {
    let (s, mut rows) = fixture(false);
    corrected(&mut rows);
    let i = inspected(&s, &rows);
    assert_eq!(i.report["counts"]["reviewedObservations"], 1);
    assert_eq!(i.report["counts"]["historyEntries"], 1);
    for (path, value) in [
        ("revision", json!(3)),
        ("context", json!("changed")),
        ("repetition", json!(3)),
        ("rank", json!(2)),
        ("mentioned", json!(false)),
    ] {
        let mut bad = rows.clone();
        let h = &mut bad[0]["assessmentHistory"][0];
        if ["context", "repetition", "rank"].contains(&path) {
            h["assessment"][path] = value;
        } else {
            h[path] = value;
        }
        if path == "mentioned" {
            h["assessment"]["recommendation"] = json!("yes");
        }
        assert!(
            inspect_experiment_report(B, &package(&s, &bad)).is_err(),
            "{path}"
        );
    }
    let mut bad = rows.clone();
    bad[0].as_object_mut().unwrap().remove("assessmentHistory");
    assert!(inspect_experiment_report(B, &package(&s, &bad)).is_err());
    let mut bad = rows.clone();
    bad[0]["assessmentHistory"][0]["assessment"] = Value::Null;
    assert!(inspect_experiment_report(B, &package(&s, &bad)).is_err());
}
#[test]
fn summary_csv_markdown_tampering_is_blocked() {
    let (s, o) = fixture(false);
    for (index, code) in [(0, "markdown"), (1, "summary"), (2, "csv")] {
        let mut p = package(&s, &o);
        if index == 1 {
            let mut e: Value =
                serde_json::from_str(p["files"][1]["content"].as_str().unwrap()).unwrap();
            e["summary"]["missingCount"] = json!(0);
            p["files"][0]["content"] = package(&e["summary"], &o)["files"][0]["content"].clone();
            p["files"][1]["content"] = json!(e.to_string());
        } else {
            let t = p["files"][index]["content"].as_str().unwrap().to_owned();
            p["files"][index]["content"] = json!(format!("{t}tampered"));
        }
        let i = inspect_experiment_report(B, &p).unwrap();
        assert_eq!(i.report["status"], "blocked");
        assert_eq!(i.report["counts"]["missing"], 1);
        assert_eq!(
            i.report["checks"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["code"] == code)
                .unwrap()["status"],
            "failed"
        );
        assert!(experiment_inspection_export(&i.report).is_ok());
        assert!(i.report["checks"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| c["code"] != code)
            .all(|c| c["status"] == "passed"));
    }
}
#[test]
fn strict_json_and_brand_boundary() {
    let (s, o) = fixture(false);
    let p = package(&s, &o);
    let wrong = "99999999-9999-4999-8999-999999999999";
    let error = inspect_experiment_report(wrong, &p).err().unwrap();
    assert!(!error.contains("PRIVATE"));
    for marker in ["\"mentioned\":true", "\"rank\":null"] {
        let mut p = p.clone();
        let original = p["files"][1]["content"].as_str().unwrap();
        let duplicate = format!("{marker},{marker}");
        p["files"][1]["content"] = json!(original.replacen(marker, &duplicate, 1));
        assert!(inspect_experiment_report(B, &p).is_err());
    }
    for field in ["unexpected", "competitors"] {
        let mut rows = o.clone();
        if field == "competitors" {
            rows[0]["assessment"].as_object_mut().unwrap().remove(field);
        } else {
            rows[0][field] = json!("PRIVATE");
        }
        assert!(inspect_experiment_report(B, &package(&s, &rows)).is_err());
    }
    let mut p = p.clone();
    let e = p["files"][1]["content"].as_str().unwrap().to_owned();
    p["files"][1]["content"] = json!(format!("{e} {{}}"));
    assert!(inspect_experiment_report(B, &p).is_err());
}
#[test]
fn public_errors_distinguish_brand_evidence_and_limits() {
    let (s, o) = fixture(false);
    let p = package(&s, &o);
    let wrong = "99999999-9999-4999-8999-999999999999";
    let brand_error = "实验包与当前品牌不匹配，请切换到导出时的品牌后重试";
    let evidence_error = "实验包原始证据未通过校验，可能已被修改；请使用未修改的原始导出包";
    let limit_error = "实验包超过处理上限，请减少固定问题、平台、轮次或样本后重新导出";
    assert_eq!(
        inspect_experiment_report(wrong, &p).err().unwrap(),
        brand_error
    );

    let mut invalid_json = p.clone();
    invalid_json["files"][1]["content"] = json!("{");
    let error = inspect_experiment_report(B, &invalid_json).err().unwrap();
    assert!(error.starts_with(evidence_error));
    assert!(error.contains("JSON 无效或含重复字段"));

    let mut too_many = o.clone();
    too_many.extend(o);
    assert_eq!(
        inspect_experiment_report(B, &package(&s, &too_many))
            .err()
            .unwrap(),
        limit_error
    );
    for error in [
        inspect_experiment_report(wrong, &p).err().unwrap(),
        inspect_experiment_report(B, &invalid_json).err().unwrap(),
        inspect_experiment_report(B, &package(&s, &too_many))
            .err()
            .unwrap(),
    ] {
        assert!(!error.contains("PRIVATE"));
    }
}
#[test]
fn raw_unicode_and_csv_preservation() {
    let (s, mut o) = fixture(false);
    for raw in [
        "\u{feff}RAW\n\"quoted\"  ",
        "  =formula\n\"quoted\"  ",
        "'legitimate",
        "+formula",
        "@formula",
    ] {
        o[0]["answer"] = json!(raw);
        let i = inspected(&s, &o);
        assert_eq!(i.report["status"], "passed");
        assert_eq!(i.observations[0]["answer"], raw);
    }
}
#[test]
fn inspection_export_excludes_private_markers() {
    let (s, o) = fixture(false);
    let i = inspected(&s, &o);
    let bytes = experiment_inspection_export(&i.report).unwrap().to_string();
    for marker in [
        "PRIVATE_CAMPAIGN",
        "PRIVATE_QUESTION",
        "PRIVATE_CONTEXT",
        "PRIVATE_ANSWER",
        "PRIVATE_URL",
        "private-host.example",
        "PRIVATE_NOTE",
        "PRIVATE_COMPETITOR",
    ] {
        assert!(!bytes.contains(marker), "{marker}");
    }
    assert!(bytes.contains("inspection.json"));
    assert!(bytes.contains("report.md"));
    let mut forged = i.report.clone();
    forged["private"] = json!("PRIVATE_ANSWER");
    assert!(experiment_inspection_export(&forged).is_err());
}
#[test]
fn domain_profiles_limits_and_empty_history_are_strict() {
    let (s, o) = fixture(false);
    for (path, value) in [
        ("evidenceMode", Value::Null),
        ("evidenceMode", json!("real")),
        ("name", json!("x".repeat(101))),
        ("repetitions", json!(11)),
    ] {
        let mut s = s.clone();
        s["campaign"][path] = value;
        assert!(
            inspect_experiment_report(B, &package(&s, &o)).is_err(),
            "{path}"
        );
    }
    let mut p = package(&s, &o);
    let mut e: Value = serde_json::from_str(p["files"][1]["content"].as_str().unwrap()).unwrap();
    e["summary"]
        .as_object_mut()
        .unwrap()
        .remove("evidenceLabel");
    p["files"][1]["content"] = json!(e.to_string());
    assert!(inspect_experiment_report(B, &p).is_err());
    for (key, value) in [
        ("assessmentRevision", json!(1)),
        ("assessmentHistory", json!([])),
        ("answer", json!("x".repeat(30001))),
        ("evidenceNote", json!("x".repeat(5001))),
        ("citedUrl", json!("http://localhost/secret")),
        ("querySnapshot", json!("changed")),
        ("answer", json!("nul\0text")),
        ("mentioned", json!("true")),
    ] {
        let mut rows = o.clone();
        rows[0][key] = value;
        assert!(
            inspect_experiment_report(B, &package(&s, &rows)).is_err(),
            "{key}"
        );
    }
    let mut rows = o.clone();
    rows.push(o[0].clone());
    assert!(inspect_experiment_report(B, &package(&s, &rows)).is_err());
    let mut rows = o.clone();
    rows[1]["id"] = rows[0]["id"].clone();
    assert!(inspect_experiment_report(B, &package(&s, &rows)).is_err());
}

#[test]
fn markdown_summary_json_is_structural_but_strict() {
    let (s, o) = fixture(false);
    let mut p = package(&s, &o);
    let pretty = serde_json::to_string_pretty(&s).unwrap();
    let compact = s.to_string();
    let markdown = p["files"][0]["content"]
        .as_str()
        .unwrap()
        .replace(&pretty, &format!("  {compact}  "));
    p["files"][0]["content"] = json!(markdown);
    assert_eq!(
        inspect_experiment_report(B, &p).unwrap().report["status"],
        "passed"
    );
    let duplicate = compact.replacen(
        "\"baselineCount\":3",
        "\"baselineCount\":3,\"baselineCount\":3",
        1,
    );
    p["files"][0]["content"] = package(&s, &o)["files"][0]["content"].clone();
    let markdown = p["files"][0]["content"]
        .as_str()
        .unwrap()
        .replace(&pretty, &duplicate);
    p["files"][0]["content"] = json!(markdown);
    let i = inspect_experiment_report(B, &p).unwrap();
    assert_eq!(i.report["checks"][4]["status"], "failed");
    assert_eq!(i.report["checks"][2]["status"], "passed");
}
#[test]
fn optional_campaign_mode_and_pristine_history_keep_legacy_semantics() {
    let (mut s, o) = fixture(false);
    s["campaign"]
        .as_object_mut()
        .unwrap()
        .remove("evidenceMode");
    s["evidenceMode"] = json!("unspecified");
    s["evidenceLabel"] = json!("历史性质未分类：不能作为已确认的真实平台实测");
    let i = inspected(&s, &o);
    assert_eq!(i.report["status"], "passed");
    assert_eq!(i.report["profile"], "current-v1");
    assert_eq!(i.report["evidenceMode"], "unspecified");
    assert!(i.observations[0].get("assessmentRevision").is_none());
    assert!(i.observations[0].get("assessmentHistory").is_none());
    let (mut s, o) = fixture(true);
    s["campaign"]["evidenceMode"] = json!("unspecified");
    assert!(inspect_experiment_report(B, &package(&s, &o)).is_err());
}
fn histories(rows: &mut [Value], count: usize, note: &str, competitors: &str) {
    for v in rows {
        let mut versions = Vec::new();
        for n in 1..=count {
            let mut h = json!({"revision":n,"supersededAt":"2026-10-22T09:00:00Z"});
            for key in [
                "mentioned",
                "accuracy",
                "citedUrl",
                "evidenceNote",
                "assessment",
            ] {
                h[key] = v[key].clone();
            }
            h["evidenceNote"] = json!(note);
            h["assessment"]["competitors"] = json!(competitors);
            versions.push(h);
        }
        v["assessmentHistory"] = json!(versions);
        v["assessmentRevision"] = json!(count + 1);
    }
}
#[test]
fn every_history_version_and_fifty_version_boundary_are_validated() {
    let (s, mut rows) = fixture(false);
    histories(&mut rows[..1], 50, "note", "competitor");
    let i = inspected(&s, &rows);
    assert_eq!(i.report["counts"]["historyEntries"], 50);
    let mut bad = rows.clone();
    bad[0]["assessmentHistory"][29]["mentioned"] = json!(false);
    bad[0]["assessmentHistory"][29]["assessment"]["recommendation"] = json!("yes");
    assert!(inspect_experiment_report(B, &package(&s, &bad)).is_err());
    let mut bad = rows.clone();
    bad[0]["assessmentHistory"][49]["assessment"]["campaignId"] = Value::Null;
    assert!(inspect_experiment_report(B, &package(&s, &bad)).is_err());
    histories(&mut rows[..1], 51, "note", "competitor");
    assert!(inspect_experiment_report(B, &package(&s, &rows)).is_err());
}
#[test]
fn observation_count_accepts_full_coverage_and_rejects_first_excess_before_duplicates() {
    let (mut summary, mut rows) = fixture(false);
    let mut final_slot = rows[4].clone();
    final_slot["id"] = json!("55555555-5555-4555-8555-000000000005");
    final_slot["phase"] = json!("followup");
    rows.push(final_slot);
    summary["followupCount"] = json!(3);
    summary["missingCount"] = json!(0);
    summary["missing"] = json!([]);
    summary["platforms"][0]["followup"]["samples"] = json!(3);
    summary["platforms"][0]["followup"]["assessed"] = json!(3);
    summary["platforms"][0]["followup"]["recommended"] = json!(3);
    summary["platforms"][0]["matchedPairs"] = json!(3);
    summary["platforms"][0]["matchedAssessedPairs"] = json!(2);
    summary["platforms"][0]["matchedBaselineRecommended"] = json!(1);
    summary["platforms"][0]["matchedFollowupRecommended"] = json!(2);
    summary["platforms"][0]["unmatchedSamples"] = json!(0);
    summary["platforms"][0]["deltaPercentagePoints"] = json!(50.0);
    summary["citationHosts"]["private-host.example"] = json!(6);

    let expected = summary["expectedPerPhase"].as_u64().unwrap() as usize;
    assert_eq!(rows.len(), expected * 2);
    let evidence_bytes = summary["campaign"].to_string().len()
        + rows.iter().map(|row| row.to_string().len()).sum::<usize>();
    assert!(evidence_bytes < 4 * 1024 * 1024);
    let inspection = inspected(&summary, &rows);
    assert_eq!(inspection.report["status"], "passed");
    assert_eq!(inspection.observations, rows);
    assert_eq!(inspection.report["counts"]["observations"], 6);
    assert_eq!(inspection.report["counts"]["missing"], 0);

    let mut excess = rows[0].clone();
    excess["id"] = json!("55555555-5555-4555-8555-000000000006");
    rows.push(excess);
    assert_eq!(rows.len(), expected * 2 + 1);
    assert!(inspect_experiment_report(B, &package(&summary, &rows)).is_err());
    // A fully occupied plan has no unused slot. Pin the count error itself so the
    // duplicate-slot guard cannot conceal a regression in the earlier count guard.
    assert_eq!(
        experiment_evidence::observations(&summary["campaign"], &rows, expected).unwrap_err(),
        "样本数超出范围"
    );
}
#[test]
fn resource_budgets_are_independent_of_zip_and_collection_import_limits() {
    let (mut s, o) = fixture(false);
    let mut qs=(0..100).map(|i|json!({"id":format!("77777777-7777-4777-8777-{:012}",i),"productId":P,"text":"问".repeat(600)})).collect::<Vec<_>>();
    s["campaign"]["questions"] = json!(qs);
    let mut remaining = 200000 - s["campaign"].to_string().len();
    for q in &mut qs {
        let n = remaining.min(400);
        let mut t = q["text"].as_str().unwrap().to_owned();
        t.push_str(&"x".repeat(n));
        q["text"] = json!(t);
        remaining -= n;
    }
    assert_eq!(remaining, 0);
    s["campaign"]["questions"] = json!(qs);
    assert_eq!(s["campaign"].to_string().len(), 200000);
    assert!(inspect_experiment_report(B, &package(&s, &[])).is_ok());
    let t = s["campaign"]["questions"][99]["text"]
        .as_str()
        .unwrap()
        .to_owned();
    s["campaign"]["questions"][99]["text"] = json!(format!("{t}x"));
    assert!(inspect_experiment_report(B, &package(&s, &[])).is_err());
    let (s, _) = fixture(false);
    let mut rows = o.clone();
    histories(&mut rows, 50, &"\u{1}".repeat(4500), &"\u{1}".repeat(1800));
    assert!(rows[0].to_string().len() < 2100000);
    let two_size = s["campaign"].to_string().len()
        + rows[..2].iter().map(|v| v.to_string().len()).sum::<usize>();
    assert!(two_size > 2 * 1024 * 1024 && two_size < 4 * 1024 * 1024);
    assert!(inspect_experiment_report(B, &package(&s, &rows[..2])).is_ok());
    assert!(inspect_experiment_report(B, &package(&s, &rows[..3])).is_err());
    let mut row = o[..1].to_vec();
    histories(&mut row, 50, &"\u{1}".repeat(5000), &"\u{1}".repeat(2000));
    assert!(row[0].to_string().len() > 2100000);
    assert!(inspect_experiment_report(B, &package(&s, &row)).is_err());
}
#[test]
fn portable_stored_shape_and_identity_failures_do_not_echo_private_fields() {
    let (s, o) = fixture(false);
    for key in [
        "id",
        "brandId",
        "productId",
        "questionId",
        "querySnapshot",
        "branded",
        "platform",
        "surface",
        "phase",
        "modelLabel",
        "searchMode",
        "testedAt",
        "answer",
        "mentioned",
        "accuracy",
        "citedUrl",
        "evidenceNote",
        "assessment",
    ] {
        let mut rows = o.clone();
        rows[0].as_object_mut().unwrap().remove(key);
        let e = inspect_experiment_report(B, &package(&s, &rows))
            .err()
            .unwrap();
        assert!(!e.contains("PRIVATE"), "{key}");
    }
    for key in [
        "campaignId",
        "repetition",
        "context",
        "recommendation",
        "rank",
        "competitors",
    ] {
        let mut rows = o.clone();
        rows[0]["assessment"].as_object_mut().unwrap().remove(key);
        assert!(
            inspect_experiment_report(B, &package(&s, &rows)).is_err(),
            "{key}"
        );
    }
    let mut rows = o.clone();
    rows[0]["assessment"]["competitors"] = Value::Null;
    assert!(inspect_experiment_report(B, &package(&s, &rows)).is_err());
    let mut p = package(&s, &o);
    p["files"][1]["content"] = json!("x".repeat(16 * 1024 * 1024));
    assert!(inspect_experiment_report(B, &p).is_err());
}
#[test]
fn all_strings_including_untrusted_summary_reject_nul() {
    let (s, o) = fixture(false);
    let mut p = package(&s, &o);
    let mut e: Value = serde_json::from_str(p["files"][1]["content"].as_str().unwrap()).unwrap();
    e["summary"]["missing"][0]["questionText"] = json!("bad\0PRIVATE");
    p["files"][1]["content"] = json!(e.to_string());
    assert!(inspect_experiment_report(B, &p).is_err());
}

#[test]
fn parseable_uuid_and_timestamp_spellings_are_not_rewritten() {
    let (mut s, mut rows) = fixture(false);
    let campaign = "ABCDEFAB-1234-4ABC-8ABC-ABCDEFABCDEF";
    s["campaign"]["id"] = json!(campaign);
    s["campaign"]["createdAt"] = json!("2026-10-23T12:00:00.123456789+08:00");
    for v in &mut rows {
        v["assessment"]["campaignId"] = json!(campaign);
        v["testedAt"] = json!("2026-10-23T12:00:00.123+08:00");
    }
    let i = inspected(&s, &rows);
    assert_eq!(i.report["status"], "passed");
    assert_eq!(i.report["campaignId"], campaign);
    assert_eq!(
        i.observations[0]["testedAt"],
        "2026-10-23T12:00:00.123+08:00"
    );
}
#[test]
fn fingerprint_is_deterministic_and_binds_original_zip_hash() {
    let (s, o) = fixture(false);
    let p = package(&s, &o);
    let first = inspect_experiment_report(B, &p).unwrap();
    let again = inspect_experiment_report(B, &p).unwrap();
    assert_eq!(first.report, again.report);
    let mut different = p;
    different["source"]["sha256"] = json!("b".repeat(64));
    let changed = inspect_experiment_report(B, &different).unwrap();
    assert_ne!(
        first.report["reportFingerprint"],
        changed.report["reportFingerprint"]
    );
    let mut forged = first.report.clone();
    forged["counts"]["missing"] = json!(0);
    assert!(experiment_inspection_export(&forged).is_err());
}
