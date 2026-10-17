use crate::{ai, backup, files, runtime};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tauri::Manager;

struct AppState {
    db: Arc<Mutex<Connection>>,
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
            json!({"name":"M- GEO","version":env!("CARGO_PKG_VERSION"),"dataLocation":state.data_dir.to_string_lossy(),"platform":std::env::consts::OS,"releaseChannel":"internal","updateUrl":null}),
        ),
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
                "cnvp" => &["json"],
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
            app.manage(AppState {
                db: Arc::new(Mutex::new(db)),
                data_dir: dir,
                resource_dir: app.path().resource_dir()?,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![dispatch])
        .run(tauri::generate_context!())
        .expect("M- GEO 无法启动，请检查本地应用数据目录权限或版本兼容性");
}
