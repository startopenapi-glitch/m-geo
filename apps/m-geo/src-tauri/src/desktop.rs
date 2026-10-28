use crate::{
    ai, attribution_inspection_session, backup, channel_inspection_session,
    experiment_inspection_session, files, runtime, site_inspection_session, site_package,
};
use attribution_inspection_session::AttributionInspectionSession;
use channel_inspection_session::ChannelInspectionSession;
use experiment_inspection_session::ExperimentInspectionSession;
use rusqlite::Connection;
use serde_json::{json, Value};
use site_inspection_session::SiteInspectionSession;
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tauri::Manager;

struct AppState {
    db: Arc<Mutex<Connection>>,
    _offline_worker: crate::offline_worker::OfflineWorkflowWorker,
    site_inspection: Arc<Mutex<SiteInspectionSession>>,
    experiment_inspection: Arc<Mutex<ExperimentInspectionSession>>,
    attribution_inspection: Arc<Mutex<AttributionInspectionSession>>,
    channel_inspection: Arc<Mutex<ChannelInspectionSession>>,
    data_dir: PathBuf,
    resource_dir: PathBuf,
}
fn io_error<T>(_: T) -> String {
    "文件操作未完成，请检查空间与目录权限".into()
}
fn text<'a>(payload: &'a Value, key: &str) -> Result<&'a str, String> {
    payload
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| "操作参数不完整".into())
}
fn read_bounded(path: &Path, max: u64) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path).map_err(io_error)?;
    if file.metadata().map_err(io_error)?.len() > max {
        return Err("文件超过允许大小".into());
    }
    let mut bytes = Vec::new();
    file.take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() as u64 > max {
        return Err("文件超过允许大小".into());
    }
    Ok(bytes)
}
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or_else(|| "文件目录无效".to_string())?;
    let mut tmp = tempfile::NamedTempFile::new_in(parent).map_err(io_error)?;
    tmp.write_all(bytes).map_err(io_error)?;
    tmp.as_file().sync_all().map_err(io_error)?;
    tmp.persist(path).map_err(io_error)?;
    Ok(())
}
#[tauri::command]
async fn dispatch(
    state: tauri::State<'_, AppState>,
    action: String,
    payload: Value,
) -> Result<Value, String> {
    if action == "publication.readback" {
        let publication = {
            let db = state.db.lock().map_err(|_| "资料库暂不可用".to_string())?;
            crate::core::publication_for_readback(&db, &payload)?
        };
        // Network I/O never runs while holding the SQLite lock or a transaction.
        let result = crate::core::fetch_publication(&publication).await?;
        let db = state.db.clone();
        return tauri::async_runtime::spawn_blocking(move || {
            runtime::core_dispatch(
                &mut *db.lock().map_err(|_| "资料库暂不可用".to_string())?,
                "publication.recordReadback",
                serde_json::json!({"brandId":publication["brandId"],"id":publication["id"],"result":result}),
            )
        })
        .await
        .map_err(|_| "公开 URL 回读结果保存中断".to_string())?;
    }
    if runtime::CORE_ACTIONS.contains(&action.as_str()) {
        let db = state.db.clone();
        return tauri::async_runtime::spawn_blocking(move || {
            runtime::core_dispatch(
                &mut *db.lock().map_err(|_| "资料库暂不可用".to_string())?,
                &action,
                payload,
            )
        })
        .await
        .map_err(|_| "操作中断".to_string())?;
    }
    if runtime::AI_ACTIONS.contains(&action.as_str()) {
        return ai::handle(state.db.clone(), &action, payload).await;
    }
    match action.as_str() {
        "app.info" => Ok(
            json!({"name":"M- GEO","version":env!("CARGO_PKG_VERSION"),"dataLocation":state.data_dir.to_string_lossy(),"platform":std::env::consts::OS,"releaseChannel":"test","updateUrl":"https://github.com/startopenapi-glitch/m-geo/releases"}),
        ),
        "app.openReleases" => {
            open::that("https://github.com/startopenapi-glitch/m-geo/releases")
                .map_err(|_| "无法打开版本发布页".to_string())?;
            Ok(json!({"opened":true}))
        }
        "app.openPlatform" => {
            let url = match text(&payload, "platform")? {
                "doubao" => "https://www.doubao.com/",
                "yuanbao" => "https://yuanbao.tencent.com/",
                "qianwen" => "https://www.qianwen.com/",
                "baidu" => "https://chat.baidu.com/",
                _ => return Err("不支持的平台".into()),
            };
            open::that(url).map_err(|_| "无法打开默认浏览器".to_string())?;
            Ok(json!({"opened":true}))
        }
        "app.openRecoveryFolder" => {
            let dir = state.data_dir.join("recovery");
            if !dir.is_dir() {
                return Err("还没有恢复前快照".into());
            }
            open::that(dir).map_err(|_| "无法打开恢复前快照目录".to_string())?;
            Ok(json!({"opened":true}))
        }
        "app.openSource" => {
            open::that(state.resource_dir.join("resources/OPEN-SOURCE.txt"))
                .map_err(|_| "无法打开本地许可说明".to_string())?;
            Ok(json!({"opened":true}))
        }
        "file.openText" => {
            let extensions: &[&str] = match text(&payload, "kind")? {
                "csv" => &["csv"],
                "cnvp" | "evidence" => &["json"],
                "document" => &["txt", "md", "markdown", "docx"],
                _ => return Err("不支持的文件类型".into()),
            };
            let Some(file) = rfd::AsyncFileDialog::new()
                .add_filter("资料", extensions)
                .pick_file()
                .await
            else {
                return Ok(Value::Null);
            };
            let name = file.file_name();
            let limit = if name.to_lowercase().ends_with(".docx") {
                8 * 1024 * 1024
            } else {
                2 * 1024 * 1024
            };
            let bytes = read_bounded(file.path(), limit)?;
            let content = files::decode_document(&name, &bytes)?;
            Ok(json!({"name":name,"content":content}))
        }
        "file.inspectChannelReport" => {
            channel_inspection_session::validate_payload(&action, &payload)?;
            let brand = text(&payload, "brandId")?.to_owned();
            let cache = state.channel_inspection.clone();
            // Evict the previous ticket before any asynchronous picker work.
            let generation = cache
                .lock()
                .map_err(|_| "历史通道复核会话暂不可用")?
                .begin_selection();
            let Some(file) = rfd::AsyncFileDialog::new()
                .add_filter("M-GEO 历史通道报告", &["zip"])
                .pick_file()
                .await
            else {
                return Ok(Value::Null);
            };
            let path = file.path().to_owned();
            let name = file.file_name();
            tauri::async_runtime::spawn_blocking(move || {
                let bytes = read_bounded(&path, 20 * 1024 * 1024)?;
                let package = site_package::decode_channel_package(&name, &bytes)?;
                let mut cache = cache.lock().map_err(|_| "历史通道复核会话暂不可用")?;
                cache.complete_selection(generation, &brand, package)
            })
            .await
            .map_err(|_| "历史通道复核操作中断".to_string())?
        }
        "file.readChannelInspectionPage" => {
            channel_inspection_session::validate_payload(&action, &payload)?;
            let brand = text(&payload, "brandId")?;
            let ticket = text(&payload, "inspectionId")?;
            let page = payload["page"]
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .ok_or("历史通道明细页码无效")?;
            let cache = state
                .channel_inspection
                .lock()
                .map_err(|_| "历史通道复核会话暂不可用")?;
            cache.read_page(brand, ticket, page)
        }
        "file.exportChannelInspection" => {
            channel_inspection_session::validate_payload(&action, &payload)?;
            let brand = text(&payload, "brandId")?.to_owned();
            let ticket = text(&payload, "inspectionId")?.to_owned();
            let expected = text(&payload, "reportFingerprint")?.to_owned();
            let cache = state.channel_inspection.clone();
            {
                let cache = cache.lock().map_err(|_| "历史通道复核会话暂不可用")?;
                cache.prepare_export(&brand, &ticket, &expected)?;
            }
            let Some(file) = rfd::AsyncFileDialog::new()
                .set_file_name("channel-inspection.zip")
                .add_filter("历史通道本地验收报告", &["zip"])
                .save_file()
                .await
            else {
                return Ok(Value::Null);
            };
            let path = file.path().to_owned();
            tauri::async_runtime::spawn_blocking(move || {
                // Keep the selection fixed through serialization and atomic write.
                // No session lock survives the native dialog await.
                let cache = cache.lock().map_err(|_| "历史通道复核会话暂不可用")?;
                let export = cache.prepare_export(&brand, &ticket, &expected)?;
                write_atomic(&path, &files::site_zip(&export)?)?;
                Ok(json!({"saved":true}))
            })
            .await
            .map_err(|_| "历史通道复核报告导出中断".to_string())?
        }
        "file.inspectAttributionReport" => {
            attribution_inspection_session::validate_payload(&action, &payload)?;
            let brand = text(&payload, "brandId")?.to_owned();
            let cache = state.attribution_inspection.clone();
            // Evict the previous ticket before any asynchronous picker work.
            let generation = cache
                .lock()
                .map_err(|_| "历史归因验收会话暂不可用")?
                .begin_selection();
            let Some(file) = rfd::AsyncFileDialog::new()
                .add_filter("M-GEO 历史归因报告", &["zip"])
                .pick_file()
                .await
            else {
                return Ok(Value::Null);
            };
            let path = file.path().to_owned();
            let name = file.file_name();
            tauri::async_runtime::spawn_blocking(move || {
                let bytes = read_bounded(&path, 20 * 1024 * 1024)?;
                let package = site_package::decode_attribution_package(&name, &bytes)?;
                let mut cache = cache.lock().map_err(|_| "历史归因验收会话暂不可用")?;
                cache.complete_selection(generation, &brand, package)
            })
            .await
            .map_err(|_| "历史归因验收操作中断".to_string())?
        }
        "file.readAttributionInspectionPage" => {
            attribution_inspection_session::validate_payload(&action, &payload)?;
            let brand = text(&payload, "brandId")?;
            let ticket = text(&payload, "inspectionId")?;
            let kind = text(&payload, "kind")?;
            let page = payload["page"]
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .ok_or("历史归因明细页码无效")?;
            let cache = state
                .attribution_inspection
                .lock()
                .map_err(|_| "历史归因验收会话暂不可用")?;
            cache.read_page(brand, ticket, kind, page)
        }
        "file.exportAttributionInspection" => {
            attribution_inspection_session::validate_payload(&action, &payload)?;
            let brand = text(&payload, "brandId")?.to_owned();
            let ticket = text(&payload, "inspectionId")?.to_owned();
            let expected = text(&payload, "reportFingerprint")?.to_owned();
            let cache = state.attribution_inspection.clone();
            {
                let cache = cache.lock().map_err(|_| "历史归因验收会话暂不可用")?;
                cache.prepare_export(&brand, &ticket, &expected)?;
            }
            let Some(file) = rfd::AsyncFileDialog::new()
                .set_file_name("attribution-inspection.zip")
                .add_filter("历史归因本地验收报告", &["zip"])
                .save_file()
                .await
            else {
                return Ok(Value::Null);
            };
            let path = file.path().to_owned();
            tauri::async_runtime::spawn_blocking(move || {
                // Keep the selection fixed through serialization and atomic write.
                // No session lock survives the native dialog await.
                let cache = cache.lock().map_err(|_| "历史归因验收会话暂不可用")?;
                let export = cache.prepare_export(&brand, &ticket, &expected)?;
                write_atomic(&path, &files::site_zip(&export)?)?;
                Ok(json!({"saved":true}))
            })
            .await
            .map_err(|_| "历史归因验收报告导出中断".to_string())?
        }
        "file.inspectExperimentReport" => {
            experiment_inspection_session::validate_payload(&action, &payload)?;
            let brand = text(&payload, "brandId")?.to_owned();
            let cache = state.experiment_inspection.clone();
            // Evict the previous ticket before any asynchronous picker work.
            let generation = cache
                .lock()
                .map_err(|_| "历史验收会话暂不可用")?
                .begin_selection();
            let Some(file) = rfd::AsyncFileDialog::new()
                .add_filter("M-GEO 历史实验报告", &["zip"])
                .pick_file()
                .await
            else {
                return Ok(Value::Null);
            };
            let path = file.path().to_owned();
            let name = file.file_name();
            tauri::async_runtime::spawn_blocking(move || {
                let bytes = read_bounded(&path, 20 * 1024 * 1024)?;
                let package = site_package::decode_experiment_package(&name, &bytes)?;
                let mut cache = cache.lock().map_err(|_| "历史验收会话暂不可用")?;
                cache.complete_selection(generation, &brand, package)
            })
            .await
            .map_err(|_| "历史验收操作中断".to_string())?
        }
        "file.readExperimentInspectionPage" => {
            experiment_inspection_session::validate_payload(&action, &payload)?;
            let brand = text(&payload, "brandId")?;
            let ticket = text(&payload, "inspectionId")?;
            let page = payload["page"]
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .ok_or("历史样本页码无效")?;
            let cache = state
                .experiment_inspection
                .lock()
                .map_err(|_| "历史验收会话暂不可用")?;
            cache.read_page(brand, ticket, page)
        }
        "file.exportExperimentInspection" => {
            experiment_inspection_session::validate_payload(&action, &payload)?;
            let brand = text(&payload, "brandId")?.to_owned();
            let ticket = text(&payload, "inspectionId")?.to_owned();
            let expected = text(&payload, "reportFingerprint")?.to_owned();
            let cache = state.experiment_inspection.clone();
            {
                let cache = cache.lock().map_err(|_| "历史验收会话暂不可用")?;
                cache.prepare_export(&brand, &ticket, &expected)?;
            }
            let Some(file) = rfd::AsyncFileDialog::new()
                .set_file_name("experiment-inspection.zip")
                .add_filter("历史实验本地验收报告", &["zip"])
                .save_file()
                .await
            else {
                return Ok(Value::Null);
            };
            let path = file.path().to_owned();
            tauri::async_runtime::spawn_blocking(move || {
                // Keep the selection fixed through serialization and atomic write.
                // No session lock survives the native dialog await.
                let cache = cache.lock().map_err(|_| "历史验收会话暂不可用")?;
                let export = cache.prepare_export(&brand, &ticket, &expected)?;
                write_atomic(&path, &files::site_zip(&export)?)?;
                Ok(json!({"saved":true}))
            })
            .await
            .map_err(|_| "历史验收报告导出中断".to_string())?
        }
        "file.inspectSitePackage" => {
            site_inspection_session::validate_payload(&action, &payload)?;
            let brand = text(&payload, "brandId")?.to_owned();
            let Some(file) = rfd::AsyncFileDialog::new()
                .add_filter("M-GEO 网站内容包", &["zip"])
                .pick_file()
                .await
            else {
                return Ok(Value::Null);
            };
            let path = file.path().to_owned();
            let name = file.file_name();
            let db = state.db.clone();
            let cache = state.site_inspection.clone();
            tauri::async_runtime::spawn_blocking(move || {
                let bytes = read_bounded(&path, 20 * 1024 * 1024)?;
                let package = site_package::decode_site_package(&name, &bytes)?;
                let db = db.lock().map_err(|_| "资料库暂不可用")?;
                let mut cache = cache.lock().map_err(|_| "验收会话暂不可用")?;
                cache.inspect(&db, &brand, package)
            })
            .await
            .map_err(|_| "验收操作中断".to_string())?
        }
        "file.recheckSitePackage" => {
            site_inspection_session::validate_payload(&action, &payload)?;
            let brand = text(&payload, "brandId")?.to_owned();
            let ticket = text(&payload, "inspectionId")?.to_owned();
            let db = state.db.clone();
            let cache = state.site_inspection.clone();
            tauri::async_runtime::spawn_blocking(move || {
                let db = db.lock().map_err(|_| "资料库暂不可用")?;
                let mut cache = cache.lock().map_err(|_| "验收会话暂不可用")?;
                cache.recheck(&db, &brand, &ticket)
            })
            .await
            .map_err(|_| "验收操作中断".to_string())?
        }
        "file.exportSiteInspection" => {
            site_inspection_session::validate_payload(&action, &payload)?;
            let brand = text(&payload, "brandId")?.to_owned();
            let ticket = text(&payload, "inspectionId")?.to_owned();
            let expected = text(&payload, "reportFingerprint")?.to_owned();
            {
                let db = state.db.lock().map_err(|_| "资料库暂不可用")?;
                let cache = state
                    .site_inspection
                    .lock()
                    .map_err(|_| "验收会话暂不可用")?;
                cache.prepare_export(&db, &brand, &ticket, &expected)?;
            }
            let Some(file) = rfd::AsyncFileDialog::new()
                .set_file_name("M-GEO-网站包验收报告.zip")
                .add_filter("本地验收报告", &["zip"])
                .save_file()
                .await
            else {
                return Ok(Value::Null);
            };
            let path = file.path().to_owned();
            let db = state.db.clone();
            let cache = state.site_inspection.clone();
            tauri::async_runtime::spawn_blocking(move || {
                // The workspace or latest selection may change while the dialog is open.
                // Always lock DB before cache; no lock survives a dialog await.
                let db = db.lock().map_err(|_| "资料库暂不可用")?;
                let cache = cache.lock().map_err(|_| "验收会话暂不可用")?;
                let report = cache.prepare_export(&db, &brand, &ticket, &expected)?;
                write_atomic(&path, &files::site_zip(&report)?)?;
                Ok(json!({"saved":true}))
            })
            .await
            .map_err(|_| "报告导出中断".to_string())?
        }
        "file.exportSite" => {
            {
                let mut db = state.db.lock().map_err(|_| "资料库暂不可用")?;
                runtime::core_dispatch(&mut db, "site.prepare", payload.clone())?;
            }
            let Some(file) = rfd::AsyncFileDialog::new()
                .set_file_name("M-GEO-商品网站内容包.zip")
                .add_filter("网站内容包", &["zip"])
                .save_file()
                .await
            else {
                return Ok(Value::Null);
            };
            let mut db = state.db.lock().map_err(|_| "资料库暂不可用")?;
            let current = runtime::core_dispatch(&mut db, "site.prepare", payload)?;
            write_atomic(file.path(), &files::site_zip(&current)?)?;
            Ok(json!({"saved":true}))
        }
        "file.exportTransitions" => {
            let db = state.db.clone();
            let initial_payload = payload.clone();
            tauri::async_runtime::spawn_blocking(move || {
                let mut db = db.lock().map_err(|_| "资料库暂不可用")?;
                let export =
                    runtime::core_dispatch(&mut db, "campaign.transitionExport", initial_payload)?;
                files::site_zip(&export)?;
                Ok::<(), String>(())
            })
            .await
            .map_err(|_| "推荐变化导出中断".to_string())??;
            let Some(file) = rfd::AsyncFileDialog::new()
                .set_file_name("recommendation-transitions.zip")
                .add_filter("M-GEO 本地推荐变化报告", &["zip"])
                .save_file()
                .await
            else {
                return Ok(Value::Null);
            };
            let path = file.path().to_owned();
            let db = state.db.clone();
            tauri::async_runtime::spawn_blocking(move || {
                // Keep the same payload/hash and DB lock through revalidation,
                // ZIP serialization and atomic write; no lock crosses the dialog await.
                let mut db = db.lock().map_err(|_| "资料库暂不可用")?;
                let current =
                    runtime::core_dispatch(&mut db, "campaign.transitionExport", payload)?;
                write_atomic(&path, &files::site_zip(&current)?)?;
                Ok(json!({"saved":true}))
            })
            .await
            .map_err(|_| "推荐变化导出中断".to_string())?
        }
        "file.exportReport" => {
            {
                let mut db = state.db.lock().map_err(|_| "资料库暂不可用")?;
                runtime::core_dispatch(&mut db, "campaign.report", payload.clone())?;
            }
            let Some(file) = rfd::AsyncFileDialog::new()
                .set_file_name("M-GEO-固定实验报告.zip")
                .add_filter("实测证据报告", &["zip"])
                .save_file()
                .await
            else {
                return Ok(Value::Null);
            };
            // A corrected assessment while the save dialog is open must be reflected.
            let mut db = state.db.lock().map_err(|_| "资料库暂不可用")?;
            let current = runtime::core_dispatch(&mut db, "campaign.report", payload)?;
            write_atomic(file.path(), &files::site_zip(&current)?)?;
            Ok(json!({"saved":true}))
        }
        "file.exportPilot" | "file.exportCollection" | "file.exportSiteAudit" => {
            let (core_action, filename) = match action.as_str() {
                "file.exportPilot" => ("pilot.prepare", "M-GEO-试点准备资料.zip"),
                "file.exportCollection" => ("collection.prepare", "M-GEO-实验采集任务.zip"),
                _ => ("site.audit", "M-GEO-本地页面与部署检查.zip"),
            };
            {
                let mut db = state.db.lock().map_err(|_| "资料库暂不可用")?;
                runtime::core_dispatch(&mut db, core_action, payload.clone())?;
            }
            let Some(file) = rfd::AsyncFileDialog::new()
                .set_file_name(filename)
                .add_filter("本地试点资料", &["zip"])
                .save_file()
                .await
            else {
                return Ok(Value::Null);
            };
            let mut db = state.db.lock().map_err(|_| "资料库暂不可用")?;
            let current = runtime::core_dispatch(&mut db, core_action, payload)?;
            write_atomic(file.path(), &files::site_zip(&current)?)?;
            Ok(json!({"saved":true}))
        }
        "file.exportAttribution"
        | "file.exportAttributionTemplate"
        | "file.exportLocalJob"
        | "file.exportOptimization"
        | "file.exportOfflineWorkflow"
        | "file.exportP0Workflow" => {
            let (core_action, filename) = if action == "file.exportAttribution" {
                ("attribution.report", "M-GEO-local-attribution-report.zip")
            } else if action == "file.exportOptimization" {
                ("optimization.export", "M-GEO-optimization-plan.zip")
            } else if action == "file.exportP0Workflow" {
                ("p0Workflow.export", "M-GEO-p0-rehearsal-simulated.zip")
            } else if action == "file.exportOfflineWorkflow" {
                (
                    "offlineWorkflow.export",
                    "M-GEO-offline-workflow-simulated.zip",
                )
            } else if action == "file.exportLocalJob" {
                ("localJobs.export", "M-GEO-local-import-job.zip")
            } else {
                (
                    "attribution.template",
                    "M-GEO-simulated-attribution-template.zip",
                )
            };
            {
                let mut db = state.db.lock().map_err(|_| "资料库暂不可用")?;
                let preview = runtime::core_dispatch(&mut db, core_action, payload.clone())?;
                files::site_zip(&preview)?;
            }
            let Some(file) = rfd::AsyncFileDialog::new()
                .set_file_name(filename)
                .add_filter("M-GEO 本地证据资料", &["zip"])
                .save_file()
                .await
            else {
                return Ok(Value::Null);
            };
            let mut db = state.db.lock().map_err(|_| "资料库暂不可用")?;
            let current = runtime::core_dispatch(&mut db, core_action, payload)?;
            write_atomic(file.path(), &files::site_zip(&current)?)?;
            Ok(json!({"saved":true}))
        }
        "file.exportFeed" | "file.exportFeedOperation" => {
            let (core_action, filename) = if action == "file.exportFeed" {
                ("feed.prepare", "M-GEO-local-feed-draft.zip")
            } else {
                match text(&payload, "operation")? {
                    "compare" => ("feed.compare", "M-GEO-local-feed-comparison.zip"),
                    "simulate" => ("feed.simulate", "M-GEO-simulated-receipt.zip"),
                    "receipt" => ("feed.checkReceipt", "M-GEO-simulated-receipt-check.zip"),
                    _ => return Err("不支持的本地商品通道操作".into()),
                }
            };
            {
                let mut db = state.db.lock().map_err(|_| "资料库暂不可用")?;
                let preview = runtime::core_dispatch(&mut db, core_action, payload.clone())?;
                // Enforce package bounds before opening a destination chooser as well.
                files::site_zip(&preview)?;
            }
            let Some(file) = rfd::AsyncFileDialog::new()
                .set_file_name(filename)
                .add_filter("M-GEO 本地草稿", &["zip"])
                .save_file()
                .await
            else {
                return Ok(Value::Null);
            };
            // A concurrent edit or an expiry crossing while the dialog is open invalidates
            // the preview. Recompute under the DB lock before atomically writing files.
            let mut db = state.db.lock().map_err(|_| "资料库暂不可用")?;
            let current = runtime::core_dispatch(&mut db, core_action, payload)?;
            write_atomic(file.path(), &files::site_zip(&current)?)?;
            Ok(json!({"saved":true}))
        }
        "file.exportDraft" => {
            let format = text(&payload, "format")?.to_string();
            let export = {
                let mut db = state.db.lock().map_err(|_| "资料库暂不可用".to_string())?;
                runtime::core_dispatch(&mut db, "draft.export", payload.clone())?
            };
            let Some(file) = rfd::AsyncFileDialog::new()
                .set_file_name(text(&export, "fileName")?)
                .add_filter("内容", if format == "html" { &["html"] } else { &["md"] })
                .save_file()
                .await
            else {
                return Ok(Value::Null);
            };
            // Revalidate after the dialog: a model task or another command may have changed the source.
            let mut db = state.db.lock().map_err(|_| "资料库暂不可用".to_string())?;
            let current = runtime::core_dispatch(&mut db, "draft.export", payload)?;
            write_atomic(file.path(), text(&current, "content")?.as_bytes())?;
            Ok(json!({"saved":true}))
        }
        "file.saveTemplate" => {
            let Some(file) = rfd::AsyncFileDialog::new()
                .set_file_name("M-GEO-商品导入模板.csv")
                .add_filter("CSV", &["csv"])
                .save_file()
                .await
            else {
                return Ok(Value::Null);
            };
            write_atomic(
                file.path(),
                "\u{feff}品牌,商品名称,SKU,类目,商品链接\n".as_bytes(),
            )?;
            Ok(json!({"saved":true}))
        }
        "backup.create" => {
            let password = text(&payload, "password")?.to_string();
            let Some(file) = rfd::AsyncFileDialog::new()
                .set_file_name(format!(
                    "M-GEO-{}.mgeobak",
                    chrono::Local::now().format("%Y%m%d-%H%M%S")
                ))
                .add_filter("M- GEO 加密备份", &["mgeobak"])
                .save_file()
                .await
            else {
                return Ok(Value::Null);
            };
            let db = state.db.clone();
            let path = file.path().to_path_buf();
            tauri::async_runtime::spawn_blocking(move || {
                let db = db.lock().map_err(|_| "资料库暂不可用".to_string())?;
                let bytes = backup::encrypt_snapshot(&db, &password)?;
                write_atomic(&path, &bytes)
            })
            .await
            .map_err(|_| "备份中断".to_string())??;
            Ok(json!({"saved":true}))
        }
        "backup.inspect" => {
            let password = text(&payload, "password")?.to_string();
            let Some(file) = rfd::AsyncFileDialog::new()
                .add_filter("M-GEO 加密备份", &["mgeobak"])
                .pick_file()
                .await
            else {
                return Ok(Value::Null);
            };
            let bytes = read_bounded(file.path(), 100 * 1024 * 1024 + 52)?;
            tauri::async_runtime::spawn_blocking(move || {
                backup::inspect_snapshot(&bytes, &password)
            })
            .await
            .map_err(|_| "备份校验中断".to_string())?
        }
        "backup.restore" => {
            if payload.get("confirm") != Some(&Value::Bool(true)) {
                return Err("恢复需要确认替换整个工作区".into());
            }
            let password = text(&payload, "password")?.to_string();
            let Some(file) = rfd::AsyncFileDialog::new()
                .add_filter("M- GEO 加密备份", &["mgeobak"])
                .pick_file()
                .await
            else {
                return Ok(Value::Null);
            };
            let bytes = read_bounded(file.path(), 100 * 1024 * 1024 + 52)?;
            let db = state.db.clone();
            let dir = state.data_dir.join("recovery");
            let recovery = tauri::async_runtime::spawn_blocking(move || {
                let mut db = db.lock().map_err(|_| "资料库暂不可用".to_string())?;
                backup::restore_workspace(&mut db, &bytes, &password, &dir)
            })
            .await
            .map_err(|_| "恢复中断".to_string())??;
            Ok(json!({"restored":true,"recoveryPath":recovery.to_string_lossy()}))
        }
        _ => Err("不支持的操作".into()),
    }
}
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
            }
            let path = dir.join("m-geo.sqlite3");
            let db = runtime::open_database(&path).map_err(std::io::Error::other)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
            }
            let db = Arc::new(Mutex::new(db));
            let offline_worker = crate::offline_worker::OfflineWorkflowWorker::start(db.clone())
                .map_err(std::io::Error::other)?;
            app.manage(AppState {
                db,
                _offline_worker: offline_worker,
                site_inspection: Arc::new(Mutex::new(SiteInspectionSession::default())),
                experiment_inspection: Arc::new(Mutex::new(ExperimentInspectionSession::default())),
                attribution_inspection: Arc::new(Mutex::new(
                    AttributionInspectionSession::default(),
                )),
                channel_inspection: Arc::new(Mutex::new(ChannelInspectionSession::default())),
                data_dir: dir,
                resource_dir: app.path().resource_dir()?,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![dispatch])
        .run(tauri::generate_context!())
        .expect("M- GEO 无法启动，请检查本地应用数据目录权限或版本兼容性");
}
