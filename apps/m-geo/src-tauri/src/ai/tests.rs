use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Default)]
struct MemoryKeys(Mutex<std::collections::HashMap<String, String>>);
impl KeyStore for MemoryKeys {
    fn get(&self, reference: &str) -> Result<Option<String>, String> {
        Ok(self.0.lock().unwrap().get(reference).cloned())
    }
    fn set(&self, reference: &str, key: &str) -> Result<(), String> {
        self.0.lock().unwrap().insert(reference.into(), key.into());
        Ok(())
    }
    fn clear(&self, reference: &str) -> Result<(), String> {
        self.0.lock().unwrap().remove(reference);
        Ok(())
    }
}
fn fake_response(text: &str) -> TransportResponse {
    TransportResponse {
        text: text.into(),
        model: "deepseek-chat".into(),
        provider_request_id: None,
        citations: vec![],
    }
}
struct FakeTransport {
    delay: Duration,
    answer: Result<TransportResponse, TransportError>,
    calls: AtomicUsize,
}
impl Transport for FakeTransport {
    fn complete<'a>(&'a self, request: ModelRequest) -> TransportFuture<'a> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            assert_eq!(request.api_key, "fake-secret");
            assert!(request.url.starts_with("https://"));
            tokio::time::sleep(self.delay).await;
            self.answer.clone()
        })
    }
}
fn fixture(delay_ms: u64) -> (Arc<Mutex<Connection>>, Services, Arc<FakeTransport>, String) {
    let mut db = Connection::open_in_memory().unwrap();
    crate::core::initialize(&mut db).unwrap();
    initialize(&mut db).unwrap();
    let b = crate::core::handle(&mut db, "brand.create", json!({"name":"测试品牌"})).unwrap();
    let keys = Arc::new(MemoryKeys::default());
    keys.set("test-key-ref", "fake-secret").unwrap();
    db.execute("UPDATE ai_settings SET key_ref='test-key-ref'", [])
        .unwrap();
    let transport = Arc::new(FakeTransport {
        delay: Duration::from_millis(delay_ms),
        answer: Ok(fake_response("候选事实")),
        calls: AtomicUsize::new(0),
    });
    (
        Arc::new(Mutex::new(db)),
        Services {
            keys,
            transport: transport.clone(),
        },
        transport,
        b["id"].as_str().unwrap().into(),
    )
}
async fn wait_job(db: Arc<Mutex<Connection>>, services: &Services, brand: &str, id: &str) -> Value {
    for _ in 0..100 {
        let jobs = handle_with(
            db.clone(),
            "ai.jobs",
            json!({"brandId":brand}),
            services.clone(),
        )
        .await
        .unwrap();
        let job = jobs
            .as_array()
            .unwrap()
            .iter()
            .find(|j| j["id"] == id)
            .unwrap()
            .clone();
        if !matches!(job["status"].as_str().unwrap(), "queued" | "running") {
            return job;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("job did not finish");
}
#[test]
fn rejects_insecure_or_credential_bearing_urls() {
    for base in [
        "http://example.com/v1",
        "https://u:password@example.com",
        "https://example.com?api_key=secret",
        "https://example.com/#secret",
        "file:///tmp/model",
    ] {
        assert!(validate_settings(base, "model").is_err(), "accepted {base}");
    }
    assert!(validate_settings("https://api.deepseek.com/v1", "deepseek-chat").is_ok());
}

#[tokio::test]
async fn settings_read_reports_reference_presence_without_accessing_credentials() {
    struct UnavailableKeys;
    impl KeyStore for UnavailableKeys {
        fn get(&self, _: &str) -> Result<Option<String>, String> {
            Err("settings must not read the credential store".into())
        }
        fn set(&self, _: &str, _: &str) -> Result<(), String> {
            panic!("settings must not write credentials")
        }
        fn clear(&self, _: &str) -> Result<(), String> {
            panic!("settings must not clear credentials")
        }
    }
    let (db, mut services, transport, _) = fixture(0);
    services.keys = Arc::new(UnavailableKeys);
    for (reference, configured) in [
        (Some("test-key-ref"), true),
        (None, false),
        (Some(""), false),
    ] {
        db.lock()
            .unwrap()
            .execute("UPDATE ai_settings SET key_ref=?1", [reference])
            .unwrap();
        let value = handle_with(db.clone(), "ai.settings.get", json!({}), services.clone())
            .await
            .unwrap();
        assert_eq!(value["hasKey"], configured);
        assert_eq!(value["keyReferenceConfigured"], configured);
        assert_eq!(value["baseUrl"], "https://api.deepseek.com/v1");
        assert_eq!(value["model"], "deepseek-chat");
        assert!(!value.to_string().contains("test-key-ref"));
    }
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
}
#[test]
fn interrupted_jobs_are_preserved_on_restart() {
    let mut db = Connection::open_in_memory().unwrap();
    initialize(&mut db).unwrap();
    db.execute("INSERT INTO ai_jobs(id,brand_id,kind,status,message,created_at) VALUES('job','brand','extract','running','','2026-01-01T00:00:00Z')",[]).unwrap();
    initialize(&mut db).unwrap();
    let s: String = db
        .query_row("SELECT status FROM ai_jobs WHERE id='job'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(s, "interrupted");
}
#[tokio::test]
async fn explicit_consent_precedes_network_or_job_creation() {
    let (db, s, t, brand) = fixture(0);
    for action in ["ai.extract", "ai.start", "ai.test"] {
        assert!(handle_with(
            db.clone(),
            action,
            json!({"brandId":brand,"text":"原始资料"}),
            s.clone()
        )
        .await
        .is_err());
    }
    assert_eq!(t.calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        handle_with(db, "ai.jobs", json!({"brandId":brand}), s)
            .await
            .unwrap(),
        json!([])
    );
}
#[tokio::test]
async fn extraction_returns_candidates_without_creating_facts_or_exposing_keys() {
    let (db, s, _, brand) = fixture(0);
    let start = handle_with(
        db.clone(),
        "ai.extract",
        json!({"brandId":brand,"text":"选中的公开资料","consent":true}),
        s.clone(),
    )
    .await
    .unwrap();
    let job = wait_job(db.clone(), &s, &brand, start["id"].as_str().unwrap()).await;
    assert_eq!(job["status"], "succeeded");
    assert_eq!(job["resultText"], "候选事实");
    assert!(job["draftId"].is_null());
    let snapshot = crate::core::handle(
        &mut db.lock().unwrap(),
        "workspace.snapshot",
        json!({"brandId":brand}),
    )
    .unwrap();
    assert_eq!(snapshot["products"], json!([]));
    assert!(!job.to_string().contains("fake-secret"));
    assert!(job.get("contextJson").is_none());
    let settings = handle_with(db.clone(), "ai.settings.get", json!({}), s)
        .await
        .unwrap();
    assert_eq!(settings["hasKey"], true);
    assert!(!settings.to_string().contains("fake-secret"));
    let dump: String = db
        .lock()
        .unwrap()
        .query_row(
            "SELECT coalesce(group_concat(context_json),'') FROM ai_jobs",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(!dump.contains("fake-secret"));
}
#[tokio::test]
async fn cancellation_is_idempotent_and_brand_scoped() {
    let (db, s, _, brand) = fixture(200);
    let start = handle_with(
        db.clone(),
        "ai.extract",
        json!({"brandId":brand,"text":"资料","consent":true}),
        s.clone(),
    )
    .await
    .unwrap();
    assert!(handle_with(
        db.clone(),
        "ai.cancel",
        json!({"brandId":"other-brand","id":start["id"]}),
        s.clone()
    )
    .await
    .is_err());
    for _ in 0..2 {
        let j = handle_with(
            db.clone(),
            "ai.cancel",
            json!({"brandId":brand,"id":start["id"]}),
            s.clone(),
        )
        .await
        .unwrap();
        assert_eq!(j["status"], "cancelled");
    }
    tokio::time::sleep(Duration::from_millis(250)).await;
    let job = wait_job(db, &s, &brand, start["id"].as_str().unwrap()).await;
    assert!(job["resultText"].is_null());
    assert_eq!(job["status"], "cancelled");
}
#[tokio::test]
async fn model_changes_invalidate_delayed_results() {
    let (db, s, _, brand) = fixture(100);
    let start = handle_with(
        db.clone(),
        "ai.extract",
        json!({"brandId":brand,"text":"资料","consent":true}),
        s.clone(),
    )
    .await
    .unwrap();
    handle_with(
        db.clone(),
        "ai.settings.save",
        json!({"baseUrl":"https://api.deepseek.com/v1","model":"new-model"}),
        s.clone(),
    )
    .await
    .unwrap();
    let job = wait_job(db, &s, &brand, start["id"].as_str().unwrap()).await;
    assert_eq!(job["status"], "failed");
    assert!(job["resultText"].is_null());
}
#[test]
fn provider_errors_and_responses_do_not_expose_credentials() {
    assert!(!TransportError::Authentication.message().contains("secret"));
    assert!(parse_response(
        br#"{"choices":[{"message":{"content":"fake-secret"}}]}"#,
        "fake-secret"
    )
    .is_err());
    assert!(parse_response(br#"{"error":{"message":"secret"}}"#, "fake-secret").is_err());
    let oversized = json!({"choices":[{"message":{"content":"好".repeat(30_001)}}]});
    assert!(parse_response(&serde_json::to_vec(&oversized).unwrap(), "fake-secret").is_err());
}
#[tokio::test]
async fn failures_are_not_retried_automatically() {
    let (db, mut s, _, brand) = fixture(0);
    let t = Arc::new(FakeTransport {
        delay: Duration::ZERO,
        answer: Err(TransportError::RateLimited),
        calls: AtomicUsize::new(0),
    });
    s.transport = t.clone();
    let start = handle_with(
        db.clone(),
        "ai.extract",
        json!({"brandId":brand,"text":"资料","consent":true}),
        s.clone(),
    )
    .await
    .unwrap();
    let job = wait_job(db, &s, &brand, start["id"].as_str().unwrap()).await;
    assert_eq!(job["status"], "failed");
    assert_eq!(t.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn destination_change_requires_new_credential_and_reset_does_not_discover_orphan_keys() {
    let (db, s, t, _) = fixture(0);
    let result = handle_with(
        db.clone(),
        "ai.settings.save",
        json!({"baseUrl":"https://other-provider.example/v1","model":"m"}),
        s.clone(),
    )
    .await;
    assert!(result.is_err());
    let settings = handle_with(db.clone(), "ai.settings.get", json!({}), s.clone())
        .await
        .unwrap();
    assert_eq!(settings["baseUrl"], "https://api.deepseek.com/v1");
    assert_eq!(t.calls.load(Ordering::SeqCst), 0);
    {
        let mut c = db.lock().unwrap();
        c.execute("DELETE FROM ai_settings", []).unwrap();
        initialize(&mut c).unwrap();
    }
    let settings = handle_with(db, "ai.settings.get", json!({}), s)
        .await
        .unwrap();
    assert_eq!(settings["hasKey"], false);
}

#[tokio::test]
async fn selected_credentials_are_rejected_before_job_persistence() {
    let (db, s, t, brand) = fixture(0);
    let result = handle_with(
        db.clone(),
        "ai.extract",
        json!({"brandId":brand,"text":"复制的 API Key fake-secret","consent":true}),
        s.clone(),
    )
    .await;
    assert!(
        result.is_err(),
        "active model credential must not enter persisted selected text"
    );
    assert_eq!(t.calls.load(Ordering::SeqCst), 0);
    let jobs = handle_with(db, "ai.jobs", json!({"brandId":brand}), s)
        .await
        .unwrap();
    assert_eq!(jobs, json!([]));
}

#[tokio::test]
async fn credential_cannot_be_saved_inside_frontend_visible_settings() {
    let mut connection = Connection::open_in_memory().unwrap();
    initialize(&mut connection).unwrap();
    connection
        .execute("UPDATE ai_settings SET key_ref='test-key-ref'", [])
        .unwrap();
    let keys = Arc::new(MemoryKeys::default());
    keys.set("test-key-ref", "fake-secret").unwrap();
    let db = Arc::new(Mutex::new(connection));
    let s = Services {
        keys,
        transport: Arc::new(FakeTransport {
            delay: Duration::ZERO,
            answer: Ok(fake_response("文本")),
            calls: AtomicUsize::new(0),
        }),
    };
    let result = handle_with(
        db,
        "ai.settings.save",
        json!({"baseUrl":"https://api.deepseek.com/v1","model":"fake-secret"}),
        s,
    )
    .await;
    assert!(
        result.is_err(),
        "model label must not expose configured credential"
    );
}

#[tokio::test]
async fn delayed_product_revision_change_does_not_create_a_draft() {
    let (db, s, _, brand) = fixture(150);
    let product = {
        let mut c = db.lock().unwrap();
        let p=crate::core::handle(&mut c,"product.save",json!({"brandId":brand,"name":"电水壶","sku":"SKU-1","category":"家电","officialUrl":"https://example.com/item","sourceText":"RAW_SECRET","privateNotes":"PRIVATE_SECRET","facts":[{"id":"f1","label":"容量","value":"1.5L","source":"官方参数表","visibility":"public","validUntil":null}]})).unwrap();
        crate::core::handle(
            &mut c,
            "product.confirm",
            json!({"brandId":brand,"id":p["id"],"expectedRevision":p["revision"]}),
        )
        .unwrap()
    };
    let start = handle_with(
        db.clone(),
        "ai.start",
        json!({"brandId":brand,"productId":product["id"],"kind":"description","consent":true}),
        s.clone(),
    )
    .await
    .unwrap();
    {
        let mut c = db.lock().unwrap();
        let mut edit = product.clone();
        edit["expectedRevision"] = product["revision"].clone();
        edit["name"] = json!("新名字");
        crate::core::handle(&mut c, "product.save", edit).unwrap();
    }
    let job = wait_job(db.clone(), &s, &brand, start["id"].as_str().unwrap()).await;
    assert_eq!(job["status"], "failed");
    assert!(job["draftId"].is_null());
    let snapshot = crate::core::handle(
        &mut db.lock().unwrap(),
        "workspace.snapshot",
        json!({"brandId":brand}),
    )
    .unwrap();
    assert_eq!(snapshot["drafts"], json!([]));
}

#[tokio::test]
async fn provider_faults_fail_safely_and_are_never_retried() {
    for fault in [
        TransportError::Authentication,
        TransportError::RateLimited,
        TransportError::Timeout,
        TransportError::Network,
        TransportError::Redirect,
        TransportError::InvalidResponse,
        TransportError::TooLarge,
    ] {
        let (db, mut services, _, brand) = fixture(0);
        let transport = Arc::new(FakeTransport {
            delay: Duration::ZERO,
            answer: Err(fault),
            calls: AtomicUsize::new(0),
        });
        services.transport = transport.clone();
        let start = handle_with(
            db.clone(),
            "ai.extract",
            json!({"brandId":brand,"text":"公开资料","consent":true}),
            services.clone(),
        )
        .await
        .unwrap();
        let result = wait_job(db, &services, &brand, start["id"].as_str().unwrap()).await;
        assert_eq!(result["status"], "failed");
        assert!(result["resultText"].is_null());
        assert!(!result.to_string().contains("fake-secret"));
        assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn one_brands_successful_job_never_appears_in_another_brand() {
    let (db, s, _, brand) = fixture(0);
    let other = crate::core::handle(
        &mut db.lock().unwrap(),
        "brand.create",
        json!({"name":"品牌 B"}),
    )
    .unwrap();
    let start = handle_with(
        db.clone(),
        "ai.extract",
        json!({"brandId":brand,"text":"品牌 A 资料","consent":true}),
        s.clone(),
    )
    .await
    .unwrap();
    let result = wait_job(db.clone(), &s, &brand, start["id"].as_str().unwrap()).await;
    assert_eq!(result["brandId"], brand);
    assert_eq!(
        handle_with(db, "ai.jobs", json!({"brandId":other["id"]}), s)
            .await
            .unwrap(),
        json!([])
    );
}

struct FailingKeys;
impl KeyStore for FailingKeys {
    fn get(&self, _: &str) -> Result<Option<String>, String> {
        Ok(None)
    }
    fn set(&self, _: &str, _: &str) -> Result<(), String> {
        Err("系统凭据存储失败，未保存密钥".into())
    }
    fn clear(&self, _: &str) -> Result<(), String> {
        Err("系统凭据存储失败".into())
    }
}
#[tokio::test]
async fn vault_failure_rolls_back_settings_without_plaintext_fallback() {
    let (db, services, transport, _) = fixture(0);
    let failing = Services {
        keys: Arc::new(FailingKeys),
        transport,
    };
    assert!(handle_with(db.clone(),"ai.settings.save",json!({"baseUrl":"https://other.example/v1","model":"new-model","apiKey":"never-store-this-secret"}),failing).await.is_err());
    let settings = handle_with(db.clone(), "ai.settings.get", json!({}), services)
        .await
        .unwrap();
    assert_eq!(settings["baseUrl"], "https://api.deepseek.com/v1");
    assert_eq!(settings["version"], 1);
    let persisted: String = db
        .lock()
        .unwrap()
        .query_row(
            "SELECT base_url || model || coalesce(key_ref,'') FROM ai_settings",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(!persisted.contains("never-store-this-secret"));
}
#[tokio::test]
async fn saved_key_and_cleared_key_do_not_trigger_requests() {
    let (db, services, transport, _) = fixture(0);
    let settings=handle_with(db.clone(),"ai.settings.save",json!({"baseUrl":"https://new-provider.example/v1","model":"model","apiKey":"new-private-credential"}),services.clone()).await.unwrap();
    assert_eq!(settings["hasKey"], true);
    assert!(!settings.to_string().contains("new-private-credential"));
    let cleared = handle_with(
        db.clone(),
        "ai.settings.clearKey",
        json!({}),
        services.clone(),
    )
    .await
    .unwrap();
    assert_eq!(cleared["hasKey"], false);
    assert_eq!(cleared["version"], 3);
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
    let c = db.lock().unwrap();
    let reference: Option<String> = c
        .query_row("SELECT key_ref FROM ai_settings", [], |r| r.get(0))
        .unwrap();
    assert!(reference.is_none());
}

#[test]
fn token_limited_provider_text_is_not_saved_as_complete_content() {
    let response=br#"{"choices":[{"finish_reason":"length","message":{"content":"a truncated paragraph"}}]}"#;
    assert!(parse_response(response, "fake-secret").is_err());
}
#[tokio::test]
async fn json_escaped_credential_is_also_rejected_before_persisting_selected_text() {
    let (db, s, t, brand) = fixture(0);
    s.keys.set("test-key-ref", "quoted-\"credential\"").unwrap();
    let result = handle_with(
        db.clone(),
        "ai.extract",
        json!({"brandId":brand,"text":"复制 quoted-\"credential\"","consent":true}),
        s.clone(),
    )
    .await;
    assert!(result.is_err());
    assert_eq!(t.calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        handle_with(db, "ai.jobs", json!({"brandId":brand}), s)
            .await
            .unwrap(),
        json!([])
    );
}

#[tokio::test]
async fn successful_generation_creates_only_an_unreviewed_draft_in_its_brand() {
    let (db, s, _, brand) = fixture(0);
    let product = {
        let mut c = db.lock().unwrap();
        let p=crate::core::handle(&mut c,"product.save",json!({"brandId":brand,"name":"电水壶","sku":"SKU","category":"家电","officialUrl":"https://example.com/item","sourceText":"RAW_SECRET","privateNotes":"PRIVATE_SECRET","facts":[{"id":"fact","label":"容量","value":"1.5L","source":"官方","visibility":"public","validUntil":null}]})).unwrap();
        crate::core::handle(
            &mut c,
            "product.confirm",
            json!({"brandId":brand,"id":p["id"],"expectedRevision":p["revision"]}),
        )
        .unwrap()
    };
    let start = handle_with(
        db.clone(),
        "ai.start",
        json!({"brandId":brand,"productId":product["id"],"kind":"description","consent":true}),
        s.clone(),
    )
    .await
    .unwrap();
    let result = wait_job(db.clone(), &s, &brand, start["id"].as_str().unwrap()).await;
    assert_eq!(result["status"], "succeeded");
    assert!(result["draftId"].is_string());
    let snapshot = crate::core::handle(
        &mut db.lock().unwrap(),
        "workspace.snapshot",
        json!({"brandId":brand}),
    )
    .unwrap();
    let drafts = snapshot["drafts"].as_array().unwrap();
    assert_eq!(drafts.len(), 1);
    assert_eq!(drafts[0]["brandId"], brand);
    assert_eq!(drafts[0]["body"], "候选事实");
    assert_eq!(drafts[0]["generationMode"], "ai");
    assert!(drafts[0]["reviewedAt"].is_null());
}
#[tokio::test]
async fn running_job_cancellation_discards_late_transport_result() {
    let (db, s, t, brand) = fixture(200);
    let start = handle_with(
        db.clone(),
        "ai.extract",
        json!({"brandId":brand,"text":"资料","consent":true}),
        s.clone(),
    )
    .await
    .unwrap();
    for _ in 0..100 {
        if t.calls.load(Ordering::SeqCst) > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(t.calls.load(Ordering::SeqCst), 1);
    let result = handle_with(
        db.clone(),
        "ai.cancel",
        json!({"brandId":brand,"id":start["id"]}),
        s.clone(),
    )
    .await
    .unwrap();
    assert_eq!(result["status"], "cancelled");
    tokio::time::sleep(Duration::from_millis(220)).await;
    let final_job = wait_job(db, &s, &brand, start["id"].as_str().unwrap()).await;
    assert_eq!(final_job["status"], "cancelled");
    assert!(final_job["resultText"].is_null());
}

#[tokio::test]
async fn connection_test_result_is_invalidated_by_configuration_change() {
    let (db, services, transport, _) = fixture(120);
    let request_db = db.clone();
    let request_services = services.clone();
    let test = tokio::spawn(async move {
        handle_with(
            request_db,
            "ai.test",
            json!({"consent":true}),
            request_services,
        )
        .await
    });
    for _ in 0..100 {
        if transport.calls.load(Ordering::SeqCst) > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
    handle_with(
        db,
        "ai.settings.save",
        json!({"baseUrl":"https://api.deepseek.com/v1","model":"replacement-model"}),
        services,
    )
    .await
    .unwrap();
    let result = test.await.unwrap();
    assert!(
        result.is_err(),
        "the old provider result cannot confirm the new configuration"
    );
    assert!(!result.unwrap_err().contains("fake-secret"));
}

#[tokio::test]
async fn connection_test_result_is_invalidated_when_restore_resets_only_key_reference() {
    let (db, services, transport, _) = fixture(120);
    let original_config = config(&db.lock().unwrap()).unwrap();
    let request_db = db.clone();
    let request_services = services.clone();
    let test = tokio::spawn(async move {
        handle_with(
            request_db,
            "ai.test",
            json!({"consent":true}),
            request_services,
        )
        .await
    });
    for _ in 0..100 {
        if transport.calls.load(Ordering::SeqCst) > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
    {
        let mut connection = db.lock().unwrap();
        connection.execute("DELETE FROM ai_settings", []).unwrap();
        initialize(&mut connection).unwrap();
        assert_eq!(config(&connection).unwrap(), original_config);
        assert!(key_reference(&connection).unwrap().is_none());
    }
    let result = test.await.unwrap();
    assert!(
        result.is_err(),
        "same base URL, model and version cannot hide restored credential reset"
    );
    assert!(!result.unwrap_err().contains("fake-secret"));
}

#[tokio::test]
async fn connection_test_succeeds_when_complete_configuration_is_unchanged() {
    let (db, services, transport, _) = fixture(10);
    let response = handle_with(db, "ai.test", json!({"consent":true}), services)
        .await
        .unwrap();
    assert_eq!(response["ok"], true);
    assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
    assert!(!response.to_string().contains("fake-secret"));
}

#[tokio::test]
async fn complete_mock_model_workflow_survives_backup_restore_and_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("flow.sqlite3");
    let db = Arc::new(Mutex::new(crate::runtime::open_database(&path).unwrap()));
    let keys = Arc::new(MemoryKeys::default());
    let transport = Arc::new(FakeTransport {
        delay: Duration::ZERO,
        answer: Ok(fake_response(
            "测试商品容量为 500mL。依据：虚构规格表。仅用于软件验收。",
        )),
        calls: AtomicUsize::new(0),
    });
    let services = Services {
        keys,
        transport: transport.clone(),
    };
    let call = |action: &str, payload: Value| {
        crate::runtime::core_dispatch(&mut db.lock().unwrap(), action, payload).unwrap()
    };
    let brand = call("brand.create", json!({"name":"全流程隔离品牌"}));
    let b = brand["id"].as_str().unwrap();
    let product = call(
        "product.save",
        json!({
            "brandId":b,"name":"测试保温杯","sku":"AUDIT-001","category":"家居",
            "sourceText":"仅本机原始资料哨兵","privateNotes":"仅本机备注哨兵",
            "facts":[
                {"id":"capacity","label":"容量","value":"500mL","source":"虚构规格表","visibility":"public","validUntil":null},
                {"id":"cost","label":"成本","value":"内部成本哨兵","source":"内部资料","visibility":"private","validUntil":null}
            ]
        }),
    );
    call(
        "product.confirm",
        json!({"brandId":b,"id":product["id"],"expectedRevision":1}),
    );
    let q = call(
        "question.save",
        json!({"brandId":b,"productId":product["id"],"text":"全流程隔离品牌杯子的容量是多少？","intent":"selection","origin":"customer","source":"虚构问题，仅用于软件验收","branded":true,"confirmed":true}),
    );
    handle_with(
        db.clone(),
        "ai.settings.save",
        json!({"baseUrl":"https://model.example/v1","model":"mock-text","apiKey":"fake-secret"}),
        services.clone(),
    )
    .await
    .unwrap();
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
    handle_with(
        db.clone(),
        "ai.test",
        json!({"consent":true}),
        services.clone(),
    )
    .await
    .unwrap();
    let job = handle_with(db.clone(), "ai.start", json!({"brandId":b,"productId":product["id"],"kind":"faq","questionId":q["id"],"consent":true}), services.clone()).await.unwrap();
    let job = wait_job(db.clone(), &services, b, job["id"].as_str().unwrap()).await;
    assert_eq!(job["status"], "succeeded");
    assert_eq!(transport.calls.load(Ordering::SeqCst), 2);
    let snapshot = call("workspace.snapshot", json!({"brandId":b}));
    let draft = &snapshot["drafts"][0];
    assert_eq!(draft["generationMode"], "ai");
    assert!(draft["reviewedAt"].is_null());
    let reviewed = call(
        "draft.save",
        json!({"brandId":b,"id":draft["id"],"expectedRevision":1,"title":draft["title"],"body":draft["body"],"claims":[{"id":uuid::Uuid::new_v4().to_string(),"text":"测试商品容量为 500mL。","factId":"capacity"}]}),
    );
    let reviewed = call(
        "draft.review",
        json!({"brandId":b,"id":draft["id"],"expectedRevision":reviewed["revision"]}),
    );
    for format in ["markdown", "html"] {
        let exported = call(
            "draft.export",
            json!({"brandId":b,"id":draft["id"],"expectedRevision":reviewed["revision"],"format":format}),
        );
        let content = exported["content"].as_str().unwrap();
        assert!(content.contains("500mL"));
        for secret in ["fake-secret", "原始资料哨兵", "备注哨兵", "内部成本哨兵"] {
            assert!(!content.contains(secret));
        }
    }
    let publication = json!({"brandId":b,"draftId":draft["id"],"expectedRevision":reviewed["revision"],"channel":"隔离测试登记","url":"https://example.com/mgeo-audit","publishedAt":"2026-10-23T05:00:00Z","note":"没有实际发布"});
    let first = call("publication.add", publication.clone());
    assert_eq!(call("publication.add", publication)["id"], first["id"]);
    for platform in ["doubao", "yuanbao", "qianwen", "baidu"] {
        for phase in ["baseline", "followup"] {
            let observation = json!({"brandId":b,"productId":product["id"],"questionId":q["id"],"platform":platform,"surface":"consumer","phase":phase,"modelLabel":"模拟回答夹具","searchMode":"未联网","testedAt":if phase=="baseline"{"2026-10-23T05:00:00Z"}else{"2026-10-23T05:01:00Z"},"answer":"模拟回答：500mL，不代表真实平台表现。","mentioned":true,"accuracy":"correct","citedUrl":"","evidenceNote":"隔离软件验收"});
            let saved = call("observation.add", observation.clone());
            assert_eq!(call("observation.add", observation)["id"], saved["id"]);
        }
    }
    let before = call("workspace.snapshot", json!({"brandId":b}));
    assert_eq!(before["products"].as_array().unwrap().len(), 1);
    assert_eq!(before["drafts"].as_array().unwrap().len(), 1);
    assert_eq!(before["publications"].as_array().unwrap().len(), 1);
    assert_eq!(before["observations"].as_array().unwrap().len(), 8);
    let password = "Mgeo-audit-test-only-2026";
    let bytes = crate::backup::encrypt_snapshot(&db.lock().unwrap(), password).unwrap();
    call("brand.create", json!({"name":"恢复前快照标记"}));
    let recovery = crate::backup::restore_workspace(
        &mut db.lock().unwrap(),
        &bytes,
        password,
        &dir.path().join("recovery"),
    )
    .unwrap();
    assert!(recovery.is_file());
    assert_eq!(call("brand.list", json!({})).as_array().unwrap().len(), 1);
    assert_eq!(call("workspace.snapshot", json!({"brandId":b})), before);
    assert_eq!(
        handle_with(db.clone(), "ai.settings.get", json!({}), services.clone())
            .await
            .unwrap()["hasKey"],
        false
    );
    assert!(
        handle_with(db.clone(), "ai.jobs", json!({"brandId":b}), services)
            .await
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty()
    );
    let mut reopened = crate::runtime::open_database(&path).unwrap();
    assert_eq!(
        crate::runtime::core_dispatch(&mut reopened, "workspace.snapshot", json!({"brandId":b}))
            .unwrap(),
        before
    );
    crate::backup::restore_workspace(
        &mut reopened,
        &std::fs::read(recovery).unwrap(),
        password,
        &dir.path().join("rollback"),
    )
    .unwrap();
    assert_eq!(
        crate::runtime::core_dispatch(&mut reopened, "brand.list", json!({}))
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

fn monitor_fixture() -> (
    Arc<Mutex<Connection>>,
    Services,
    Arc<FakeTransport>,
    String,
    Value,
    Value,
    Value,
) {
    let (db, services, transport, brand) = fixture(0);
    let mut connection = db.lock().unwrap();
    let product = crate::core::handle(
        &mut connection,
        "product.save",
        json!({"brandId":brand,"name":"虚构商品","sku":"FICTION-001","category":"测试","sourceText":"RAW_PRIVATE_SENTINEL","privateNotes":"NOTE_PRIVATE_SENTINEL","facts":[
            {"id":"public","label":"容量","value":"500mL","source":"虚构公开规格表","visibility":"public","validUntil":null},
            {"id":"private","label":"成本","value":"FACT_PRIVATE_SENTINEL","source":"内部","visibility":"private","validUntil":null}
        ]}),
    )
    .unwrap();
    let product = crate::core::handle(
        &mut connection,
        "product.confirm",
        json!({"brandId":brand,"id":product["id"],"expectedRevision":product["revision"]}),
    )
    .unwrap();
    let question = crate::core::handle(
        &mut connection,
        "question.save",
        json!({"brandId":brand,"productId":product["id"],"text":"家用容量如何选择？","intent":"selection","origin":"customer","source":"虚构固定问题","branded":false,"confirmed":true}),
    )
    .unwrap();
    let campaign = crate::core::handle(
        &mut connection,
        "campaign.create",
        json!({"brandId":brand,"name":"虚构真实监测计划","evidenceMode":"recorded","questionIds":[question["id"]],"platforms":["doubao"],"modelLabel":"deepseek-chat","searchMode":"API；未联网","context":"固定新对话；仅公开资料","repetitions":2}),
    )
    .unwrap();
    drop(connection);
    (db, services, transport, brand, product, question, campaign)
}
fn monitor_input(brand: &str, product: &Value, question: &Value, campaign: &Value) -> Value {
    json!({"brandId":brand,"campaignId":campaign["id"],"productId":product["id"],"questionId":question["id"],"platform":"doubao","phase":"baseline","repetition":1})
}

#[tokio::test]
async fn monitor_preparation_is_deterministic_and_contains_only_public_facts() {
    let (db, services, transport, brand, product, question, campaign) = monitor_fixture();
    let input = monitor_input(&brand, &product, &question, &campaign);
    let first = handle_with(
        db.clone(),
        "ai.monitor.prepare",
        input.clone(),
        services.clone(),
    )
    .await
    .unwrap();
    let second = handle_with(db, "ai.monitor.prepare", input, services)
        .await
        .unwrap();
    assert_eq!(first["preparationHash"], second["preparationHash"]);
    assert_eq!(first["surface"], "api");
    assert_eq!(first["evidenceMode"], "recorded");
    assert_eq!(first["assessment"]["campaignId"], campaign["id"]);
    assert_eq!(first["configVersion"], 1);
    assert!(first["prompt"].as_str().unwrap().contains("500mL"));
    for sentinel in [
        "RAW_PRIVATE_SENTINEL",
        "NOTE_PRIVATE_SENTINEL",
        "FACT_PRIVATE_SENTINEL",
        "fake-secret",
    ] {
        assert!(!first.to_string().contains(sentinel));
    }
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn monitor_consent_and_valid_credential_are_required_before_job_or_transport() {
    let (db, services, transport, brand, product, question, campaign) = monitor_fixture();
    let prepared = handle_with(
        db.clone(),
        "ai.monitor.prepare",
        monitor_input(&brand, &product, &question, &campaign),
        services.clone(),
    )
    .await
    .unwrap();
    let start = json!({"brandId":brand,"preparationHash":prepared["preparationHash"]});
    assert!(handle_with(
        db.clone(),
        "ai.monitor.start",
        start.clone(),
        services.clone()
    )
    .await
    .is_err());
    services.keys.clear("test-key-ref").unwrap();
    let mut with_consent = start;
    with_consent["consent"] = json!(true);
    assert!(handle_with(
        db.clone(),
        "ai.monitor.start",
        with_consent,
        services.clone()
    )
    .await
    .is_err());
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        handle_with(db, "ai.jobs", json!({"brandId":brand}), services)
            .await
            .unwrap(),
        json!([])
    );
}

#[tokio::test]
async fn monitor_repeated_start_uses_preparation_identity_without_retry() {
    let (db, services, transport, brand, product, question, campaign) = monitor_fixture();
    let prepared = handle_with(
        db.clone(),
        "ai.monitor.prepare",
        monitor_input(&brand, &product, &question, &campaign),
        services.clone(),
    )
    .await
    .unwrap();
    let start =
        json!({"brandId":brand,"preparationHash":prepared["preparationHash"],"consent":true});
    let first = handle_with(
        db.clone(),
        "ai.monitor.start",
        start.clone(),
        services.clone(),
    )
    .await
    .unwrap();
    let second = handle_with(db.clone(), "ai.monitor.start", start, services.clone())
        .await
        .unwrap();
    assert_eq!(first["id"], second["id"]);
    let finished = wait_job(db.clone(), &services, &brand, first["id"].as_str().unwrap()).await;
    assert_eq!(
        finished["status"], "succeeded",
        "monitor job failed: {finished}"
    );
    assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
    let mut connection = db.lock().unwrap();
    let snapshot = crate::core::handle(
        &mut connection,
        "workspace.snapshot",
        json!({"brandId":brand}),
    )
    .unwrap();
    assert_eq!(snapshot["observations"].as_array().unwrap().len(), 1);
    let saved = &snapshot["observations"][0];
    for (key, expected) in [
        ("surface", "api"),
        ("evidenceMode", "recorded"),
        ("sourceKind", "real_api"),
        ("transport", "real_api"),
        ("accuracy", "unknown"),
    ] {
        assert_eq!(saved[key], expected);
    }
    assert_eq!(saved["preparationHash"], prepared["preparationHash"]);
    assert_eq!(saved["responseModel"], "deepseek-chat");
    crate::core::validate_database_detailed(&connection)
        .unwrap_or_else(|error| panic!("{error}: {saved}"));
    // Neither a frontend payload nor an unregistered internal commit can
    // manufacture native API evidence around the preparation boundary.
    assert!(crate::core::handle(
        &mut connection,
        "observation.add",
        json!({"brandId":brand,"sourceKind":"real_api"}),
    )
    .is_err());
    assert!(crate::core::handle(
        &mut connection,
        "internal.monitor.commit",
        json!({"brandId":brand,"surface":"api","evidenceMode":"recorded","sourceKind":"real_api","transport":"real_api","accuracy":"unknown","preparationHash":"0".repeat(64)}),
    )
    .is_err());
}

#[tokio::test]
async fn monitor_authentication_failure_is_not_saved_or_retried() {
    let (db, mut services, _, brand, product, question, campaign) = monitor_fixture();
    let transport = Arc::new(FakeTransport {
        delay: Duration::ZERO,
        answer: Err(TransportError::Authentication),
        calls: AtomicUsize::new(0),
    });
    services.transport = transport.clone();
    let prepared = handle_with(
        db.clone(),
        "ai.monitor.prepare",
        monitor_input(&brand, &product, &question, &campaign),
        services.clone(),
    )
    .await
    .unwrap();
    let started = handle_with(
        db.clone(),
        "ai.monitor.start",
        json!({"brandId":brand,"preparationHash":prepared["preparationHash"],"consent":true}),
        services.clone(),
    )
    .await
    .unwrap();
    let job = wait_job(
        db.clone(),
        &services,
        &brand,
        started["id"].as_str().unwrap(),
    )
    .await;
    assert_eq!(job["status"], "failed");
    assert!(job["resultText"].is_null());
    assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
    let snapshot = crate::core::handle(
        &mut db.lock().unwrap(),
        "workspace.snapshot",
        json!({"brandId":brand}),
    )
    .unwrap();
    assert_eq!(snapshot["observations"], json!([]));
}

#[test]
fn response_and_monitor_payload_preserve_real_metadata_without_accuracy_claims() {
    let parsed = parse_response(
        r#"{"id":"provider-request-1","model":"deepseek-chat","citations":["https://example.com/evidence"],"choices":[{"message":{"content":"虚构商品容量为 500mL。"}}]}"#.as_bytes(),
        "fake-secret",
    )
    .unwrap();
    assert_eq!(
        parsed.provider_request_id.as_deref(),
        Some("provider-request-1")
    );
    assert_eq!(parsed.citations, ["https://example.com/evidence"]);
    let context = json!({
        "brandId":"b","productId":"p","questionId":"q","campaignId":"c","platform":"doubao","phase":"baseline",
        "modelLabel":"deepseek-chat","searchMode":"API；未联网","testedAt":"2026-10-24T00:00:00.987654Z",
        "preparationHash":"prep","publicSnapshot":{"brandName":"测试品牌","productName":"虚构商品","sku":"FICTION-001"},
        "assessment":{"campaignId":"c","repetition":1,"context":"固定条件","recommendation":"unknown","rank":null,"competitors":""}
    });
    let (input, identity) = monitor_observation(&context, &parsed).unwrap();
    assert_eq!(input["testedAt"], "2026-10-24T00:00:00Z");
    assert_eq!(input["surface"], "api");
    assert_eq!(input["sourceKind"], "real_api");
    assert_eq!(input["transport"], "real_api");
    assert_eq!(input["evidenceMode"], "recorded");
    assert_eq!(input["mentioned"], true);
    assert_eq!(input["accuracy"], "unknown");
    assert_eq!(input["citations"], json!(["https://example.com/evidence"]));
    assert_eq!(input["providerRequestId"], "provider-request-1");
    assert_eq!(monitor_observation(&context, &parsed).unwrap().1, identity);
    for invalid in [
        r#"{"id":"fake-secret","model":"deepseek-chat","choices":[{"message":{"content":"回答"}}]}"#.as_bytes(),
        r#"{"model":"fake-secret","choices":[{"message":{"content":"回答"}}]}"#.as_bytes(),
        r#"{"model":"deepseek-chat","citations":["https://example.com/fake-secret"],"choices":[{"message":{"content":"回答"}}]}"#.as_bytes(),
    ] {
        assert!(parse_response(invalid, "fake-secret").is_err());
    }
}

#[tokio::test]
async fn simulated_campaign_cannot_be_used_for_real_monitoring() {
    let (db, services, transport, brand, product, question, campaign) = monitor_fixture();
    let mut input = monitor_input(&brand, &product, &question, &campaign);
    input["campaignId"] = json!(crate::core::handle(
        &mut db.lock().unwrap(),
        "campaign.create",
        json!({"brandId":brand,"name":"模拟计划","evidenceMode":"simulated","questionIds":[question["id"]],"platforms":["doubao"],"modelLabel":"deepseek-chat","searchMode":"模拟","context":"模拟条件","repetitions":1}),
    )
    .unwrap()["id"]);
    assert!(handle_with(db, "ai.monitor.prepare", input, services)
        .await
        .is_err());
    assert_eq!(transport.calls.load(Ordering::SeqCst), 0);
}
