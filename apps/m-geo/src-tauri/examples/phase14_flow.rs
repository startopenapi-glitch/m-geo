//! Disposable real-core Phase14 fixture. All evidence is fictional, local and simulated.
use m_geo_lib::{files, runtime};
use rusqlite::{types::ValueRef, Connection};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
};

const RAW: &str = "  =SUM(1,2)\u{feff}\"P14_PRIVATE_RAW_8019\"\n<script>P14_INERT_8019</script>  ";
const NOTE: &str = "  P14_PRIVATE_NOTE_8019\n原始备注保持换行  ";
const COMPETITOR: &str = "P14_PRIVATE_COMPETITOR_8019";
const URL: &str = "https://example.com/P14_PRIVATE_URL_8019";
const MARKERS: [&str; 6] = [
    "P14_PRIVATE_RAW_8019",
    "P14_INERT_8019",
    "P14_PRIVATE_NOTE_8019",
    COMPETITOR,
    "P14_PRIVATE_URL_8019",
    "P14_PRIVATE_CORRECTED_8019",
];
const QUESTION: &str = "=SUM(1,2) 虚构收纳用品如何选择？<示例> \"引号\"\n第二行（无真实平台结果）";

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
fn save(dir: &Path, name: &str, v: &Value) {
    write(dir, name, &serde_json::to_vec_pretty(v).unwrap());
}
fn guarded(report: &Value) -> Value {
    json!({"brandId":report["brandId"],"campaignId":report["campaign"]["id"],"reportFingerprint":report["reportFingerprint"]})
}
fn lookup(report: &Value, row: &Value) -> Value {
    let mut p = guarded(report);
    for k in ["questionId", "platform", "repetition"] {
        p[k] = row[k].clone();
    }
    p
}
fn report(db: &mut Connection, brand: &Value, campaign: &Value) -> Value {
    call(
        db,
        "campaign.transitions",
        json!({"brandId":brand,"campaignId":campaign["id"]}),
    )
}
fn source(db: &Connection, brand: &Value, campaign: &Value) -> Value {
    let b = brand.as_str().unwrap();
    let cid = campaign["id"].as_str().unwrap();
    let raw: String = db
        .query_row(
            "SELECT data FROM m_campaigns WHERE id=?1 AND brand_id=?2",
            [cid, b],
            |r| r.get(0),
        )
        .unwrap();
    let plan: Value = serde_json::from_str(&raw).unwrap();
    let observations: Vec<Value> = db.prepare("SELECT data FROM m_observations WHERE brand_id=?1 AND json_extract(data,'$.assessment.campaignId')=?2 ORDER BY id").unwrap().query_map([b,cid],|r|r.get::<_,String>(0)).unwrap().map(|r|serde_json::from_str(&r.unwrap()).unwrap()).collect();
    let mut ids = BTreeSet::new();
    for q in plan["questions"].as_array().unwrap() {
        ids.insert(q["productId"].as_str().unwrap().to_owned());
    }
    let products: Vec<Value> = ids
        .into_iter()
        .map(|pid| {
            let raw: String = db
                .query_row(
                    "SELECT data FROM m_products WHERE id=?1 AND brand_id=?2",
                    [pid.as_str(), b],
                    |r| r.get(0),
                )
                .unwrap();
            let p: Value = serde_json::from_str(&raw).unwrap();
            json!({"productId":p["id"],"name":p["name"],"sku":p["sku"]})
        })
        .collect();
    json!({"campaign":plan,"observations":observations,"products":products})
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
        result[name] =
            json!({"rows":rows.len(),"sha256":digest(&serde_json::to_vec(&rows).unwrap())});
    }
    result
}
fn checkpoint(db: &Connection, path: &Path, source: &Value) -> Value {
    json!({"databaseSha256":digest(&fs::read(path).unwrap()),"sourceSha256":digest(&serde_json::to_vec(source).unwrap()),"tableHashes":table_hashes(db),"schemaVersion":db.pragma_query_value::<i64,_>(None,"user_version",|r|r.get(0)).unwrap(),"totalChanges":db.total_changes()})
}
fn product(db: &mut Connection, brand: &Value, name: &str, sku: &str) -> Value {
    let p = call(
        db,
        "product.save",
        json!({"brandId":brand,"name":name,"sku":sku,"category":"虚构收纳用品","officialUrl":"","sourceText":"P14_PRIVATE_PRODUCT_8019","privateNotes":"P14_PRIVATE_PRODUCT_8019","facts":[{"id":"fixture:purpose","label":"虚构验收用途","value":"本地模拟，不出售","source":"虚构本地规格","visibility":"public","validUntil":"2099-12-31"}]}),
    );
    call(
        db,
        "product.confirm",
        json!({"brandId":brand,"id":p["id"],"expectedRevision":p["revision"]}),
    )
}
fn question(db: &mut Connection, brand: &Value, product: &Value, text: &str) -> Value {
    let q = call(
        db,
        "question.save",
        json!({"brandId":brand,"productId":product["id"],"text":text,"intent":"selection","origin":"customer","source":"虚构本地问题，不代表真实消费者反馈","branded":false,"confirmed":false}),
    );
    call(
        db,
        "question.confirm",
        json!({"brandId":brand,"id":q["id"],"confirmed":true}),
    )
}
fn campaign(
    db: &mut Connection,
    brand: &Value,
    name: &str,
    questions: &[Value],
    reps: usize,
) -> Value {
    let ids: Vec<_> = questions.iter().map(|q| q["id"].clone()).collect();
    call(
        db,
        "campaign.create",
        json!({"brandId":brand,"name":name,"questionIds":ids,"platforms":["doubao","yuanbao"],"repetitions":reps,"modelLabel":"虚构模型标签（从未调用）","searchMode":"离线模拟，无平台访问","context":"本地虚构推荐变化验收；相同问题按商品ID隔离","evidenceMode":"simulated"}),
    )
}
struct Slot<'a> {
    q: usize,
    platform: &'a str,
    rep: usize,
    phase: &'a str,
    rec: &'a str,
}
fn sample(db: &mut Connection, brand: &Value, campaign: &Value, s: Slot<'_>) -> Value {
    let answer = format!(
        "{RAW}\n计划={}；q={}；平台={}；轮次={}；阶段={}",
        campaign["id"], s.q, s.platform, s.rep, s.phase
    );
    let rank = if s.rec == "yes" && s.phase == "followup" && s.rep == 2 {
        json!(3)
    } else {
        Value::Null
    };
    call(
        db,
        "observation.add",
        json!({"brandId":brand,"productId":campaign["questions"][s.q]["productId"],"questionId":campaign["questions"][s.q]["id"],"platform":s.platform,"surface":"consumer","phase":s.phase,"modelLabel":campaign["modelLabel"],"searchMode":campaign["searchMode"],"testedAt":if s.phase=="baseline"{"2026-10-24T01:00:00Z"}else{"2026-10-24T02:00:00Z"},"answer":answer,"mentioned":s.rec=="yes","accuracy":"unknown","citedUrl":if s.rep==1{URL}else{""},"evidenceNote":NOTE,"assessment":{"campaignId":campaign["id"],"repetition":s.rep,"context":campaign["context"],"recommendation":s.rec,"rank":rank,"competitors":COMPETITOR}}),
    )
}
fn conservation(m: &Value) {
    let n = |k: &str| m[k].as_u64().unwrap();
    assert_eq!(
        n("total"),
        [
            "gained",
            "lost",
            "retainedYes",
            "retainedNo",
            "unknownPair",
            "baselineOnly",
            "followupOnly",
            "missingBoth"
        ]
        .iter()
        .map(|k| n(k))
        .sum::<u64>()
    );
    assert_eq!(n("paired"), n("assessedPairs") + n("unknownPair"));
    assert_eq!(n("baselineSamples"), n("paired") + n("baselineOnly"));
    assert_eq!(n("followupSamples"), n("paired") + n("followupOnly"));
    assert_eq!(n("baselineRecommended"), n("lost") + n("retainedYes"));
    assert_eq!(n("followupRecommended"), n("gained") + n("retainedYes"));
}
fn validate_initial(r: &Value, a: &Value, b: &Value) {
    let s = &r["summary"];
    for (k, n) in [
        ("total", 8),
        ("baselineSamples", 6),
        ("followupSamples", 6),
        ("paired", 5),
        ("assessedPairs", 4),
        ("baselineRecommended", 2),
        ("followupRecommended", 2),
    ] {
        assert_eq!(s[k], n, "{k}");
    }
    for k in [
        "gained",
        "lost",
        "retainedYes",
        "retainedNo",
        "unknownPair",
        "baselineOnly",
        "followupOnly",
        "missingBoth",
    ] {
        assert_eq!(s[k], 1, "{k}");
    }
    assert_eq!(s["deltaPercentagePoints"], 0.0);
    conservation(s);
    assert_eq!(r["products"].as_array().unwrap().len(), 2);
    for (id, delta) in [(a["id"].clone(), 50.0), (b["id"].clone(), -50.0)] {
        let g = r["products"]
            .as_array()
            .unwrap()
            .iter()
            .find(|g| g["productId"] == id)
            .unwrap();
        assert_eq!(g["metrics"]["assessedPairs"], 2);
        assert_eq!(g["metrics"]["deltaPercentagePoints"], delta);
    }
    for k in ["products", "questions", "platforms"] {
        let groups = r[k].as_array().unwrap();
        assert_eq!(groups.len(), 2);
        assert_eq!(
            groups
                .iter()
                .map(|g| g["metrics"]["total"].as_u64().unwrap())
                .sum::<u64>(),
            8
        );
        for g in groups {
            conservation(&g["metrics"]);
        }
    }
    let states: BTreeSet<_> = r["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["transition"].as_str().unwrap())
        .collect();
    assert_eq!(states.len(), 8);
}
fn evidence_all(db: &mut Connection, r: &Value, src: &Value) -> Value {
    let mut all = vec![];
    for row in r["rows"].as_array().unwrap() {
        let e = call(db, "campaign.transitionEvidence", lookup(r, row));
        assert_eq!(e["row"], *row);
        for side in ["baseline", "followup"] {
            if row[side].is_null() {
                assert!(e[side].is_null());
                continue;
            }
            let original = src["observations"]
                .as_array()
                .unwrap()
                .iter()
                .find(|o| o["id"] == row[side]["observationId"])
                .unwrap();
            assert_eq!(e[side], *original);
            assert!(e[side]["answer"].as_str().unwrap().starts_with(RAW));
            assert_eq!(
                row[side]["assessmentRevision"],
                original
                    .get("assessmentRevision")
                    .cloned()
                    .unwrap_or(json!(1))
            );
            assert_eq!(row[side]["rank"], original["assessment"]["rank"]);
            assert_eq!(
                row[side]["cited"],
                !original["citedUrl"].as_str().unwrap().is_empty()
            );
        }
        all.push(e);
    }
    json!(all)
}
fn archive(db: &mut Connection, r: &Value, dir: &Path, name: &str) -> Value {
    let package = call(db, "campaign.transitionExport", guarded(r));
    assert_eq!(package["fileName"], "recommendation-transitions.zip");
    let bytes = files::site_zip(&package).unwrap();
    write(dir, name, &bytes);
    let mut z = zip::ZipArchive::new(Cursor::new(fs::read(dir.join(name)).unwrap())).unwrap();
    assert_eq!(z.len(), 5);
    let mut names = vec![];
    let mut entries = vec![];
    for i in 0..z.len() {
        let mut f = z.by_index(i).unwrap();
        let path = f.name().to_owned();
        let mut text = String::new();
        f.read_to_string(&mut text).unwrap();
        for marker in MARKERS.into_iter().chain(["P14_PRIVATE_PRODUCT_8019"]) {
            assert!(
                !text.contains(marker),
                "private marker leaked in {path}: {marker}"
            );
        }
        if path == "transition-report.json" {
            assert_eq!(serde_json::from_str::<Value>(&text).unwrap(), *r);
        }
        if path == "products.csv" {
            assert!(text.contains("\"'=SUM(1,2)"));
        }
        if path == "questions.csv" {
            assert!(text.contains("\"'=SUM(1,2)"));
        }
        names.push(path.clone());
        entries.push(json!({"path":path,"bytes":text.len(),"sha256":digest(text.as_bytes())}));
    }
    names.sort();
    assert_eq!(
        names,
        [
            "pairs.csv",
            "products.csv",
            "questions.csv",
            "report.md",
            "transition-report.json"
        ]
    );
    json!({"path":dir.join(name),"sha256":digest(&bytes),"bytes":bytes.len(),"files":entries})
}
fn readonly_round(
    db: &mut Connection,
    path: &Path,
    brand: &Value,
    campaign: &Value,
    dir: &Path,
    label: &str,
) -> (Value, Value, Value) {
    let src = source(db, brand, campaign);
    let before = checkpoint(db, path, &src);
    let r = report(db, brand, campaign);
    assert_eq!(before["sourceSha256"], r["evidenceFingerprint"]);
    let repeated = report(db, brand, campaign);
    assert_eq!(r, repeated);
    let evidence = evidence_all(db, &r, &src);
    let zip = archive(
        db,
        &r,
        dir,
        &format!("{label}-recommendation-transitions.zip"),
    );
    let current = source(db, brand, campaign);
    let after = checkpoint(db, path, &current);
    assert_eq!(src, current);
    assert_eq!(before, after);
    save(dir, &format!("{label}-report.json"), &r);
    save(dir, &format!("{label}-evidence.json"), &evidence);
    (
        r,
        src,
        json!({"before":before,"after":after,"reportFingerprint":repeated["reportFingerprint"],"evidenceFingerprint":repeated["evidenceFingerprint"],"zip":zip}),
    )
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(
        args.len(),
        2,
        "usage: phase14_flow NEW_ABSOLUTE_OUTPUT_DIRECTORY"
    );
    let dir = PathBuf::from(&args[1]);
    assert!(dir.is_absolute(), "output directory must be absolute");
    assert!(!dir.exists(), "refuse to reuse fixture output directory");
    fs::create_dir_all(dir.parent().unwrap()).unwrap();
    fs::create_dir(&dir).unwrap();
    let path = dir.join("fictional.sqlite3");
    let mut db = runtime::open_database(&path).unwrap();
    let brand = call(
        &mut db,
        "brand.create",
        json!({"name":"虚构Phase14推荐变化实验室（仅本地模拟）"}),
    )["id"]
        .clone();
    let foreign = call(
        &mut db,
        "brand.create",
        json!({"name":"虚构Phase14隔离品牌（仅本地模拟）"}),
    )["id"]
        .clone();
    let a = product(
        &mut db,
        &brand,
        "=SUM(1,2) 虚构星砂收纳盒A <示例>",
        "P14-FICTIONAL-A",
    );
    let b = product(
        &mut db,
        &brand,
        "+虚构月砂收纳盒B \"引号\"",
        "P14-FICTIONAL-B",
    );
    let qa = question(&mut db, &brand, &a, QUESTION);
    let qb = question(&mut db, &brand, &b, QUESTION);
    assert_ne!(qa["id"], qb["id"]);
    assert_eq!(qa["text"], qb["text"]);
    let exact = campaign(
        &mut db,
        &brand,
        "Phase14 八状态精确验收（8单元）",
        &[qa.clone(), qb.clone()],
        2,
    );
    let mut correction_target = Value::Null;
    for (q, platform, rep, base, follow) in [
        (0, "doubao", 1, Some("no"), Some("yes")),
        (0, "doubao", 2, Some("yes"), Some("yes")),
        (0, "yuanbao", 1, Some("yes"), None),
        (0, "yuanbao", 2, None, None),
        (1, "doubao", 1, Some("yes"), Some("no")),
        (1, "doubao", 2, Some("no"), Some("no")),
        (1, "yuanbao", 1, Some("unknown"), Some("yes")),
        (1, "yuanbao", 2, None, Some("yes")),
    ] {
        for (phase, rec) in [("baseline", base), ("followup", follow)] {
            if let Some(rec) = rec {
                let o = sample(
                    &mut db,
                    &brand,
                    &exact,
                    Slot {
                        q,
                        platform,
                        rep,
                        phase,
                        rec,
                    },
                );
                if q == 0 && platform == "doubao" && rep == 1 && phase == "baseline" {
                    correction_target = o;
                }
            }
        }
    }
    let mut qs = vec![];
    for i in 0..21 {
        let text=format!("=SUM(1,2) 本地虚构分页问题{i:02}：如何选择可重复使用的桌面收纳用品？{} <示例> \"长文本\"\n第二行", "仅用于横向滚动验收，无真实平台结果。".repeat(12));
        qs.push(question(
            &mut db,
            &brand,
            if i % 2 == 0 { &a } else { &b },
            &text,
        ));
    }
    let ui = campaign(
        &mut db,
        &brand,
        "Phase14 原生分页与横向滚动（42单元）",
        &qs,
        1,
    );
    for q in 0..21 {
        for platform in ["doubao", "yuanbao"] {
            for phase in ["baseline", "followup"] {
                let rec = match q % 3 {
                    0 => {
                        if phase == "baseline" {
                            "no"
                        } else {
                            "yes"
                        }
                    }
                    1 => {
                        if phase == "baseline" {
                            "yes"
                        } else {
                            "no"
                        }
                    }
                    _ => "unknown",
                };
                sample(
                    &mut db,
                    &brand,
                    &ui,
                    Slot {
                        q,
                        platform,
                        rep: 1,
                        phase,
                        rec,
                    },
                );
            }
        }
    }
    let fp = product(&mut db, &foreign, "虚构独立隔离商品", "P14-FICTIONAL-A");
    let fq = question(&mut db, &foreign, &fp, QUESTION);
    let fc = campaign(&mut db, &foreign, "Phase14 第二品牌隔离计划", &[fq], 1);
    sample(
        &mut db,
        &foreign,
        &fc,
        Slot {
            q: 0,
            platform: "doubao",
            rep: 1,
            phase: "baseline",
            rec: "no",
        },
    );
    sample(
        &mut db,
        &foreign,
        &fc,
        Slot {
            q: 0,
            platform: "doubao",
            rep: 1,
            phase: "followup",
            rec: "yes",
        },
    );
    let (before, src_before, proof_before) =
        readonly_round(&mut db, &path, &brand, &exact, &dir, "before-correction");
    validate_initial(&before, &a, &b);
    let mut cross = guarded(&before);
    cross["brandId"] = foreign.clone();
    assert!(runtime::core_dispatch(&mut db, "campaign.transitionExport", cross.clone()).is_err());
    cross.as_object_mut().unwrap().remove("reportFingerprint");
    assert!(runtime::core_dispatch(&mut db, "campaign.transitions", cross).is_err());
    let (ui_report, ui_source, ui_proof) =
        readonly_round(&mut db, &path, &brand, &ui, &dir, "pagination");
    assert_eq!(ui_report["summary"]["total"], 42);
    assert_eq!(ui_report["questions"].as_array().unwrap().len(), 21);
    let foreign_report = report(&mut db, &foreign, &fc);
    assert_eq!(foreign_report["summary"]["total"], 2);
    assert!(before["rows"]
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r["productId"] != fp["id"]));
    let corrected = call(
        &mut db,
        "observation.assess",
        json!({"brandId":brand,"id":correction_target["id"],"expectedRevision":1,"mentioned":true,"accuracy":"correct","citedUrl":URL,"evidenceNote":"  P14_PRIVATE_CORRECTED_8019\n更正后仍保留原始回答  ","recommendation":"yes","rank":null,"competitors":COMPETITOR}),
    );
    assert_eq!(corrected["assessmentRevision"], 2);
    assert_eq!(corrected["answer"], correction_target["answer"]);
    assert_eq!(
        corrected["assessmentHistory"][0]["assessment"],
        correction_target["assessment"]
    );
    let old_row = before["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["baseline"]["observationId"] == correction_target["id"])
        .unwrap();
    let stale_evidence = runtime::core_dispatch(
        &mut db,
        "campaign.transitionEvidence",
        lookup(&before, old_row),
    )
    .unwrap_err();
    let stale_export =
        runtime::core_dispatch(&mut db, "campaign.transitionExport", guarded(&before)).unwrap_err();
    assert!(stale_evidence.contains("重新计算"));
    assert!(stale_export.contains("重新计算"));
    let (after, src_after, proof_after) =
        readonly_round(&mut db, &path, &brand, &exact, &dir, "after-correction");
    assert_ne!(before["reportFingerprint"], after["reportFingerprint"]);
    assert_ne!(before["evidenceFingerprint"], after["evidenceFingerprint"]);
    assert_eq!(after["summary"]["deltaPercentagePoints"], -25.0);
    assert_eq!(after["summary"]["gained"], 0);
    assert_eq!(after["summary"]["retainedYes"], 2);
    conservation(&after["summary"]);
    let current = src_after["observations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["id"] == corrected["id"])
        .unwrap();
    assert_eq!(*current, corrected);
    save(
        &dir,
        "source-observations.json",
        &json!({"before":src_before,"after":src_after}),
    );
    save(&dir, "pagination-source-observations.json", &ui_source);
    save(&dir, "foreign-report.json", &foreign_report);
    let before_reopen_hash = digest(&fs::read(&path).unwrap());
    let final_tables = table_hashes(&db);
    drop(db);
    let mut reopened = runtime::open_database(&path).unwrap();
    // Runtime startup initializes metadata/user_version; scope byte immutability
    // to the subsequent read operations, while requiring logical source identity
    // across the actual startup path.
    let after_startup_hash = digest(&fs::read(&path).unwrap());
    let reopened_source = source(&reopened, &brand, &exact);
    let reopened_checkpoint = checkpoint(&reopened, &path, &reopened_source);
    assert_eq!(reopened_source, src_after);
    assert_eq!(report(&mut reopened, &brand, &exact), after);
    assert_eq!(report(&mut reopened, &brand, &ui), ui_report);
    assert_eq!(table_hashes(&reopened), final_tables);
    assert_eq!(
        checkpoint(&reopened, &path, &source(&reopened, &brand, &exact)),
        reopened_checkpoint
    );
    drop(reopened);
    let final_hash = digest(&fs::read(&path).unwrap());
    assert_eq!(final_hash, after_startup_hash);
    let mut artifacts = vec![];
    for name in [
        "fictional.sqlite3",
        "source-observations.json",
        "pagination-source-observations.json",
        "before-correction-report.json",
        "after-correction-report.json",
        "pagination-report.json",
        "foreign-report.json",
        "before-correction-evidence.json",
        "after-correction-evidence.json",
        "pagination-evidence.json",
    ] {
        let bytes = fs::read(dir.join(name)).unwrap();
        artifacts.push(json!({"path":dir.join(name),"bytes":bytes.len(),"sha256":digest(&bytes)}));
    }
    save(
        &dir,
        "acceptance.json",
        &json!({"status":"passed","fixture":"fictional-local-simulated-only","nativeAcceptance":"pending-controller","databasePath":path,"brandId":brand,"brandName":"虚构Phase14推荐变化实验室（仅本地模拟）","foreignBrandId":foreign,"foreignCampaignId":fc["id"],"productAId":a["id"],"productBId":b["id"],"campaignId":exact["id"],"campaignName":exact["name"],"questionAId":qa["id"],"questionBId":qb["id"],"paginationCampaignId":ui["id"],"paginationCampaignName":ui["name"],"pagination":{"units":42,"questionGroups":21,"pairPages":[20,20,2],"questionPages":[20,1]},"correctedObservationId":corrected["id"],"beforeReportFile":"before-correction-report.json","afterReportFile":"after-correction-report.json","sourceFile":"source-observations.json","paginationReportFile":"pagination-report.json","beforeMetrics":before["summary"],"afterMetrics":after["summary"],"expectedProductDeltasBefore":{"productA":50,"productB":-50},"readonly":{"beforeCorrection":proof_before,"afterCorrection":proof_after,"pagination":ui_proof},"staleRejections":{"evidence":stale_evidence,"export":stale_export},"reopen":{"reportUnchanged":true,"databaseSha256":final_hash,"allTablesUnchanged":true,"sourceUnchanged":true,"databaseSha256BeforeStartup":before_reopen_hash,"databaseSha256AfterStartup":after_startup_hash,"readonlyCheckpoint":reopened_checkpoint,"startupNote":"runtime initialization rewrites schema metadata/user_version; byte comparison scopes subsequent readonly report calls"},"artifacts":artifacts,"privateMarkers":MARKERS,"nativeMenuLabels":["AI表现","逐商品与问题推荐变化","计算推荐变化","导出全计划变化 ZIP","查看两期原始证据"],"checks":["all-data-created-through-existing-actions","two-products-identical-question-text-exact-eight-states","six-baseline-six-followup-five-paired-four-assessed","product-plus50-minus50-hidden-whole-zero","eight-local-evidence-lookups-preserve-original-json-null-sides-rank-and-version","five-file-real-zip-readback-no-raw-private-markers","formula-like-name-question-export-safety","42-unit-21-question-native-pagination-fixture","second-brand-isolation","full-source-table-and-database-readonly-fingerprints","assessment-correction-rejects-old-evidence-and-export-token","new-report-revision2-history-original-answer-unchanged","close-reopen-no-persisted-report-state"],"modelJobs":0,"networkCalls":0,"platformSubmissions":0,"publications":0}),
    );
    println!("Phase14 real-core acceptance passed: exact8 states; +50/-50 products, whole0; baseline6/followup6,paired5/assessed4;42 pagination units;21 question groups;all8 evidence;3 actual five-file ZIPs;stale guards and revision2;source/tables/DB readonly and reopen stable. Native acceptance pending controller. Output: {}",dir.display());
}
