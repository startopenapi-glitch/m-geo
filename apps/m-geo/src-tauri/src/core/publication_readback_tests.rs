use super::*;
use publication_readback::{content_sha256, fetch_and_verify, page_title, public_ip, visible_text};
use serde_json::json;

fn fixture() -> (Connection, Value, Value) {
    let mut c = Connection::open_in_memory().unwrap();
    initialize(&mut c).unwrap();
    let b = handle(&mut c, "brand.create", json!({"name":"虚构回读品牌"})).unwrap();
    let p = handle(&mut c,"product.save",json!({"brandId":b["id"],"name":"虚构折叠灯","sku":"SIM-LAMP-19","category":"测试","officialUrl":"","sourceText":"","privateNotes":"绝不应进入回读","facts":[{"id":"f","label":"色温","value":"4000K","source":"虚构规格表","visibility":"public","validUntil":null}]})).unwrap();
    handle(
        &mut c,
        "product.confirm",
        json!({"brandId":b["id"],"id":p["id"],"expectedRevision":p["revision"]}),
    )
    .unwrap();
    let d = handle(
        &mut c,
        "draft.generateLocal",
        json!({"brandId":b["id"],"productId":p["id"],"kind":"description"}),
    )
    .unwrap();
    handle(
        &mut c,
        "draft.review",
        json!({"brandId":b["id"],"id":d["id"],"expectedRevision":d["revision"]}),
    )
    .unwrap();
    let publication = handle(&mut c,"publication.add",json!({"brandId":b["id"],"draftId":d["id"],"expectedRevision":d["revision"],"channel":"品牌官网","url":"https://example.com/sim-lamp","publishedAt":"2026-10-24T01:00:00Z","note":"人工登记"})).unwrap();
    (c, b, publication)
}

fn result(publication: &Value) -> Value {
    json!({
        "fetchedAt":"2026-10-24T02:00:00Z",
        "httpStatus":200,
        "finalUrl":"https://example.com/sim-lamp",
        "title":"虚构折叠灯",
        "bodySha256":"a".repeat(64),
        "draftRevision":publication["draftRevision"],
        "matchedContent":true,
        "errorCategory":null,
        "body":"SECRET BODY MUST NOT BE SAVED",
        "privatePath":"/Users/nobody/private.html"
    })
}

fn apply(c: &Connection, b: &Value, action: &str, payload: Value) -> Result<Value> {
    publication_readback::dispatch(c, action, &payload, b["id"].as_str().unwrap())?
        .ok_or_else(|| "action was not handled".into())
}

#[test]
fn record_readback_persists_only_bounded_public_metadata() {
    let (c, b, publication) = fixture();
    let saved = apply(
        &c,
        &b,
        "publication.recordReadback",
        json!({"id":publication["id"],"result":result(&publication)}),
    )
    .unwrap();
    assert_eq!(saved["lifecycle"], "online");
    assert_eq!(saved["readbacks"][0]["httpStatus"], 200);
    assert_eq!(
        saved["readbacks"][0]["draftRevision"],
        publication["draftRevision"]
    );
    assert_eq!(saved["readbacks"][0]["matchedContent"], true);
    assert_eq!(saved["readbacks"][0]["transport"], "https");
    assert!(saved["readbacks"][0].get("body").is_none());
    assert!(saved["readbacks"][0].get("privatePath").is_none());
    assert!(!saved.to_string().contains("SECRET BODY MUST NOT BE SAVED"));
    assert!(!saved.to_string().contains("/Users/nobody"));
}

#[test]
fn readback_distinguishes_changed_content_and_unreachable_and_caps_history() {
    let (c, b, publication) = fixture();
    let mut saved = publication.clone();
    for n in 0..14 {
        let mut r = result(&publication);
        r["fetchedAt"] = json!(format!("2026-10-24T02:{:02}:00Z", n));
        r["matchedContent"] = json!(n % 2 == 0);
        saved = apply(
            &c,
            &b,
            "publication.recordReadback",
            json!({"id":publication["id"],"result":r}),
        )
        .unwrap();
    }
    assert_eq!(saved["lifecycle"], "content_changed");
    assert_eq!(saved["readbacks"].as_array().unwrap().len(), 12);
    let failed = json!({"fetchedAt":"2026-10-24T03:00:00Z","httpStatus":404,"finalUrl":"https://example.com/missing","title":"","bodySha256":null,"draftRevision":publication["draftRevision"],"matchedContent":false,"errorCategory":"http_status"});
    saved = apply(
        &c,
        &b,
        "publication.recordReadback",
        json!({"id":publication["id"],"result":failed}),
    )
    .unwrap();
    assert_eq!(saved["lifecycle"], "unreachable");
    assert_eq!(saved["readbacks"][0]["httpStatus"], 404);
    let mut changed = result(&publication);
    changed["matchedContent"] = json!(false);
    saved = apply(
        &c,
        &b,
        "publication.recordReadback",
        json!({"id":publication["id"],"result":changed}),
    )
    .unwrap();
    assert_eq!(saved["lifecycle"], "content_changed");
}

