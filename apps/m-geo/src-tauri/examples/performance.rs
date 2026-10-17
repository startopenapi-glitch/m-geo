use m_geo_lib::{core, runtime};
use serde_json::json;
use std::time::Instant;
fn main() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("benchmark.sqlite3");
    let started = Instant::now();
    let mut db = runtime::open_database(&path).unwrap();
    let open_ms = started.elapsed().as_secs_f64() * 1000.;
    let b = core::handle(&mut db, "brand.create", json!({"name":"性能隔离品牌"})).unwrap();
    let mut imports = vec![];
    for batch in 0..5 {
        let mut csv = "品牌,商品名称,SKU,类目,商品链接\n".to_string();
        for i in 0..200 {
            let n = batch * 200 + i;
            csv.push_str(&format!(
                "性能隔离品牌,商品{n},SKU-{n},家居,https://example.com/item/{n}\n"
            ));
        }
        let now = Instant::now();
        core::handle(
            &mut db,
            "import.csv",
            json!({"brandId":b["id"],"content":csv,"confirm":true}),
        )
        .unwrap();
        imports.push(now.elapsed().as_secs_f64() * 1000.);
    }
    let mut snapshots = vec![];
    for _ in 0..10 {
        let now = Instant::now();
        let s = core::handle(&mut db, "workspace.snapshot", json!({"brandId":b["id"]})).unwrap();
        assert_eq!(s["products"].as_array().unwrap().len(), 1000);
        snapshots.push(now.elapsed().as_secs_f64() * 1000.);
    }
    let now = Instant::now();
    core::validate_database(&db).unwrap();
    let validation_ms = now.elapsed().as_secs_f64() * 1000.;
    println!(
        "{}",
        json!({"profile":"debug, real SQLite on disposable local disk","first_database_open_ms":open_ms,"import_200_rows_ms":imports,"snapshot_1000_products_ms":snapshots,"backup_business_validation_1000_ms":validation_ms,"not_measured":["native cold first paint","Windows","Intel","frontend 1000-row paint"]})
    );
}
