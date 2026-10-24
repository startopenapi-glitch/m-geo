//! One ephemeral frozen-artifact inspection; no database access or persistence.
use crate::core;
use serde_json::{json, Value};
type Result<T> = std::result::Result<T, String>;
const PAGE_SIZE: usize = 20;
const INVALID_SESSION: &str = "历史实验验收会话已失效，请重新选择ZIP";
const INVALID_PAYLOAD: &str = "历史实验验收参数无效";

struct StoredInspection {
    id: String,
    brand_id: String,
    report: Value,
    observations: Vec<Value>,
}
#[derive(Default)]
pub struct ExperimentInspectionSession {
    generation: u64,
    current: Option<StoredInspection>,
}
impl ExperimentInspectionSession {
    /// Reserve before opening the picker. Cancellation/failure leaves the session empty.
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
        if generation != self.generation || generation == 0 {
            return Err(INVALID_SESSION.into());
        }
        self.current = None;
        let inspected = core::inspect_experiment_report(brand, &package)?;
        let id = uuid::Uuid::new_v4().to_string();
        let envelope = json!({"inspectionId":id,"report":inspected.report});
        self.current = Some(StoredInspection {
            id,
            brand_id: brand.to_owned(),
            report: inspected.report,
            observations: inspected.observations,
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
        let total = selected.observations.len();
        let pages = total.div_ceil(PAGE_SIZE).max(1);
        if page >= pages {
            return Err("历史样本页码无效".into());
        }
        let start = page * PAGE_SIZE;
        let end = (start + PAGE_SIZE).min(total);
        Ok(
            json!({"inspectionId":ticket,"page":page,"pageSize":PAGE_SIZE,"total":total,"observations":selected.observations[start..end]}),
        )
    }
    /// Call before the save picker and again after it, while holding the session lock.
    pub fn prepare_export(&self, brand: &str, ticket: &str, expected: &str) -> Result<Value> {
        let selected = self.selected(brand, ticket)?;
        if selected.report["reportFingerprint"].as_str() != Some(expected) {
            return Err("历史验收报告已变化，请重新核对后导出".into());
        }
        core::experiment_inspection_export(&selected.report)
    }
}
fn canonical_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value)
}
/// Native commands accept identifiers only. No caller-authored path, report, or files.
pub fn validate_payload(action: &str, payload: &Value) -> Result<()> {
    let keys: &[&str] = match action {
        "file.inspectExperimentReport" => &["brandId"],
        "file.readExperimentInspectionPage" => &["brandId", "inspectionId", "page"],
        "file.exportExperimentInspection" => &["brandId", "inspectionId", "reportFingerprint"],
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
#[path = "experiment_inspection_session_tests.rs"]
mod tests;