#[test]
fn withdrawal_requires_confirmation_reason_and_blocks_online_readback() {
    let (c, b, publication) = fixture();
    for payload in [
        json!({"id":publication["id"],"confirm":false,"reason":"虚构页面整改","status":"withdrawing"}),
        json!({"id":publication["id"],"confirm":true,"reason":"","status":"withdrawing"}),
    ] {
        assert!(apply(&c, &b, "publication.withdraw", payload).is_err());
    }
    let mut saved = apply(&c,&b,"publication.withdraw",json!({"id":publication["id"],"confirm":true,"reason":"虚构页面整改","status":"withdrawing"})).unwrap();
    assert_eq!(saved["lifecycle"], "withdrawing");
    assert!(apply(
        &c,
        &b,
        "publication.recordReadback",
        json!({"id":publication["id"],"result":result(&publication)})
    )
    .is_err());
    saved = apply(&c,&b,"publication.withdraw",json!({"id":publication["id"],"confirm":true,"reason":"人工看到页面已撤下，未核查平台收录","status":"withdrawn"})).unwrap();
    assert_eq!(saved["lifecycle"], "withdrawn");
    assert_eq!(saved["withdrawal"]["history"].as_array().unwrap().len(), 1);
    assert!(apply(&c,&b,"publication.withdraw",json!({"id":publication["id"],"confirm":true,"reason":"虚构状态回退","status":"withdrawing"})).is_err());
    saved = apply(&c,&b,"publication.withdraw",json!({"id":publication["id"],"confirm":true,"reason":"后续看到缓存，需要人工核查","status":"withdrawal_unverified"})).unwrap();
    assert_eq!(saved["lifecycle"], "withdrawal_unverified");
    assert!(apply(
        &c,
        &b,
        "publication.recordReadback",
        json!({"id":publication["id"],"result":result(&publication)})
    )
    .is_err());
}

#[test]
fn readback_rejects_stale_revision_invalid_hash_and_other_brand() {
    let (mut c, b, publication) = fixture();
    let other = handle(&mut c, "brand.create", json!({"name":"另一个虚构品牌"})).unwrap();
    assert!(apply(
        &c,
        &other,
        "publication.recordReadback",
        json!({"id":publication["id"],"result":result(&publication)})
    )
    .is_err());
    let mut stale = result(&publication);
    stale["draftRevision"] = json!(99);
    assert!(apply(
        &c,
        &b,
        "publication.recordReadback",
        json!({"id":publication["id"],"result":stale})
    )
    .is_err());
    let mut bad_hash = result(&publication);
    bad_hash["bodySha256"] = json!("not-a-hash");
    assert!(apply(
        &c,
        &b,
        "publication.recordReadback",
        json!({"id":publication["id"],"result":bad_hash})
    )
    .is_err());
    let mut forged = result(&publication);
    forged["httpStatus"] = json!(503);
    forged["errorCategory"] = json!("http_status");
    assert!(apply(
        &c,
        &b,
        "publication.recordReadback",
        json!({"id":publication["id"],"result":forged})
    )
    .is_err());
}

#[test]
fn transport_helpers_are_conservative_and_block_non_public_ips() {
    let html = "<html><head><title>虚构 &amp; 商品</title><style>SECRET</style></head><body><h1>虚构折叠灯</h1><script>PRIVATE</script><p>色温：4000K</p></body></html>";
    let body = visible_text(html);
    assert!(body.contains("虚构折叠灯 色温：4000K"));
    assert!(!body.contains("SECRET"));
    assert!(!body.contains("PRIVATE"));
    assert_eq!(page_title(html), "虚构 & 商品");
    assert_eq!(
        content_sha256("abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    for ip in [
        "127.0.0.1",
        "10.0.0.1",
        "169.254.1.1",
        "192.168.1.1",
        "100.64.0.1",
        "::1",
        "fc00::1",
        "fe80::1",
        "2001:db8::1",
    ] {
        assert!(!public_ip(ip.parse().unwrap()), "accepted {ip}");
    }
    assert!(public_ip("8.8.8.8".parse().unwrap()));
}

#[tokio::test]
async fn local_urls_are_rejected_without_network_io() {
    for url in [
        "http://127.0.0.1:9/article",
        "http://localhost:9/article",
        "http://[::1]:9/article",
        "https://user:pass@example.com/article",
    ] {
        let publication = json!({"url":url,"draftRevision":1,"contentSnapshot":"# 虚构商品"});
        assert!(
            fetch_and_verify(&publication).await.is_err(),
            "accepted {url}"
        );
    }
}

#[test]
fn dispatch_leaves_unknown_actions_to_core() {
    let (c, b, _) = fixture();
    assert_eq!(
        publication_readback::dispatch(
            &c,
            "publication.add",
            &json!({}),
            b["id"].as_str().unwrap()
        )
        .unwrap(),
        None
    );
}
