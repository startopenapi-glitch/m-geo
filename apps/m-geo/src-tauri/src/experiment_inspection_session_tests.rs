use super::*;
use serde_json::{json, Value};
const B: &str = "11111111-1111-4111-8111-111111111111";
const FOREIGN: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const C: &str = "22222222-2222-4222-8222-222222222222";
fn package(count: usize) -> Value {
    let questions: Vec<_> = (0..3).map(|i| json!({"id":format!("33333333-3333-4333-8333-{:012}",i),"productId":"44444444-4444-4444-8444-444444444444","text":format!("虚构问题{}",i)})).collect();
    let plan = json!({"id":C,"brandId":B,"name":"虚构历史计划","questions":questions,"platforms":["doubao"],"modelLabel":"虚构模型","searchMode":"虚构检索","context":"虚构条件","repetitions":10,"createdAt":"2026-10-24T00:00:00Z","evidenceMode":"simulated"});
    let rows: Vec<_> = (0..count).map(|i| json!({"id":format!("55555555-5555-4555-8555-{:012}",i),"brandId":B,"productId":questions[i/10]["productId"],"questionId":questions[i/10]["id"],"querySnapshot":questions[i/10]["text"],"branded":false,"platform":"doubao","surface":"consumer","phase":"baseline","modelLabel":"虚构模型","searchMode":"虚构检索","testedAt":"2026-10-24T00:00:00Z","answer":"  原文\u{feff}\n<script>inert</script>","mentioned":false,"accuracy":"unknown","citedUrl":"","evidenceNote":"PRIVATE-NOTE","assessment":{"campaignId":C,"repetition":i%10+1,"context":"虚构条件","recommendation":"unknown","rank":null,"competitors":""}})).collect();
    let summary = json!({"campaign":plan,"evidenceMode":"simulated","evidenceLabel":"模拟样本：仅用于工作流演练，不代表真实平台结果","expectedPerPhase":30,"baselineCount":count,"followupCount":0,"missingCount":60-count,"missing":[],"platforms":[],"citationHosts":{}});
    let evidence =
        json!({"format":"m-geo-experiment-evidence-v1","summary":summary,"observations":rows});
    let files = json!({"files":[{"path":"report.md","content":"deliberately inconsistent companion"},{"path":"evidence.json","content":evidence.to_string()},{"path":"observations.csv","content":"deliberately inconsistent companion"}]});
    crate::site_package::decode_experiment_package(
        "fictional.zip",
        &crate::files::site_zip(&files).unwrap(),
    )
    .unwrap()
}
fn install(session: &mut ExperimentInspectionSession, count: usize) -> Value {
    let generation = session.begin_selection();
    session
        .complete_selection(generation, B, package(count))
        .unwrap()
}
#[test]
fn selection_generation_evicts_old_and_rejects_late_completion() {
    let mut session = ExperimentInspectionSession::default();
    let first = install(&mut session, 0);
    let old = first["inspectionId"].as_str().unwrap();
    let slow = session.begin_selection();
    assert!(session.read_page(B, old, 0).is_err());
    let latest = session.begin_selection(); // cancellation leaves no old result
    assert!(session.complete_selection(slow, B, package(0)).is_err());
    assert!(session.complete_selection(latest, B, json!({})).is_err());
    assert!(session.read_page(B, old, 0).is_err());
    assert!(session.complete_selection(slow, B, package(0)).is_err());
    let current = install(&mut session, 0);
    assert!(session.complete_selection(latest, B, package(0)).is_err());
    assert_eq!(
        session
            .read_page(B, current["inspectionId"].as_str().unwrap(), 0)
            .unwrap()["observations"],
        json!([])
    );
}
#[test]
fn page_and_export_bind_ticket_brand_fingerprint() {
    let mut session = ExperimentInspectionSession::default();
    let empty = install(&mut session, 0);
    let id = empty["inspectionId"].as_str().unwrap();
    assert_eq!(
        session.read_page(B, id, 0).unwrap(),
        json!({"inspectionId":id,"page":0,"pageSize":20,"total":0,"observations":[]})
    );
    assert!(session.read_page(B, id, 1).is_err());
    let selected = install(&mut session, 21);
    let id = selected["inspectionId"].as_str().unwrap();
    let fp = selected["report"]["reportFingerprint"].as_str().unwrap();
    let page = session.read_page(B, id, 0).unwrap();
    assert_eq!(page["observations"].as_array().unwrap().len(), 20);
    assert_eq!(page["total"], 21);
    assert_eq!(
        page["observations"][0]["answer"],
        "  原文\u{feff}\n<script>inert</script>"
    );
    assert_eq!(
        session.read_page(B, id, 1).unwrap()["observations"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(session.read_page(B, id, 2).is_err());
    assert!(session.read_page(B, id, usize::MAX).is_err());
    assert!(session.read_page(FOREIGN, id, 0).is_err());
    assert!(session.prepare_export(FOREIGN, id, fp).is_err());
    assert!(session.prepare_export(B, id, &"0".repeat(64)).is_err());
    let export = session.prepare_export(B, id, fp).unwrap();
    assert_eq!(export["files"].as_array().unwrap().len(), 2);
    assert!(!export.to_string().contains("PRIVATE-NOTE"));
    assert!(!export.to_string().contains("inert"));
    session.begin_selection(); // picker replacement after an export was prepared
    assert!(session.prepare_export(B, id, fp).is_err());
    let fresh = ExperimentInspectionSession::default();
    assert!(fresh.read_page(B, id, 0).is_err());
}
#[test]
fn payloads_reject_smuggled_data() {
    let id = "66666666-6666-4666-8666-666666666666";
    for (action, payload) in [
        ("file.inspectExperimentReport", json!({"brandId":B})),
        (
            "file.readExperimentInspectionPage",
            json!({"brandId":B,"inspectionId":id,"page":0}),
        ),
        (
            "file.exportExperimentInspection",
            json!({"brandId":B,"inspectionId":id,"reportFingerprint":"a".repeat(64)}),
        ),
    ] {
        validate_payload(action, &payload).unwrap();
        for key in ["path", "report", "files", "evidence"] {
            let mut forged = payload.clone();
            forged[key] = json!("private");
            assert!(validate_payload(action, &forged).is_err());
        }
    }
    for value in [
        json!(-1),
        json!(0.5),
        json!("0"),
        json!(null),
        json!(true),
        json!(18446744073709551615u64),
    ] {
        assert!(validate_payload(
            "file.readExperimentInspectionPage",
            &json!({"brandId":B,"inspectionId":id,"page":value})
        )
        .is_err());
    }
    for brand in [
        json!(null),
        json!(1),
        json!("11111111111141118111111111111111"),
        json!(FOREIGN.to_uppercase()),
    ] {
        assert!(
            validate_payload("file.inspectExperimentReport", &json!({"brandId":brand})).is_err()
        );
    }
    assert!(validate_payload(
        "file.exportExperimentInspection",
        &json!({"brandId":B,"inspectionId":id,"reportFingerprint":"A".repeat(64)})
    )
    .is_err());
    for payload in [
        json!(null),
        json!([]),
        json!({}),
        json!({"brandId": B, "inspectionId": id}),
        json!({"brandId": B, "inspectionId": 1, "page": 0}),
    ] {
        assert!(validate_payload("file.readExperimentInspectionPage", &payload).is_err());
    }
    assert!(validate_payload("unknown", &json!({})).is_err());
}
