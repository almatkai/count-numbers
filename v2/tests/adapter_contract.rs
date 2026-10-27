//! Compile the production wrapper against the byte-for-byte pinned beta CoreContext
//! AND SDK. Only Tauri AppHandle/Emitter are substituted; all paths are temporary.
extern crate self as tauri;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};
use tezbar_count_numbers_v2::{handle, Host, HostFuture, StoreChange};

#[derive(Clone)]
pub struct AppHandle;
pub trait Emitter {
    fn emit(&self, channel: &str, payload: Value) -> std::result::Result<(), String>;
}
impl Emitter for AppHandle {
    fn emit(&self, _: &str, _: Value) -> std::result::Result<(), String> {
        panic!("No real Tauri app allowed")
    }
}
mod native_core {
    #[allow(dead_code, clippy::all)]
    pub mod common {
        include!("fixtures/beta_common.rs");
    }
}
#[path = "../tezbar_adapter.rs"]
mod adapter;
#[allow(dead_code, clippy::all)]
#[path = "fixtures/beta_v2_sdk.rs"]
mod v2_sdk;
use native_core::common::{CoreContext, CorePaths};
const SLUG: &str = "count-numbers";
type RequestLog = Arc<Mutex<Vec<(String, Vec<Value>)>>>;
fn context(dir: &std::path::Path) -> CoreContext {
    context_recording(dir).0
}
fn context_recording(dir: &std::path::Path) -> (CoreContext, RequestLog) {
    let ctx = CoreContext::headless(CorePaths {
        data_dir: dir.join("data"),
        config_dir: dir.join("config"),
        resource_dir: dir.join("resources"),
        temp_dir: dir.join("tmp"),
        home_dir: dir.join("home"),
    });
    // A recording router rejects OS effects instead of touching any real app.
    let calls = Arc::new(Mutex::new(Vec::new()));
    let recording = calls.clone();
    ctx.set_router(Arc::new(move |route, args| {
        let recording = recording.clone();
        Box::pin(async move {
            recording.lock().unwrap().push((route.clone(), args));
            Err(format!("Unexpected effect: {route}"))
        })
    }))
    .unwrap();
    (ctx, calls)
}

async fn run(ctx: &CoreContext) -> (Value, Value) {
    let mut s = json!({"futureSessionKey":{"keep":true}});
    let v = adapter::handle(
        ctx,
        "count-numbers",
        "extension:run-command",
        &json!({}),
        &mut s,
    )
    .await
    .unwrap();
    (s, v)
}

