//! Disposable real-core historical ZIP acceptance. No network, model, or publishing actions.
use m_geo_lib::{
    core, experiment_inspection_session::ExperimentInspectionSession, files, runtime, site_package,
};
use rusqlite::{types::ValueRef, Connection, OpenFlags};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
};

const RAW: &str = "  =SUM(1,2)\u{feff}\"P11_PRIVATE_RAW_5290\"\n<script>P11_INERT_5290</script>  ";
const NOTE: &str = "P11_PRIVATE_NOTE_5290";
const COMPETITOR: &str = "P11_PRIVATE_COMPETITOR_5290";
const HOST: &str = "p11-private-host-5290.example";
const URL: &str = "https://p11-private-host-5290.example/P11_PRIVATE_URL_5290";
const QUESTION: &str = "P11_PRIVATE_QUESTION_5290";
const PLAN: &str = "P11_PRIVATE_PLAN_5290";
const CONTEXT: &str = "P11_PRIVATE_CONTEXT_5290";
const PRODUCT: &str = "P11_PRIVATE_PRODUCT_5290";
const MARKERS: [&str; 11] = [
    "P11_PRIVATE_RAW_5290",
    "P11_INERT_5290",
    NOTE,
    COMPETITOR,
    HOST,
    "P11_PRIVATE_URL_5290",
    QUESTION,
    PLAN,
    CONTEXT,
    PRODUCT,
    "P11_PRIVATE_LIVE_NOTE_5290",
];

