//! One brand-bound historical attribution result, held only in process memory.
use crate::core;
use serde_json::{json, Value};
type Result<T> = std::result::Result<T, String>;
const PAGE_SIZE: usize = 20;
const INVALID_SESSION: &str = "历史归因验收会话已失效，请重新选择ZIP";
const INVALID_PAYLOAD: &str = "历史归因验收参数无效";

struct StoredInspection {
    id: String,
    brand_id: String,
    report: Value,
    orders: Vec<Value>,
    products: Vec<Value>,
}
#[derive(Default)]
pub struct AttributionInspectionSession {
    generation: u64,
    current: Option<StoredInspection>,
}
impl AttributionInspectionSession {
    /// Reserve before opening the picker; cancellation/failure leaves no previous ticket.
    pub fn begin_selection(&mut self) -> u64 {
        self.current = None;
        self.generation = self.generation.wrapping_add(1);
        self.generation
    }
    pub fn complete_selection(
        &mut self,
        generation: u64,
        brand: &str,
        package: Value,
    ) -> Result<Value> {
        if generation == 0 || generation != self.generation {
            return Err(INVALID_SESSION.into());
        }
        self.current = None;
        let inspected = core::inspect_attribution_report(brand, &package)?;
        let id = uuid::Uuid::new_v4().to_string();
        let envelope = json!({"inspectionId":id,"report":inspected.report});
        self.current = Some(StoredInspection {
            id,
            brand_id: brand.to_owned(),
            report: inspected.report,
            orders: inspected.orders,
            products: inspected.products,
        });
        Ok(envelope)
    }
    fn selected(&self, brand: &str, ticket: &str) -> Result<&StoredInspection> {
        self.current
            .as_ref()
            .filter(|s| s.brand_id == brand && s.id == ticket)
            .ok_or_else(|| INVALID_SESSION.into())
    }
    pub fn read_page(&self, brand: &str, ticket: &str, kind: &str, page: usize) -> Result<Value> {
        let selected = self.selected(brand, ticket)?;
        let rows = match kind {
            "orders" => &selected.orders,
            "products" => &selected.products,
            _ => return Err("历史归因明细类型无效".into()),
        };
        let total = rows.len();
        if page >= total.div_ceil(PAGE_SIZE).max(1) {
            return Err("历史归因明细页码无效".into());
        }
        let start = page * PAGE_SIZE;
        let end = (start + PAGE_SIZE).min(total);
        Ok(
            json!({"inspectionId":ticket,"kind":kind,"page":page,"pageSize":PAGE_SIZE,"total":total,"rows":rows[start..end]}),
        )
    }
    /// Recheck before and after the save dialog; the caller keeps the lock through atomic write.
    pub fn prepare_export(&self, brand: &str, ticket: &str, fingerprint: &str) -> Result<Value> {
        let selected = self.selected(brand, ticket)?;
        if selected.report["reportFingerprint"].as_str() != Some(fingerprint) {
            return Err("历史归因验收报告已变化，请重新核对后导出".into());
        }
        core::attribution_inspection_export(&selected.report)
    }
}
fn canonical_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value)
}
/// Native-only identifier payloads never accept caller-authored reports, files or paths.
pub fn validate_payload(action: &str, payload: &Value) -> Result<()> {
    let keys: &[&str] = match action {
        "file.inspectAttributionReport" => &["brandId"],
        "file.readAttributionInspectionPage" => &["brandId", "inspectionId", "kind", "page"],
        "file.exportAttributionInspection" => &["brandId", "inspectionId", "reportFingerprint"],
        _ => return Err(INVALID_PAYLOAD.into()),
    };
    let error = || INVALID_PAYLOAD.to_string();
    let object = payload.as_object().ok_or_else(error)?;
    if object.len() != keys.len() || object.keys().any(|key| !keys.contains(&key.as_str())) {
        return Err(error());
    }
    for key in keys {
        if *key == "page" {
            if object[*key]
                .as_u64()
                .filter(|n| *n <= 9_007_199_254_740_991)
                .and_then(|n| usize::try_from(n).ok())
                .and_then(|n| n.checked_mul(PAGE_SIZE))
                .is_none()
            {
                return Err(error());
            }
            continue;
        }
        let value = object[*key].as_str().ok_or_else(error)?;
        let valid = match *key {
            "kind" => matches!(value, "orders" | "products"),
            "reportFingerprint" => {
                value.len() == 64
                    && value
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            }
            _ => canonical_uuid(value),
        };
        if !valid {
            return Err(error());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{core, files, runtime, site_package};
    use serde_json::{json, Value};
    const FOREIGN: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
    fn package(count: usize) -> (String, Value) {
        let mut db = runtime::open_database(std::path::Path::new(":memory:")).unwrap();
        let brand = core::handle(&mut db, "brand.create", json!({"name":"虚构历史分页品牌"}))
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let mut orders = vec![];
        for i in 0..count.max(1) {
            let sku = format!("=PRIVATE-SKU-{i:03}\n\t");
            core::handle(
                &mut db,
                "product.save",
                json!({"brandId":brand,"sku":sku,"name":format!("<PRIVATE-NAME-{i}>"),"facts":[]}),
            )
            .unwrap();
            if count > 0 {
                orders.push(json!({"id":format!("PRIVATE-ORDER-{i:03}"),"sessionId":null,"sku":sku,"occurredAt":"2026-10-03T00:00:00Z","status":"paid","amountMinor":100}));
            }
        }
        let visits = if count == 0 {
            json!([{"id":"PRIVATE-VISIT","sessionId":"PRIVATE-SESSION","sku":"=PRIVATE-SKU-000\n\t","occurredAt":"2026-10-01T00:00:00Z","source":"unknown","sourceEvidence":"unknown"}])
        } else {
            json!([])
        };
        let input = json!({"format":"m-geo-attribution-v1","evidenceMode":"simulated","sourceLabel":"PRIVATE-SOURCE","coverageStart":"2026-10-01T00:00:00Z","coverageEnd":"2026-11-01T00:00:00Z","currency":"CNY","visits":visits,"orders":orders,"refunds":[]});
        let mut request =
            json!({"brandId":brand,"name":"PRIVATE-DATASET","content":input.to_string()});
        let preview = core::handle(&mut db, "attribution.preview", request.clone()).unwrap();
        request["expectedHash"] = preview["contentHash"].clone();
        let saved = core::handle(&mut db, "attribution.apply", request).unwrap();
        let report = core::handle(&mut db,"attribution.report",json!({"brandId":brand,"datasetId":saved["datasetId"],"from":"2026-10-02T00:00:00Z","to":"2026-10-23T00:00:00Z","lookbackDays":7})).unwrap();
        let zip = files::site_zip(&report).unwrap();
        (
            brand,
            site_package::decode_attribution_package("fictional.zip", &zip).unwrap(),
        )
    }
    fn install(session: &mut AttributionInspectionSession, brand: &str, package: Value) -> Value {
        let generation = session.begin_selection();
        session
            .complete_selection(generation, brand, package)
            .unwrap()
    }
    #[test]
    fn selection_clears_before_cancel_fail_and_rejects_late_install() {
        let (brand, package) = package(0);
        let mut session = AttributionInspectionSession::default();
        assert!(session
            .complete_selection(0, &brand, package.clone())
            .is_err());
        let initial = install(&mut session, &brand, package.clone());
        let old = initial["inspectionId"].as_str().unwrap();
        let slow = session.begin_selection();
        assert!(session.read_page(&brand, old, "orders", 0).is_err());
        let latest = session.begin_selection();
        assert!(session
            .complete_selection(slow, &brand, package.clone())
            .is_err());
        assert!(session
            .complete_selection(latest, &brand, json!({}))
            .is_err());
        assert!(session.read_page(&brand, old, "orders", 0).is_err());
        let current = install(&mut session, &brand, package.clone());
        assert!(session.complete_selection(latest, &brand, package).is_err());
        assert_eq!(
            session
                .read_page(
                    &brand,
                    current["inspectionId"].as_str().unwrap(),
                    "orders",
                    0
                )
                .unwrap()["rows"],
            json!([])
        );
    }
    #[test]
    fn both_page_kinds_are_bounded_and_bind_ticket_brand_and_range() {
        let mut session = AttributionInspectionSession::default();
        let (brand, empty) = package(0);
        let selected = install(&mut session, &brand, empty);
        let ticket = selected["inspectionId"].as_str().unwrap();
        for kind in ["orders", "products"] {
            assert_eq!(
                session.read_page(&brand, ticket, kind, 0).unwrap(),
                json!({"inspectionId":ticket,"kind":kind,"page":0,"pageSize":20,"total":0,"rows":[]})
            );
            assert!(session.read_page(&brand, ticket, kind, 1).is_err());
        }
        let (brand, many) = package(21);
        let selected = install(&mut session, &brand, many);
        let ticket = selected["inspectionId"].as_str().unwrap();
        for kind in ["orders", "products"] {
            let page = session.read_page(&brand, ticket, kind, 0).unwrap();
            assert_eq!(page["total"], 21);
            assert_eq!(page["rows"].as_array().unwrap().len(), 20);
            assert_eq!(
                session.read_page(&brand, ticket, kind, 1).unwrap()["rows"]
                    .as_array()
                    .unwrap()
                    .len(),
                1
            );
            assert!(session.read_page(&brand, ticket, kind, 2).is_err());
            assert!(session.read_page(&brand, ticket, kind, usize::MAX).is_err());
            assert!(session.read_page(FOREIGN, ticket, kind, 0).is_err());
            assert!(session.read_page(&brand, FOREIGN, kind, 0).is_err());
        }
        for kind in ["visits", "refunds", "Orders", ""] {
            assert!(session.read_page(&brand, ticket, kind, 0).is_err());
        }
        assert!(AttributionInspectionSession::default()
            .read_page(&brand, ticket, "orders", 0)
            .is_err());
    }
    #[test]
    fn export_rechecks_fingerprint_and_replacement_and_omits_identifiers() {
        let (brand, package) = package(21);
        let mut session = AttributionInspectionSession::default();
        let selected = install(&mut session, &brand, package.clone());
        let ticket = selected["inspectionId"].as_str().unwrap();
        let fingerprint = selected["report"]["reportFingerprint"].as_str().unwrap();
        assert!(session
            .prepare_export(FOREIGN, ticket, fingerprint)
            .is_err());
        assert!(session
            .prepare_export(&brand, FOREIGN, fingerprint)
            .is_err());
        assert!(session
            .prepare_export(&brand, ticket, &"0".repeat(64))
            .is_err());
        let export = session.prepare_export(&brand, ticket, fingerprint).unwrap();
        assert_eq!(export["files"].as_array().unwrap().len(), 2);
        assert!(!export.to_string().contains("PRIVATE-"));
        session.begin_selection();
        assert!(session.prepare_export(&brand, ticket, fingerprint).is_err());
        install(&mut session, &brand, package);
        assert!(session.prepare_export(&brand, ticket, fingerprint).is_err());
    }
    #[test]
    fn exact_native_payloads_only_accept_identifiers_kind_and_safe_page() {
        let brand = "11111111-1111-4111-8111-111111111111";
        let id = "22222222-2222-4222-8222-222222222222";
        for (action, payload) in [
            ("file.inspectAttributionReport", json!({"brandId":brand})),
            (
                "file.readAttributionInspectionPage",
                json!({"brandId":brand,"inspectionId":id,"kind":"orders","page":0}),
            ),
            (
                "file.readAttributionInspectionPage",
                json!({"brandId":brand,"inspectionId":id,"kind":"products","page":9007199254740991u64}),
            ),
            (
                "file.exportAttributionInspection",
                json!({"brandId":brand,"inspectionId":id,"reportFingerprint":"a".repeat(64)}),
            ),
        ] {
            validate_payload(action, &payload).unwrap();
            for key in ["path", "report", "files", "data", "extra"] {
                let mut forged = payload.clone();
                forged[key] = json!("private");
                assert!(validate_payload(action, &forged).is_err());
            }
            for key in payload.as_object().unwrap().keys() {
                let mut missing = payload.clone();
                missing.as_object_mut().unwrap().remove(key);
                assert!(validate_payload(action, &missing).is_err());
                for value in [json!(null), json!(true), json!([]), json!({})] {
                    let mut wrong = payload.clone();
                    wrong[key] = value;
                    assert!(validate_payload(action, &wrong).is_err());
                }
            }
        }
        for page in [
            json!(-1),
            json!(0.5),
            json!("0"),
            json!(9007199254740992u64),
            json!(u64::MAX),
        ] {
            assert!(validate_payload(
                "file.readAttributionInspectionPage",
                &json!({"brandId":brand,"inspectionId":id,"kind":"orders","page":page})
            )
            .is_err());
        }
        for kind in ["visits", "refunds", "Orders", ""] {
            assert!(validate_payload(
                "file.readAttributionInspectionPage",
                &json!({"brandId":brand,"inspectionId":id,"kind":kind,"page":0})
            )
            .is_err());
        }
        for value in [
            "11111111111141118111111111111111",
            "AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA",
            "not-uuid",
        ] {
            assert!(
                validate_payload("file.inspectAttributionReport", &json!({"brandId":value}))
                    .is_err()
            );
            assert!(validate_payload(
                "file.exportAttributionInspection",
                &json!({"brandId":brand,"inspectionId":value,"reportFingerprint":"a".repeat(64)})
            )
            .is_err());
        }
        for fp in ["A".repeat(64), "a".repeat(63), "g".repeat(64)] {
            assert!(validate_payload(
                "file.exportAttributionInspection",
                &json!({"brandId":brand,"inspectionId":id,"reportFingerprint":fp})
            )
            .is_err());
        }
        for payload in [json!(null), json!([]), json!({})] {
            assert!(validate_payload("file.inspectAttributionReport", &payload).is_err());
        }
        assert!(validate_payload("unknown", &json!({})).is_err());
    }
}