#[tokio::test]
async fn disposing_and_relaunching_never_reuses_an_old_action_id() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = context(dir.path());
    let (mut state, first) = run(&ctx).await;
    adapter::handle(
        &ctx,
        "count-numbers",
        "extension:dispose-session",
        &json!({}),
        &mut state,
    )
    .await
    .unwrap();
    let reopened = adapter::handle(
        &ctx,
        "count-numbers",
        "extension:run-command",
        &json!({}),
        &mut state,
    )
    .await
    .unwrap();
    assert_ne!(action(&first, "Count"), action(&reopened, "Count"));
    assert!(invoke(&ctx, &mut state, &first, "Count", Value::Null)
        .await
        .is_err());
    assert_eq!(stored(&ctx).await["data"]["counters"][0]["count"], 0);
    invoke(&ctx, &mut state, &reopened, "Count", Value::Null)
        .await
        .unwrap();
    assert_eq!(stored(&ctx).await["data"]["counters"][0]["count"], 1.0);
}
fn action(view: &Value, title: &str) -> String {
    view["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["title"] == title)
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned()
}
async fn invoke(
    ctx: &CoreContext,
    s: &mut Value,
    v: &Value,
    title: &str,
    values: Value,
) -> std::result::Result<Value, String> {
    adapter::handle(
        ctx,
        "count-numbers",
        "extension:invoke-action",
        &json!({"actionId":action(v,title),"formValues":values}),
        s,
    )
    .await
}
async fn stored(ctx: &CoreContext) -> Value {
    v2_sdk::read_store(ctx, SLUG).await.unwrap()
}
async fn seed(ctx: &CoreContext, data: Value) {
    v2_sdk::update_store(ctx, SLUG, move |s| {
        s["data"] = data;
        Ok(())
    })
    .await
    .unwrap();
}
fn values(name: &str, count: &str, increment: &str, modulo: &str) -> Value {
    json!({"name":name,"count":count,"increment":increment,"modulo":modulo})
}
async fn create(ctx: &CoreContext, s: &mut Value, v: &Value, name: &str) -> Value {
    let f = invoke(ctx, s, v, "Create Counter", Value::Null)
        .await
        .unwrap();
    invoke(ctx, s, &f, "Create", values(name, "0", "1", "1"))
        .await
        .unwrap()
}
fn same_data(actual: &Value, expected: &Value) {
    assert_eq!(actual["currentCounter"], expected["currentCounter"]);
    let a = actual["counters"].as_array().unwrap();
    let b = expected["counters"].as_array().unwrap();
    assert_eq!(a.len(), b.len());
    for (a, b) in a.iter().zip(b) {
        for key in ["id", "name"] {
            assert_eq!(a[key], b[key]);
        }
        for key in ["count", "increment", "modulo"] {
            assert_eq!(a[key].as_f64(), b[key].as_f64(), "{key}");
        }
    }
}

#[test]
fn schema_original_icon_and_pinned_source_integrity() {
    let manifest: Value = serde_json::from_str(adapter::MANIFEST).unwrap();
    let schema: Value =
        serde_json::from_str(include_str!("fixtures/extension-v2.schema.json")).unwrap();
    jsonschema::validator_for(&schema)
        .unwrap()
        .validate(&manifest)
        .unwrap();
    assert_eq!(manifest["id"], "com.tezbar.native.count-numbers");
    assert_eq!(manifest["replaces"], json!(["raycast.count-numbers"]));
    assert_eq!(manifest["icon"], "/extensions/icons/count-numbers.png");
    assert_eq!(adapter::commands(), ["count-numbers"]);
    assert_eq!(adapter::ICON.len(), 17194);
    assert_eq!(
        format!("{:x}", Sha256::digest(adapter::ICON)),
        "50e6a26f8f9a2ae698163d7e11aa31883ae895a08b952403584990a49a7b01ee"
    );
    let original: Value = serde_json::from_str(include_str!("fixtures/v1-package.json")).unwrap();
    assert_eq!(manifest["platforms"], original["platforms"]);
    assert_eq!(manifest["preferences"], original["preferences"]);
    for (a, b) in manifest["commands"]
        .as_array()
        .unwrap()
        .iter()
        .zip(original["commands"].as_array().unwrap())
    {
        for key in [
            "name",
            "title",
            "description",
            "mode",
            "platforms",
            "preferences",
        ] {
            assert_eq!(a[key], b[key]);
        }
        assert_eq!(a["runtime"], "rust");
    }
    assert!(manifest["verification"]
        .as_array()
        .unwrap()
        .iter()
        .all(|v| v["state"] == "not-run"));
    let hashes: Value = serde_json::from_str(include_str!("fixtures/hashes.json")).unwrap();
    for (name, bytes) in [
        (
            "beta_common.rs",
            include_bytes!("fixtures/beta_common.rs").as_slice(),
        ),
        (
            "beta_v2_sdk.rs",
            include_bytes!("fixtures/beta_v2_sdk.rs").as_slice(),
        ),
        (
            "extension-v2.schema.json",
            include_bytes!("fixtures/extension-v2.schema.json").as_slice(),
        ),
        (
            "TEZBAR-LICENSE.txt",
            include_bytes!("fixtures/TEZBAR-LICENSE.txt").as_slice(),
        ),
    ] {
        assert_eq!(
            format!("{:x}", Sha256::digest(bytes)),
            hashes[name].as_str().unwrap()
        );
    }
    // Optional surrounding source check; the published Cargo package is self-contained.
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/count-numbers.jsx");
    if source.is_file() {
        let vectors: Value =
            serde_json::from_str(include_str!("fixtures/v1-vectors.json")).unwrap();
        assert_eq!(
            format!("{:x}", Sha256::digest(std::fs::read(source).unwrap())),
            vectors["sourceSha256"].as_str().unwrap()
        );
    }
}

#[tokio::test]
async fn seven_actions_settings_cancel_create_switch_reset_and_restart() {
    let dir = tempfile::tempdir().unwrap();
    let (ctx, calls) = context_recording(dir.path());
    let (mut s, v) = run(&ctx).await;
    assert_eq!(v["root"]["type"], "Detail");
    assert!(v["root"]["props"]["markdown"]
        .as_str()
        .unwrap()
        .contains("data:image/svg+xml,"));
    assert_eq!(
        v["actions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["title"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "Count",
            "Settings",
            "Decrement Count",
            "Reset Counter",
            "Switch Counter",
            "Create Counter",
            "Delete Counter"
        ]
    );
    let f = invoke(&ctx, &mut s, &v, "Settings", Value::Null)
        .await
        .unwrap();
    let fields = f["root"]["children"].as_array().unwrap();
    assert_eq!(
        fields
            .iter()
            .map(|f| f["props"]["value"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["Counter", "0", "1", "1"]
    );
    assert_eq!(f["root"]["props"]["backActionId"], action(&f, "Cancel"));
    assert_eq!(f["actions"][0]["kind"], "submit-form");
    let v = invoke(
        &ctx,
        &mut s,
        &f,
        "Save",
        values("Renamed <>&", "8tail", "2", "5"),
    )
    .await
    .unwrap();
    assert_eq!(v["title"], "Renamed <>&");
    assert!(!v["root"]["props"]["markdown"]
        .as_str()
        .unwrap()
        .contains("Renamed"));
    let v = invoke(&ctx, &mut s, &v, "Count", Value::Null)
        .await
        .unwrap();
    assert_eq!(
        stored(&ctx).await["data"]["counters"][0]["count"].as_f64(),
        Some(0.0)
    );
    let v = invoke(&ctx, &mut s, &v, "Decrement Count", Value::Null)
        .await
        .unwrap();
    assert_eq!(
        stored(&ctx).await["data"]["counters"][0]["count"].as_f64(),
        Some(-2.0)
    );
    let v = invoke(&ctx, &mut s, &v, "Reset Counter", Value::Null)
        .await
        .unwrap();
    assert!(invoke(&ctx, &mut s, &v, "Delete Counter", Value::Null)
        .await
        .unwrap_err()
        .contains("only counter"));
    let f = invoke(&ctx, &mut s, &v, "Create Counter", Value::Null)
        .await
        .unwrap();
    assert_eq!(f["root"]["children"][0]["props"]["value"], "Counter 2");
    let v = invoke(&ctx, &mut s, &f, "Cancel", values("Discard", "9", "9", "9"))
        .await
        .unwrap();
    assert_eq!(
        stored(&ctx).await["data"]["counters"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let v = create(&ctx, &mut s, &v, "Second").await;
    let f = invoke(&ctx, &mut s, &v, "Switch Counter", Value::Null)
        .await
        .unwrap();
    assert_eq!(f["root"]["children"][0]["type"], "Form.Dropdown");
    assert_eq!(f["root"]["children"][0]["props"]["value"], "1");
    assert_eq!(
        f["root"]["children"][0]["children"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let v = invoke(&ctx, &mut s, &f, "Switch", json!({"counter":"0"}))
        .await
        .unwrap();
    assert_eq!(v["title"], "Renamed <>&");
    let (mut restarted, new_view) = run(&context(dir.path())).await;
    assert_eq!(new_view["title"], "Renamed <>&");
    let f = invoke(&ctx, &mut restarted, &new_view, "Settings", Value::Null)
        .await
        .unwrap();
    assert_eq!(f["root"]["children"][2]["props"]["value"], "2");
    assert_eq!(s["futureSessionKey"], json!({"keep":true}));
    adapter::handle(
        &ctx,
        "count-numbers",
        "extension:dispose-session",
        &json!({}),
        &mut s,
    )
    .await
    .unwrap();
    assert!(s["countNumbers"].get("screen").is_none());
    assert!(s["countNumbers"].get("counterId").is_none());
    assert_eq!(s["futureSessionKey"], json!({"keep":true}));
    assert!(calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn original_javascript_oracle_matches_numeric_settings_arithmetic_create_and_delete() {
    let cases: Value = serde_json::from_str(include_str!("fixtures/v1-vectors.json")).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let ctx = context(dir.path());
    for case in cases["parses"].as_array().unwrap() {
        let (mut s, v) = run(&ctx).await;
        let f = invoke(&ctx, &mut s, &v, "Settings", Value::Null)
            .await
            .unwrap();
        invoke(&ctx, &mut s, &f, "Save", case["input"].clone())
            .await
            .unwrap();
        same_data(&stored(&ctx).await["data"], &case["expected"]);
    }
    for case in cases["arithmetic"].as_array().unwrap() {
        seed(&ctx, case["data"].clone()).await;
        let (mut s, v) = run(&ctx).await;
        invoke(
            &ctx,
            &mut s,
            &v,
            if case["reverse"] == true {
                "Decrement Count"
            } else {
                "Count"
            },
            Value::Null,
        )
        .await
        .unwrap();
        same_data(&stored(&ctx).await["data"], &case["expected"]);
    }
    seed(&ctx,json!({"counters":[{"id":0,"name":"Counter","count":0,"increment":1,"modulo":1}],"currentCounter":0})).await;
    let (mut s, v) = run(&ctx).await;
    let f = invoke(&ctx, &mut s, &v, "Create Counter", Value::Null)
        .await
        .unwrap();
    let v = invoke(
        &ctx,
        &mut s,
        &f,
        "Create",
        values("Created", "-3.9", "0", "0x10"),
    )
    .await
    .unwrap();
    same_data(&stored(&ctx).await["data"], &cases["create"]);
    invoke(&ctx, &mut s, &v, "Delete Counter", Value::Null)
        .await
        .unwrap();
    same_data(&stored(&ctx).await["data"], &cases["deleted"]);
}

#[tokio::test]
async fn drafts_refresh_errors_and_stale_or_forged_actions() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = context(dir.path());
    let (mut s, v) = run(&ctx).await;
    let f = invoke(&ctx, &mut s, &v, "Settings", Value::Null)
        .await
        .unwrap();
    let before = s.clone();
    let bytes = std::fs::read(
        dir.path()
            .join("data/native-extensions-v2/count-numbers/storage.json"),
    )
    .unwrap();
    for _ in 0..3 {
        assert_eq!(
            adapter::handle(
                &ctx,
                "count-numbers",
                "extension:refresh-session",
                &json!({"formValues":values("Typed draft","123","2","3")}),
                &mut s
            )
            .await
            .unwrap()["mode"],
            "unchanged"
        );
    }
    assert_eq!(s, before);
    assert_eq!(
        std::fs::read(
            dir.path()
                .join("data/native-extensions-v2/count-numbers/storage.json")
        )
        .unwrap(),
        bytes
    );
    for bad in [
        json!({}),
        json!({"name":"x","count":1,"increment":"1","modulo":"1"}),
        values("x", &"9".repeat(400), "1", "1"),
    ] {
        assert!(invoke(&ctx, &mut s, &f, "Save", bad).await.is_err());
        assert_eq!(s, before);
    }
    for a in [
        action(&v, "Count"),
        "unknown".into(),
        "count-numbers:1:submit:999".into(),
    ] {
        assert!(adapter::handle(
            &ctx,
            "count-numbers",
            "extension:invoke-action",
            &json!({"actionId":a,"counterId":999}),
            &mut s
        )
        .await
        .is_err());
        assert_eq!(s, before);
    }
    let v = invoke(&ctx, &mut s, &f, "Cancel", Value::Null)
        .await
        .unwrap();
    assert!(
        invoke(&ctx, &mut s, &f, "Save", values("Stale", "7", "1", "1"))
            .await
            .is_err()
    );
    let f = invoke(&ctx, &mut s, &v, "Switch Counter", Value::Null)
        .await
        .unwrap();
    for target in ["999", "00", "0x0", "-0"] {
        assert!(
            invoke(&ctx, &mut s, &f, "Switch", json!({"counter":target}))
                .await
                .is_err()
        );
    }
    for (command, operation) in [
        ("wrong", "extension:run-command"),
        ("count-numbers", "extension:search-text-changed"),
        ("count-numbers", "run-command"),
    ] {
        assert!(
            adapter::handle(&ctx, command, operation, &json!({}), &mut s)
                .await
                .is_err()
        );
    }
    let v = invoke(&ctx, &mut s, &f, "Cancel", Value::Null)
        .await
        .unwrap();
    adapter::handle(
        &ctx,
        "count-numbers",
        "extension:dispose-session",
        &json!({}),
        &mut s,
    )
    .await
    .unwrap();
    assert!(invoke(&ctx, &mut s, &v, "Count", Value::Null)
        .await
        .is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn parallel_fresh_contexts_never_lose_updates_or_follow_another_sessions_selection() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = context(dir.path());
    let (s0, v0) = run(&ctx).await;
    let (mut s1, v1) = run(&ctx).await;
    let v1 = create(&ctx, &mut s1, &v1, "Other").await;
    let mut jobs = Vec::new();
    for _ in 0..40 {
        let ctx = context(dir.path());
        let mut s = s0.clone();
        let v = v0.clone();
        jobs.push(tokio::spawn(async move {
            invoke(&ctx, &mut s, &v, "Count", json!({"counterId":1}))
                .await
                .unwrap();
        }));
    }
    for job in jobs {
        job.await.unwrap();
    }
    let data = stored(&ctx).await;
    assert_eq!(data["data"]["currentCounter"], 1);
    assert_eq!(data["data"]["counters"][0]["count"].as_f64(), Some(40.0));
    assert_eq!(data["data"]["counters"][1]["count"].as_f64(), Some(0.0));
    // Simultaneous creates also allocate IDs inside the same atomic lane.
    let mut jobs = Vec::new();
    for n in 0..8 {
        let ctx = context(dir.path());
        let mut s = s0.clone();
        let v = v0.clone();
        jobs.push(tokio::spawn(async move {
            create(&ctx, &mut s, &v, &format!("Parallel {n}")).await
        }));
    }
    for job in jobs {
        job.await.unwrap();
    }
    let model = stored(&ctx).await;
    let ids: std::collections::HashSet<_> = model["data"]["counters"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_u64().unwrap())
        .collect();
    assert_eq!(ids.len(), 10);
    assert!(ids.contains(&9));
    // Settings stays bound to its own ID, too, despite another session selecting ID 1.
    let mut s0 = s0;
    let f = invoke(&ctx, &mut s0, &v0, "Settings", Value::Null)
        .await
        .unwrap();
    invoke(&ctx, &mut s0, &f, "Save", values("Original", "8", "3", "1"))
        .await
        .unwrap();
    assert_eq!(stored(&ctx).await["data"]["counters"][1]["name"], "Other");
    invoke(&ctx, &mut s1, &v1, "Count", Value::Null)
        .await
        .unwrap();
    assert_eq!(
        stored(&ctx).await["data"]["counters"][1]["count"].as_f64(),
        Some(1.0)
    );
}

#[tokio::test]
async fn deleted_intended_counter_fails_then_back_or_refresh_recovers_and_delete_selects_min_index()
{
    let dir = tempfile::tempdir().unwrap();
    let ctx = context(dir.path());
    let (mut s, v) = run(&ctx).await;
    let v = create(&ctx, &mut s, &v, "B").await;
    let v = create(&ctx, &mut s, &v, "C").await;
    let (mut waiting, shown) = run(&ctx).await;
    let mut detail_waiting = waiting.clone();
    let form = invoke(&ctx, &mut waiting, &shown, "Settings", Value::Null)
        .await
        .unwrap();
    let draft = waiting.clone();
    // Another form offers C as a target while remaining bound to surviving B.
    let (mut picker, pv) = run(&ctx).await;
    let pf = invoke(&ctx, &mut picker, &pv, "Switch Counter", Value::Null)
        .await
        .unwrap();
    let pv = invoke(&ctx, &mut picker, &pf, "Switch", json!({"counter":"1"}))
        .await
        .unwrap();
    let offered = invoke(&ctx, &mut picker, &pv, "Switch Counter", Value::Null)
        .await
        .unwrap();
    let v = invoke(&ctx, &mut s, &v, "Delete Counter", Value::Null)
        .await
        .unwrap();
    assert_eq!(stored(&ctx).await["data"]["currentCounter"], 1);
    assert!(invoke(
        &ctx,
        &mut waiting,
        &form,
        "Save",
        values("Lost draft", "4", "1", "1")
    )
    .await
    .unwrap_err()
    .contains("no longer exists"));
    assert_eq!(waiting, draft);
    assert!(
        invoke(&ctx, &mut detail_waiting, &shown, "Count", Value::Null)
            .await
            .is_err()
    );
    let back = invoke(&ctx, &mut waiting, &form, "Cancel", Value::Null)
        .await
        .unwrap();
    assert_eq!(back["title"], "B");
    // The stale detail session can refresh after its target disappears.
    let mut stale = detail_waiting.clone();
    assert_eq!(
        adapter::handle(
            &ctx,
            "count-numbers",
            "extension:refresh-session",
            &json!({}),
            &mut stale
        )
        .await
        .unwrap()["title"],
        "B"
    );
    let f = invoke(&ctx, &mut s, &v, "Switch Counter", Value::Null)
        .await
        .unwrap();
    let v = invoke(&ctx, &mut s, &f, "Switch", json!({"counter":"0"}))
        .await
        .unwrap();
    let v = invoke(&ctx, &mut s, &v, "Delete Counter", Value::Null)
        .await
        .unwrap();
    assert_eq!(v["title"], "B");
    assert_eq!(stored(&ctx).await["data"]["currentCounter"], 1);
    assert!(invoke(&ctx, &mut s, &v, "Delete Counter", Value::Null)
        .await
        .is_err());
    let v = create(&ctx, &mut s, &v, "Next").await;
    // ID 2 was deleted and reused by max + 1. It is still a DIFFERENT counter.
    assert!(
        invoke(&ctx, &mut detail_waiting, &shown, "Count", Value::Null)
            .await
            .unwrap_err()
            .contains("ID was reused")
    );
    assert_eq!(stored(&ctx).await["data"]["currentCounter"], 2);
    assert_eq!(v["title"], "Next");
    assert!(invoke(
        &ctx,
        &mut picker,
        &offered,
        "Switch",
        json!({"counter":"2"})
    )
    .await
    .unwrap_err()
    .contains("ID was reused"));
}

#[tokio::test]
async fn sdk_namespace_unknown_keys_and_corruption_are_preserved_across_fresh_contexts() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = context(dir.path());
    v2_sdk::update_store(&ctx, SLUG, |s| {
        s["futureRoot"] = json!({"keep":true});
        Ok(())
    })
    .await
    .unwrap();
    let (mut s, v) = run(&ctx).await;
    v2_sdk::update_store(&ctx, SLUG, |s| {
        s["data"]["futureModel"] = json!(7);
        s["data"]["counters"][0]["futureCounter"] = json!(8);
        Ok(())
    })
    .await
    .unwrap();
    invoke(&ctx, &mut s, &v, "Count", Value::Null)
        .await
        .unwrap();
    let data = stored(&context(dir.path())).await;
    assert_eq!(data["futureRoot"], json!({"keep":true}));
    assert_eq!(data["data"]["futureModel"], 7);
    assert_eq!(data["data"]["counters"][0]["futureCounter"], 8);
    assert_eq!(v2_sdk::read_store(&ctx, "other").await.unwrap(), json!({}));
    v2_sdk::update_store(&ctx, "other", |s| {
        *s = json!({"data":"other namespace"});
        Ok(())
    })
    .await
    .unwrap();
    assert_eq!(stored(&ctx).await, data);
    let path = dir
        .path()
        .join("data/native-extensions-v2/count-numbers/storage.json");
    for bad in [
        b"{broken".to_vec(),
        b"null".to_vec(),
        b"[]".to_vec(),
        br#"{"data":null,"unknown":1}"#.to_vec(),
        br#"{"data":{}}"#.to_vec(),
        br#"{"data":{"counters":[],"currentCounter":0}}"#.to_vec(),
    ] {
        std::fs::write(&path, &bad).unwrap();
        let mut s = json!({});
        assert!(adapter::handle(
            &ctx,
            "count-numbers",
            "extension:run-command",
            &json!({}),
            &mut s
        )
        .await
        .is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bad);
        assert_eq!(s, json!({}));
    }
    for mutate in [
        "duplicate",
        "current",
        "name",
        "count",
        "id",
        "generation",
        "instance",
    ] {
        let mut model = data.clone();
        match mutate {
            "duplicate" => {
                let c = model["data"]["counters"][0].clone();
                model["data"]["counters"].as_array_mut().unwrap().push(c);
            }
            "current" => model["data"]["currentCounter"] = json!(999),
            "name" => model["data"]["counters"][0]["name"] = json!(null),
            "count" => model["data"]["counters"][0]["count"] = json!("NaN"),
            "generation" => model["counterGeneration"] = json!("broken"),
            "instance" => model["data"]["counters"][0]["nativeInstance"] = json!(1),
            _ => model["data"]["counters"][0]["id"] = json!(-1),
        };
        let bytes = serde_json::to_vec(&model).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        assert!(adapter::handle(
            &ctx,
            "count-numbers",
            "extension:run-command",
            &json!({}),
            &mut json!({})
        )
        .await
        .is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
}

struct FailingHost {
    data: Mutex<Value>,
    failure: Mutex<Option<std::result::Result<Value, String>>>,
    writes: Mutex<usize>,
}
impl Host for FailingHost {
    fn read_store(&self) -> HostFuture<'_> {
        let result = self
            .failure
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(|| Ok(self.data.lock().unwrap().clone()));
        Box::pin(async move { result })
    }
    fn update_store(&self, change: StoreChange) -> HostFuture<'_> {
        *self.writes.lock().unwrap() += 1;
        let result = if let Some(fail) = self.failure.lock().unwrap().clone() {
            fail
        } else {
            let mut data = self.data.lock().unwrap();
            let mut staged = data.clone();
            match change(&mut staged) {
                Ok(()) => {
                    *data = staged.clone();
                    Ok(staged)
                }
                Err(e) => Err(e),
            }
        };
        Box::pin(async move { result })
    }
}
#[tokio::test]
async fn host_async_failures_leave_drafts_and_store_unchanged_no_heartbeat_writes() {
    let host = FailingHost {
        data: Mutex::new(json!({})),
        failure: Mutex::new(None),
        writes: Mutex::new(0),
    };
    let mut s = json!({});
    let v = handle(
        &host,
        "count-numbers",
        "extension:run-command",
        &json!({}),
        &mut s,
    )
    .await
    .unwrap();
    let f = handle(
        &host,
        "count-numbers",
        "extension:invoke-action",
        &json!({"actionId":action(&v,"Settings")}),
        &mut s,
    )
    .await
    .unwrap();
    let before = s.clone();
    let data = host.data.lock().unwrap().clone();
    for fail in [
        Err("disk denied".into()),
        Ok(json!({"ok":false,"message":"save denied"})),
        Ok(json!({"ok":false,"error":"offline"})),
    ] {
        *host.failure.lock().unwrap() = Some(fail);
        let e = handle(
            &host,
            "count-numbers",
            "extension:invoke-action",
            &json!({"actionId":action(&f,"Save"),"formValues":values("Draft","12","1","1")}),
            &mut s,
        )
        .await
        .unwrap_err();
        assert!(matches!(
            e.as_str(),
            "disk denied" | "save denied" | "offline"
        ));
        assert_eq!(s, before);
        assert_eq!(*host.data.lock().unwrap(), data);
    }
    *host.failure.lock().unwrap() = None;
    let writes = *host.writes.lock().unwrap();
    for _ in 0..3 {
        handle(
            &host,
            "count-numbers",
            "extension:refresh-session",
            &json!({}),
            &mut s,
        )
        .await
        .unwrap();
    }
    assert_eq!(*host.writes.lock().unwrap(), writes);
    let v = handle(
        &host,
        "count-numbers",
        "extension:invoke-action",
        &json!({"actionId":action(&f,"Save"),"formValues":values("Draft","12","1","1")}),
        &mut s,
    )
    .await
    .unwrap();
    assert_eq!(v["title"], "Draft");
    let writes = *host.writes.lock().unwrap();
    for _ in 0..3 {
        handle(
            &host,
            "count-numbers",
            "extension:refresh-session",
            &json!({}),
            &mut s,
        )
        .await
        .unwrap();
    }
    assert_eq!(*host.writes.lock().unwrap(), writes);
    // A finite store may still overflow during increment; no null or success is saved.
    host.data.lock().unwrap()["data"]["counters"][0]["count"] = json!(f64::MAX);
    host.data.lock().unwrap()["data"]["counters"][0]["increment"] = json!(f64::MAX);
    let before = s.clone();
    assert!(handle(
        &host,
        "count-numbers",
        "extension:invoke-action",
        &json!({"actionId":action(&v,"Count")}),
        &mut s
    )
    .await
    .is_err());
    assert_eq!(s, before);
    assert_eq!(
        host.data.lock().unwrap()["data"]["counters"][0]["count"].as_f64(),
        Some(f64::MAX)
    );
}