fn call(db: &mut Connection, action: &str, payload: Value) -> Value {
    runtime::core_dispatch(db, action, payload).unwrap_or_else(|e| panic!("{action}: {e}"))
}
fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
fn write(dir: &Path, name: &str, bytes: &[u8]) {
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dir.join(name))
        .unwrap();
    f.write_all(bytes).unwrap();
    f.sync_all().unwrap();
}
fn save(dir: &Path, name: &str, value: &Value) {
    write(dir, name, &serde_json::to_vec_pretty(value).unwrap());
}
fn content<'a>(package: &'a Value, path: &str) -> &'a str {
    package["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["path"] == path)
        .unwrap()["content"]
        .as_str()
        .unwrap()
}
fn replace(package: &mut Value, path: &str, text: String) {
    package["files"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|f| f["path"] == path)
        .unwrap()["content"] = json!(text);
}
fn decode(name: &str, package: &Value) -> Value {
    site_package::decode_experiment_package(name, &files::site_zip(package).unwrap()).unwrap()
}
fn artifact(dir: &Path, name: &str, package: &Value) -> Value {
    write(dir, name, &files::site_zip(package).unwrap());
    site_package::decode_experiment_package(name, &fs::read(dir.join(name)).unwrap()).unwrap()
}
fn table_hashes(db: &Connection) -> Value {
    let names: Vec<String> = db
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let mut result = json!({});
    for name in names {
        assert!(name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'));
        let mut st = db.prepare(&format!("SELECT * FROM {name}")).unwrap();
        let columns = st.column_count();
        let mut rows: Vec<String> = st
            .query_map([], |r| {
                let values: Vec<Value> = (0..columns)
                    .map(|i| match r.get_ref(i).unwrap() {
                        ValueRef::Null => Value::Null,
                        ValueRef::Integer(n) => json!({"integer":n}),
                        ValueRef::Real(n) => json!({"real":n}),
                        ValueRef::Text(s) => json!({"text":hex::encode(s)}),
                        ValueRef::Blob(s) => json!({"blob":hex::encode(s)}),
                    })
                    .collect();
                Ok(serde_json::to_string(&values).unwrap())
            })
            .unwrap()
            .map(Result::unwrap)
            .collect();
        rows.sort();
        result[name] = json!({"rows":rows.len(),"sha256":digest(serde_json::to_string(&rows).unwrap().as_bytes())});
    }
    result
}
fn required_fixture(key: &str) -> PathBuf {
    let path = PathBuf::from(std::env::var_os(key).unwrap_or_else(|| {
        panic!("required historical fixture environment variable {key} is missing")
    }));
    assert!(
        path.is_file(),
        "required historical fixture {key} is missing"
    );
    path
}
fn seed_plan(
    db: &mut Connection,
    brand: &Value,
    suffix: &str,
    questions: usize,
    platforms: Value,
    reps: usize,
) -> (Value, Value) {
    let product = call(
        db,
        "product.save",
        json!({"brandId":brand,"name":format!("虚构星砂收纳盒{suffix}（非在售）"),"sku":format!("FICTIONAL-P11-{suffix}"),"category":"虚构收纳用品","officialUrl":"","sourceText":PRODUCT,"privateNotes":PRODUCT,"facts":[{"id":"fixture:purpose","label":"虚构验收用途","value":"本地模拟流程测试，不出售","source":"虚构验收规格，非真实厂商声明","visibility":"public","validUntil":"2099-12-31"}]}),
    );
    let product = call(
        db,
        "product.confirm",
        json!({"brandId":brand,"id":product["id"],"expectedRevision":product["revision"]}),
    );
    let mut ids = vec![];
    for i in 0..questions {
        let q = call(
            db,
            "question.save",
            json!({"brandId":brand,"productId":product["id"],"text":format!("虚构桌面用品如何选择？{QUESTION}-{suffix}-{i}"),"intent":"selection","origin":"customer","source":"虚构本地软件验收问题","branded":false,"confirmed":false}),
        );
        call(
            db,
            "question.confirm",
            json!({"brandId":brand,"id":q["id"],"confirmed":true}),
        );
        ids.push(q["id"].clone());
    }
    let campaign = call(
        db,
        "campaign.create",
        json!({"brandId":brand,"name":format!("虚构模拟历史计划-{PLAN}-{suffix}"),"questionIds":ids,"platforms":platforms,"repetitions":reps,"modelLabel":"虚构模型（无模型调用）","searchMode":"模拟离线，未访问平台","context":format!("虚构本地演练-{CONTEXT}"),"evidenceMode":"simulated"}),
    );
    (product, campaign)
}
struct SampleSlot<'a> {
    question: usize,
    platform: &'a str,
    repetition: usize,
    phase: &'a str,
}
fn sample(
    db: &mut Connection,
    brand: &Value,
    campaign: &Value,
    slot: SampleSlot<'_>,
    rec: &str,
    answer: &str,
) -> Value {
    let SampleSlot {
        question: q,
        platform,
        repetition: rep,
        phase,
    } = slot;
    call(
        db,
        "observation.add",
        json!({"brandId":brand,"productId":campaign["questions"][q]["productId"],"questionId":campaign["questions"][q]["id"],"platform":platform,"surface":"consumer","phase":phase,"modelLabel":campaign["modelLabel"],"searchMode":campaign["searchMode"],"testedAt":if phase=="baseline"{"2026-10-24T01:00:00Z"}else{"2026-10-24T02:00:00Z"},"answer":answer,"mentioned":rec=="yes","accuracy":"unknown","citedUrl":URL,"evidenceNote":NOTE,"assessment":{"campaignId":campaign["id"],"repetition":rep,"context":campaign["context"],"recommendation":rec,"rank":null,"competitors":COMPETITOR}}),
    )
}
fn correct(
    db: &mut Connection,
    brand: &Value,
    row: &Value,
    revision: usize,
    yes: bool,
    note: &str,
) -> Value {
    call(
        db,
        "observation.assess",
        json!({"brandId":brand,"id":row["id"],"expectedRevision":revision,"mentioned":yes,"accuracy":"correct","citedUrl":URL,"evidenceNote":note,"recommendation":if yes{"yes"}else{"no"},"rank":if yes{json!(2)}else{Value::Null},"competitors":COMPETITOR}),
    )
}
fn checks(report: &Value, failed: Option<&str>) {
    assert_eq!(
        report["status"],
        if failed.is_some() {
            "blocked"
        } else {
            "passed"
        },
        "{report}"
    );
    for check in report["checks"].as_array().unwrap() {
        assert_eq!(
            check["status"],
            if Some(check["code"].as_str().unwrap()) == failed {
                "failed"
            } else {
                "passed"
            },
            "{report}"
        );
    }
}
fn safe_export(package: &Value) -> Value {
    let bytes = files::site_zip(package).unwrap();
    for marker in MARKERS {
        assert!(!bytes.windows(marker.len()).any(|w| w == marker.as_bytes()));
    }
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    assert_eq!(archive.len(), 2);
    let mut names = vec![];
    let mut report = Value::Null;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).unwrap();
        names.push(file.name().to_owned());
        let mut text = String::new();
        file.read_to_string(&mut text).unwrap();
        for marker in MARKERS {
            assert!(!text.contains(marker), "privacy marker leaked: {marker}");
        }
        for forbidden in [
            "\"answer\"",
            "\"evidenceNote\"",
            "\"competitors\"",
            "\"citedUrl\"",
            "\"citationHosts\"",
            "\"querySnapshot\"",
            "\"questions\"",
            "\"context\"",
            "evidence.json",
            "observations.csv",
        ] {
            assert!(
                !text.contains(forbidden),
                "raw field/file leaked: {forbidden}"
            );
        }
        if file.name() == "inspection.json" {
            report = serde_json::from_str(&text).unwrap();
        }
    }
    names.sort();
    assert_eq!(names, ["inspection.json", "report.md"]);
    assert!(!report.is_null());
    report
}
fn old_fixture(path: &Path, profile: &str, mode: &str) -> Value {
    let bytes = fs::read(path).unwrap();
    let name = path.file_name().unwrap().to_str().unwrap();
    let package = site_package::decode_experiment_package(name, &bytes).unwrap();
    let evidence: Value = serde_json::from_str(content(&package, "evidence.json")).unwrap();
    let brand = evidence["summary"]["campaign"]["brandId"].as_str().unwrap();
    let inspected = core::inspect_experiment_report(brand, &package).unwrap();
    checks(&inspected.report, None);
    assert_eq!(inspected.report["profile"], profile);
    assert_eq!(inspected.report["evidenceMode"], mode);
    assert_eq!(inspected.report["counts"]["observations"], 240);
    assert_eq!(
        safe_export(&core::experiment_inspection_export(&inspected.report).unwrap()),
        inspected.report
    );
    json!({"fileName":name,"sha256":digest(&bytes),"frozenBrandId":brand,"campaignId":inspected.report["campaignId"],"profile":profile,"evidenceMode":mode,"observations":inspected.observations.len(),"status":"passed","requiresCurrentDatabase":false})
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 2, "one NEW output directory required");
    // Resolve both required historical inputs before creating any fixture output.
    let legacy = required_fixture("MGEO_PHASE11_LEGACY");
    let current_legacy = required_fixture("MGEO_PHASE11_CURRENT_LEGACY");
    let output = Path::new(&args[1]);
    assert!(
        !output.exists(),
        "refuse to overwrite any output directory or database"
    );
    fs::create_dir_all(output.parent().unwrap()).unwrap();
    fs::create_dir(output).unwrap();
    let path = output.join("fictional.sqlite3");
    write(output, "fictional.sqlite3", &[]);
    let mut db = runtime::open_database(&path).unwrap();
    let brand = call(
        &mut db,
        "brand.create",
        json!({"name":"虚构历史实验室（仅模拟验收）"}),
    )["id"]
        .clone();
    let foreign = call(
        &mut db,
        "brand.create",
        json!({"name":"虚构隔离实验室（仅模拟验收）"}),
    )["id"]
        .clone();
    let (product, campaign) = seed_plan(&mut db, &brand, "A", 2, json!(["doubao", "yuanbao"]), 3);
    let mut target = Value::Null;
    for platform in ["doubao", "yuanbao"] {
        for q in 0..2 {
            for rep in 1..=3 {
                for phase in ["baseline", "followup"] {
                    if platform == "doubao" && q == 1 && rep == 3 && phase == "followup" {
                        continue;
                    }
                    let unknown = platform == "doubao" && q == 0 && rep == 2 && phase == "baseline";
                    let rec = if unknown {
                        "unknown"
                    } else if phase == "followup" {
                        "yes"
                    } else {
                        "no"
                    };
                    let answer = if platform == "doubao"
                        && q == 0
                        && rep == 1
                        && phase == "baseline"
                    {
                        RAW.to_owned()
                    } else if unknown {
                        "=SUM(1,2)\n\"模拟未知推荐\"\u{feff}".to_owned()
                    } else {
                        format!("【虚构模拟；非真实平台结果】{platform}/{q}/{rep}/{phase}\n\"原文引号\"\u{feff}  ")
                    };
                    let row = sample(
                        &mut db,
                        &brand,
                        &campaign,
                        SampleSlot {
                            question: q,
                            platform,
                            repetition: rep,
                            phase,
                        },
                        rec,
                        &answer,
                    );
                    assert_eq!(row["answer"], answer);
                    if platform == "doubao" && q == 0 && rep == 1 && phase == "baseline" {
                        target = row;
                    }
                }
            }
        }
    }
    let reviewed = correct(&mut db, &brand, &target, 1, true, NOTE);
    assert_eq!(reviewed["assessmentRevision"], 2);
    assert_eq!(
        reviewed["assessmentHistory"][0]["assessment"]["recommendation"],
        "no"
    );
    let request = json!({"brandId":brand,"campaignId":campaign["id"]});
    let original = call(&mut db, "campaign.report", request.clone());
    let frozen = artifact(output, "current-experiment.zip", &original);
    let frozen_bytes = fs::read(output.join("current-experiment.zip")).unwrap();
    let first = core::inspect_experiment_report(brand.as_str().unwrap(), &frozen).unwrap();
    checks(&first.report, None);
    assert_eq!(
        first.report["counts"],
        json!({"observations":23,"expectedPerPhase":12,"baseline":12,"followup":11,"missing":1,"reviewedObservations":1,"historyEntries":1})
    );
    let doubao = &first.report["platforms"][0];
    assert_eq!(doubao["baseline"]["assessed"], 5);
    assert_eq!(doubao["baseline"]["unknown"], 1);
    assert_eq!(doubao["baseline"]["recommended"], 1);
    assert_eq!(doubao["matchedPairs"], 5);
    assert_eq!(doubao["matchedAssessedPairs"], 4);
    assert_eq!(doubao["unmatchedSamples"], 1);
    assert_eq!(doubao["deltaPercentagePoints"], 75.0);
    assert_eq!(doubao["followup"]["recommended"], 5);
    assert_eq!(doubao["followup"]["ranked"], 0);
    assert_eq!(doubao["followup"]["top3"], 0);
    assert_eq!(first.report["platforms"][1]["deltaPercentagePoints"], 100.0);
    let csv = content(&frozen, "observations.csv");
    assert!(csv.contains("\r\n") && csv.contains('\u{feff}'));
    assert!(csv.contains("\"'  =SUM(1,2)") && csv.contains("\"'=SUM(1,2)"));
    assert!(csv.contains("\"\"P11_PRIVATE_RAW_5290\"\"\n<script>"));
    let frozen_row = first
        .observations
        .iter()
        .find(|r| r["id"] == target["id"])
        .unwrap();
    assert_eq!(frozen_row, &reviewed);
    assert_eq!(frozen_row["answer"], RAW);
    // A second real core correction changes live state after the ZIP was frozen.
    let live = correct(
        &mut db,
        &brand,
        &reviewed,
        2,
        false,
        "P11_PRIVATE_LIVE_NOTE_5290",
    );
    assert_eq!(live["assessmentRevision"], 3);
    assert_eq!(live["assessmentHistory"].as_array().unwrap().len(), 2);
    assert_ne!(
        call(&mut db, "campaign.report", request)["summary"],
        original["summary"]
    );
    let (_, foreign_campaign) = seed_plan(&mut db, &foreign, "B", 1, json!(["doubao"]), 1);
    sample(
        &mut db,
        &foreign,
        &foreign_campaign,
        SampleSlot {
            question: 0,
            platform: "doubao",
            repetition: 1,
            phase: "baseline",
        },
        "unknown",
        "【虚构隔离模拟】非真实平台结果",
    );
    let foreign_report = call(
        &mut db,
        "campaign.report",
        json!({"brandId":foreign,"campaignId":foreign_campaign["id"]}),
    );
    let wrong = artifact(output, "wrong-brand.zip", &foreign_report);
    checks(
        &core::inspect_experiment_report(foreign.as_str().unwrap(), &wrong)
            .unwrap()
            .report,
        None,
    );
    core::validate_database(&db).unwrap();
    for table in ["ai_jobs", "m_publications"] {
        assert_eq!(
            db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    let tables_before = table_hashes(&db);
    let schema = db
        .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
        .unwrap();
    drop(db);
    let bytes_before = fs::read(&path).unwrap();
    let hidden = output.join("fictional.sqlite3.unavailable");
    assert!(!hidden.exists());
    fs::rename(&path, &hidden).unwrap();
    assert!(
        !path.exists(),
        "source database unavailable during historical inspection"
    );
    // Everything below consumes only frozen ZIPs and in-memory sessions.
    let reread = site_package::decode_experiment_package(
        "current-experiment.zip",
        &fs::read(output.join("current-experiment.zip")).unwrap(),
    )
    .unwrap();
    let independent = core::inspect_experiment_report(brand.as_str().unwrap(), &reread).unwrap();
    assert_eq!(independent.report, first.report);
    assert_eq!(independent.observations, first.observations);
    let mut session = ExperimentInspectionSession::default();
    let generation = session.begin_selection();
    let selected = session
        .complete_selection(generation, brand.as_str().unwrap(), reread.clone())
        .unwrap();
    assert_eq!(selected["report"], first.report);
    let ticket = selected["inspectionId"].as_str().unwrap();
    let fp = selected["report"]["reportFingerprint"].as_str().unwrap();
    let page0 = session
        .read_page(brand.as_str().unwrap(), ticket, 0)
        .unwrap();
    let page1 = session
        .read_page(brand.as_str().unwrap(), ticket, 1)
        .unwrap();
    assert_eq!(page0["observations"].as_array().unwrap().len(), 20);
    assert_eq!(page1["observations"].as_array().unwrap().len(), 3);
    let paged: Vec<Value> = page0["observations"]
        .as_array()
        .unwrap()
        .iter()
        .chain(page1["observations"].as_array().unwrap())
        .cloned()
        .collect();
    assert_eq!(paged, independent.observations);
    assert!(session
        .read_page(brand.as_str().unwrap(), ticket, 2)
        .is_err());
    assert!(session
        .read_page(foreign.as_str().unwrap(), ticket, 0)
        .is_err());
    assert!(session
        .prepare_export(foreign.as_str().unwrap(), ticket, fp)
        .is_err());
    assert!(session
        .prepare_export(brand.as_str().unwrap(), ticket, &"0".repeat(64))
        .is_err());
    let reduced = session
        .prepare_export(brand.as_str().unwrap(), ticket, fp)
        .unwrap();
    assert_eq!(safe_export(&reduced), first.report);
    write(
        output,
        "inspection.zip",
        &files::site_zip(&reduced).unwrap(),
    );
    assert_eq!(
        fs::read(output.join("inspection.zip")).unwrap(),
        files::site_zip(&reduced).unwrap()
    );
    session.begin_selection(); // canceled replacement invalidates prior pages and exports
    assert!(session
        .read_page(brand.as_str().unwrap(), ticket, 0)
        .is_err());
    assert!(session
        .prepare_export(brand.as_str().unwrap(), ticket, fp)
        .is_err());
    assert!(ExperimentInspectionSession::default()
        .read_page(brand.as_str().unwrap(), ticket, 0)
        .is_err());
    let mut tampered = original.clone();
    let mut evidence: Value = serde_json::from_str(content(&tampered, "evidence.json")).unwrap();
    let old_summary = serde_json::to_string_pretty(&evidence["summary"]).unwrap();
    evidence["summary"]["baselineCount"] = json!(999);
    let new_summary = serde_json::to_string_pretty(&evidence["summary"]).unwrap();
    let md = content(&tampered, "report.md").replace(&old_summary, &new_summary);
    replace(
        &mut tampered,
        "evidence.json",
        serde_json::to_string_pretty(&evidence).unwrap(),
    );
    replace(&mut tampered, "report.md", md);
    let summary_report = core::inspect_experiment_report(
        brand.as_str().unwrap(),
        &decode("tampered-summary.zip", &tampered),
    )
    .unwrap()
    .report;
    checks(&summary_report, Some("summary"));
    let mut tampered = original.clone();
    replace(
        &mut tampered,
        "observations.csv",
        format!("{}tampered\r\n", content(&original, "observations.csv")),
    );
    let blocked = artifact(output, "blocked-companion.zip", &tampered);
    let blocked_report =
        core::inspect_experiment_report(brand.as_str().unwrap(), &blocked).unwrap();
    checks(&blocked_report.report, Some("csv"));
    assert_eq!(blocked_report.observations, first.observations);
    assert_eq!(
        safe_export(&core::experiment_inspection_export(&blocked_report.report).unwrap()),
        blocked_report.report
    );
    let mut tampered = original.clone();
    replace(
        &mut tampered,
        "report.md",
        format!(
            "{}unexpected trailing prose",
            content(&original, "report.md")
        ),
    );
    let markdown_report = core::inspect_experiment_report(
        brand.as_str().unwrap(),
        &decode("tampered-markdown.zip", &tampered),
    )
    .unwrap()
    .report;
    checks(&markdown_report, Some("markdown"));
    let wrong_error = core::inspect_experiment_report(brand.as_str().unwrap(), &wrong)
        .err()
        .unwrap();
    assert_eq!(wrong_error, "实验包证据无效、归属不符或超过处理上限");
    for marker in MARKERS {
        assert!(!wrong_error.contains(marker));
    }
    let mut extra = original.clone();
    extra["files"]
        .as_array_mut()
        .unwrap()
        .push(json!({"path":"extra.txt","content":"unrequested"}));
    assert!(site_package::decode_experiment_package(
        "extra-file.zip",
        &files::site_zip(&extra).unwrap()
    )
    .is_err());
    let actual_legacy = old_fixture(&legacy, "legacy-v1", "unspecified");
    let actual_current = old_fixture(&current_legacy, "current-v1", "simulated");
    assert_eq!(
        fs::read(output.join("current-experiment.zip")).unwrap(),
        frozen_bytes
    );
    assert!(!path.exists());
    fs::rename(&hidden, &path).unwrap();
    assert_eq!(
        fs::read(&path).unwrap(),
        bytes_before,
        "inspection/export changed DB bytes"
    );
    // Validation must not run the application's writing database initializer.
    let db = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let tables_after = table_hashes(&db);
    assert_eq!(
        tables_after, tables_before,
        "inspection/export changed a database table"
    );
    core::validate_database(&db).unwrap();
    assert_eq!(
        db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        schema
    );
    drop(db);
    let database_sha256_before = digest(&bytes_before);
    let database_sha256_after = digest(&fs::read(&path).unwrap());
    assert_eq!(
        database_sha256_after, database_sha256_before,
        "final read-only validation changed DB bytes"
    );
    let mut artifacts = json!({});
    for name in [
        "fictional.sqlite3",
        "current-experiment.zip",
        "blocked-companion.zip",
        "wrong-brand.zip",
        "inspection.zip",
    ] {
        let bytes = fs::read(output.join(name)).unwrap();
        artifacts[name] = json!({"bytes":bytes.len(),"sha256":digest(&bytes)});
    }
    save(
        output,
        "acceptance.json",
        &json!({"status":"passed","fixture":"fictional-local-simulated-only","nativeAcceptance":"pending-controller","brandId":brand,"foreignBrandId":foreign,"campaignId":campaign["id"],"foreignCampaignId":foreign_campaign["id"],"productId":product["id"],"correctedObservationId":target["id"],"frozenAssessmentRevision":2,"liveAssessmentRevision":3,"observations":23,"databaseTotalObservations":24,"pages":[20,3],"schema":schema,"inspection":first.report,"blockedInspection":blocked_report.report,"tamperChecks":{"summary":summary_report["checks"],"csv":blocked_report.report["checks"],"markdown":markdown_report["checks"]},"historicalFixtures":[actual_legacy,actual_current],"readonly":{"sourceDatabaseUnavailable":true,"databaseSha256Before":database_sha256_before,"databaseSha256After":database_sha256_after,"allTableHashesBefore":tables_before,"allTableHashesAfter":tables_after},"artifacts":artifacts,"modelJobs":0,"networkCalls":0,"platformSubmissions":0,"publications":0,"checks":["real-core-confirmed-product-questions-and-simulated-campaign","unknown-and-missing-pair-denominators","actual-two-corrections-freeze-first","raw-feff-formula-quotes-multiline-whitespace-preserved","two-pages-20-plus-3-with-history","historical-review-with-source-db-unavailable","deterministic-reinspection-after-live-correction","all-database-table-hashes-and-bytes-unchanged","actual-phase2-and-phase3-archives-required","summary-csv-markdown-independently-blocked","wrong-brand-generic-rejection","extra-file-rejected","reduced-export-actual-zip-readback-private-markers-excluded","canceled-selection-and-restart-invalidates-ticket"]}),
    );
    println!("Phase11 acceptance passed: 23 simulated rows, frozen revision 2/live revision 3, actual Phase2/3 ZIPs, 20+3 pages, DB-unavailable deterministic review and safe ZIP readback; zero network/model/publishing. Native acceptance pending controller.");
}
