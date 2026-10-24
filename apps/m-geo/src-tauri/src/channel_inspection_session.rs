//! One brand-bound historical channel operation result, held only in process memory.
use crate::core;
use serde_json::{json, Value};
type Result<T> = std::result::Result<T, String>;
const PAGE_SIZE: usize = 20;
const INVALID_SESSION: &str = "历史通道复核会话已失效，请重新选择ZIP";
const INVALID_PAYLOAD: &str = "历史通道复核参数无效";

struct StoredInspection {
    id: String,
    brand_id: String,
    report: Value,
    rows: Vec<Value>,
}
#[derive(Default)]
pub struct ChannelInspectionSession {
    generation: u64,
    pending: bool,
    current: Option<StoredInspection>,
}
impl ChannelInspectionSession {
    /// Reserve before opening the picker; cancellation/failure leaves no previous ticket.
    pub fn begin_selection(&mut self) -> u64 {
        self.current = None;
        self.generation = self.generation.wrapping_add(1).max(1);
        self.pending = true;
        self.generation
    }
    pub fn complete_selection(
        &mut self,
        generation: u64,
        brand: &str,
        package: Value,
    ) -> Result<Value> {
        if generation == 0 || generation != self.generation || !self.pending {
            return Err(INVALID_SESSION.into());
        }
        self.pending = false;
        self.current = None;
        if !canonical_uuid(brand) {
            return Err(INVALID_PAYLOAD.into());
        }
        let inspected = core::inspect_channel_report(brand, &package)?;
        let id = uuid::Uuid::new_v4().to_string();
        let envelope = json!({"inspectionId":id,"report":inspected.report});
        self.current = Some(StoredInspection {
            id,
            brand_id: brand.to_owned(),
            report: inspected.report,
            rows: inspected.rows,
        });
        Ok(envelope)
    }
    fn selected(&self, brand: &str, ticket: &str) -> Result<&StoredInspection> {
        self.current
            .as_ref()
            .filter(|s| s.brand_id == brand && s.id == ticket)
            .ok_or_else(|| INVALID_SESSION.into())
    }
    pub fn read_page(&self, brand: &str, ticket: &str, page: usize) -> Result<Value> {
        let selected = self.selected(brand, ticket)?;
        let total = selected.rows.len();
        if page >= total.div_ceil(PAGE_SIZE).max(1) {
            return Err("历史通道明细页码无效".into());
        }
        let start = page.checked_mul(PAGE_SIZE).ok_or("历史通道明细页码无效")?;
        let end = start.saturating_add(PAGE_SIZE).min(total);
        Ok(
            json!({"inspectionId":ticket,"page":page,"pageSize":PAGE_SIZE,"total":total,"rows":selected.rows[start..end]}),
        )
    }
    /// Recheck before and after the save dialog; caller holds the lock through atomic write.
    pub fn prepare_export(&self, brand: &str, ticket: &str, fingerprint: &str) -> Result<Value> {
        let selected = self.selected(brand, ticket)?;
        if selected.report["reportFingerprint"].as_str() != Some(fingerprint) {
            return Err("历史通道复核报告已变化，请重新核对后导出".into());
        }
        core::channel_inspection_export(&selected.report)
    }
}
fn canonical_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value)
}
/// Native-only payloads never accept caller-authored reports, files, paths or page kinds.
pub fn validate_payload(action: &str, payload: &Value) -> Result<()> {
    let keys: &[&str] = match action {
        "file.inspectChannelReport" => &["brandId"],
        "file.readChannelInspectionPage" => &["brandId", "inspectionId", "page"],
        "file.exportChannelInspection" => &["brandId", "inspectionId", "reportFingerprint"],
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
        let valid = if *key == "reportFingerprint" {
            value.len() == 64
                && value
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        } else {
            canonical_uuid(value)
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
    use sha2::{Digest, Sha256};
    const BRAND: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
    const FOREIGN: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
    fn seal(value: &mut Value, key: &str) {
        value.as_object_mut().unwrap().remove(key);
        value[key] = json!(hex::encode(Sha256::digest(
            serde_json::to_vec(value).unwrap()
        )));
    }
    fn manifest(start: usize, count: usize) -> Value {
        let items: Vec<Value> = (start..start + count).map(|n| {
            let sku = format!("PRIVATE-SKU-{n:03}");
            let mut row = json!({"productId":format!("{n:08x}-aaaa-4aaa-8aaa-aaaaaaaaaaaa"),"draftId":format!("{n:08x}-bbbb-4bbb-8bbb-bbbbbbbbbbbb"),"sku":sku,"productRevision":1,"draftRevision":1,"localStatus":"ready","data":{"id":sku,"title":"Frozen title","description":"PRIVATE-DESCRIPTION","link":"https://example.com/PRIVATE-URL","imageLink":"https://example.com/image.png","price":"29.90","currency":"CNY","availability":"InStock","catalog":{"kind":"standalone","condition":"new","titleOrigin":"human","productClass":"physical_non_apparel","brand":"Frozen brand","mpn":"X"}},"validity":[{"field":"commerce:availability","validUntil":"2001-01-01"},{"field":"commerce:price","validUntil":"2001-01-01"}]});
            seal(&mut row, "rowHash"); row
        }).collect();
        let mut manifest = json!({"format":"m-geo-feed-v1","schemaVersion":1,"profile":"mgeo-catalog-v1","brandId":BRAND,"mode":"local_draft","submissionStatus":"not_submitted","selectionScope":"selected_products_not_full_merchant_catalog","items":items});
        seal(&mut manifest, "packageId");
        manifest
    }
    fn package(count: usize, disjoint: bool) -> Value {
        let baseline = manifest(1, count);
        let current = manifest(if disjoint { count + 1 } else { 1 }, count);
        let notice="仅比较本次本地选择。移除表示不在当前本地选择中，不代表平台删除，也不是完整商家目录替换授权。所有商品尚未提交平台；摘要用于完整性核对，不是签名或认证。\n";
        // A deliberately altered report is valid blocked evidence; rows are recomputed.
        let files = json!([
            {"path":"comparison-report.json","content":"{}"},
            {"path":"baseline-manifest.json","content":baseline.to_string()},
            {"path":"feed-manifest.json","content":current.to_string()},
            {"path":"local-operation-notice.txt","content":notice}
        ]);
        let size: usize = files
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["content"].as_str().unwrap().len())
            .sum();
        json!({"source":{"fileName":"fictional.zip","sha256":"b".repeat(64),"compressedBytes":512,"uncompressedBytes":size,"fileCount":4},"files":files})
    }
    fn install(session: &mut ChannelInspectionSession, package: Value) -> Value {
        let generation = session.begin_selection();
        session
            .complete_selection(generation, BRAND, package)
            .unwrap()
    }
    #[test]
    fn channel_session_selection_clears_cancel_failure_and_late_generation() {
        let mut session = ChannelInspectionSession::default();
        let package = package(1, false);
        assert!(session
            .complete_selection(0, BRAND, package.clone())
            .is_err());
        let initial = install(&mut session, package.clone());
        let old = initial["inspectionId"].as_str().unwrap();
        let slow = session.begin_selection();
        assert!(session.read_page(BRAND, old, 0).is_err());
        let latest = session.begin_selection();
        assert!(session
            .complete_selection(slow, BRAND, package.clone())
            .is_err());
        assert!(session
            .complete_selection(latest, BRAND, json!({}))
            .is_err());
        assert!(session.read_page(BRAND, old, 0).is_err());
        let generation = session.begin_selection();
        let current = session
            .complete_selection(generation, BRAND, package.clone())
            .unwrap();
        assert!(session
            .complete_selection(generation, BRAND, package.clone())
            .is_err());
        assert!(session.complete_selection(latest, BRAND, package).is_err());
        assert!(session
            .read_page(BRAND, current["inspectionId"].as_str().unwrap(), 0)
            .is_ok());
    }
    #[test]
    fn channel_session_pages_bind_brand_ticket_and_bound_21_and_400_rows() {
        for (count, disjoint, total) in [(21, false, 21), (200, true, 400)] {
            let mut session = ChannelInspectionSession::default();
            let selected = install(&mut session, package(count, disjoint));
            let ticket = selected["inspectionId"].as_str().unwrap();
            assert_eq!(
                session.read_page(BRAND, ticket, 0).unwrap()["rows"]
                    .as_array()
                    .unwrap()
                    .len(),
                20
            );
            assert_eq!(session.read_page(BRAND, ticket, 0).unwrap()["total"], total);
            let last = (total - 1) / 20;
            assert_eq!(
                session.read_page(BRAND, ticket, last).unwrap()["rows"]
                    .as_array()
                    .unwrap()
                    .len(),
                if total == 21 { 1 } else { 20 }
            );
            assert!(session.read_page(BRAND, ticket, last + 1).is_err());
            assert!(session.read_page(BRAND, ticket, usize::MAX).is_err());
            assert!(session.read_page(FOREIGN, ticket, 0).is_err());
            assert!(session.read_page(BRAND, FOREIGN, 0).is_err());
            assert!(ChannelInspectionSession::default()
                .read_page(BRAND, ticket, 0)
                .is_err());
        }
    }
    #[test]
    fn channel_session_empty_rows_have_only_page_zero() {
        let session = ChannelInspectionSession {
            generation: 1,
            pending: false,
            current: Some(StoredInspection {
                id: FOREIGN.into(),
                brand_id: BRAND.into(),
                report: json!({}),
                rows: vec![],
            }),
        };
        assert_eq!(
            session.read_page(BRAND, FOREIGN, 0).unwrap(),
            json!({"inspectionId":FOREIGN,"page":0,"pageSize":20,"total":0,"rows":[]})
        );
        assert!(session.read_page(BRAND, FOREIGN, 1).is_err());
        assert!(session.read_page(BRAND, FOREIGN, usize::MAX).is_err());
    }

    #[test]
    fn channel_session_export_rechecks_fingerprint_and_replacement() {
        let mut session = ChannelInspectionSession::default();
        let selected = install(&mut session, package(21, false));
        let ticket = selected["inspectionId"].as_str().unwrap();
        let fingerprint = selected["report"]["reportFingerprint"].as_str().unwrap();
        assert!(session
            .prepare_export(FOREIGN, ticket, fingerprint)
            .is_err());
        assert!(session.prepare_export(BRAND, FOREIGN, fingerprint).is_err());
        assert!(session
            .prepare_export(BRAND, ticket, &"0".repeat(64))
            .is_err());
        let export = session.prepare_export(BRAND, ticket, fingerprint).unwrap();
        assert_eq!(export["files"].as_array().unwrap().len(), 2);
        assert!(!export.to_string().contains("PRIVATE-"));
        session.begin_selection();
        assert!(session.prepare_export(BRAND, ticket, fingerprint).is_err());
        install(&mut session, package(1, false));
        assert!(session.prepare_export(BRAND, ticket, fingerprint).is_err());
    }
    #[test]
    fn channel_session_native_payloads_are_exact_and_typed() {
        for (action, payload) in [
            ("file.inspectChannelReport", json!({"brandId":BRAND})),
            (
                "file.readChannelInspectionPage",
                json!({"brandId":BRAND,"inspectionId":FOREIGN,"page":0}),
            ),
            (
                "file.readChannelInspectionPage",
                json!({"brandId":BRAND,"inspectionId":FOREIGN,"page":9007199254740991u64}),
            ),
            (
                "file.exportChannelInspection",
                json!({"brandId":BRAND,"inspectionId":FOREIGN,"reportFingerprint":"a".repeat(64)}),
            ),
        ] {
            validate_payload(action, &payload).unwrap();
            for key in ["path", "report", "files", "kind", "data", "extra"] {
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
                "file.readChannelInspectionPage",
                &json!({"brandId":BRAND,"inspectionId":FOREIGN,"page":page})
            )
            .is_err());
        }
        for uuid in [
            "AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA",
            "aaaaaaaaaaaa4aaa8aaaaaaaaaaaaaaa",
            "not-uuid",
        ] {
            assert!(
                validate_payload("file.inspectChannelReport", &json!({"brandId":uuid})).is_err()
            );
            assert!(validate_payload(
                "file.readChannelInspectionPage",
                &json!({"brandId":BRAND,"inspectionId":uuid,"page":0})
            )
            .is_err());
        }
        for fp in ["A".repeat(64), "g".repeat(64), "a".repeat(63)] {
            assert!(validate_payload(
                "file.exportChannelInspection",
                &json!({"brandId":BRAND,"inspectionId":FOREIGN,"reportFingerprint":fp})
            )
            .is_err());
        }
        for payload in [json!(null), json!([]), json!({})] {
            assert!(validate_payload("file.inspectChannelReport", &payload).is_err());
        }
        assert!(validate_payload("unknown", &json!({})).is_err());
    }
}
