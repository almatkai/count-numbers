//! Count Numbers, translated from the MIT Raycast source; see LICENSE.
//! In-process library: storage and its atomic transaction lane belong to the host.
use serde_json::{json, Value};
use std::{collections::HashSet, future::Future, pin::Pin};

pub const ICON: &[u8] = include_bytes!("../assets/extension_icon.png");
pub const MANIFEST: &str = include_str!("../manifest.json");
pub type Result = std::result::Result<Value, String>;
pub type HostFuture<'a> = Pin<Box<dyn Future<Output = Result> + Send + 'a>>;
pub type StoreChange =
    Box<dyn FnOnce(&mut Value) -> std::result::Result<(), String> + Send + 'static>;

/// Return the complete namespace object. Updates must read the latest object,
/// execute the callback and persist atomically across ALL sessions/contexts.
/// A callback error must leave storage untouched. Never use a per-session cache.
pub trait Host: Sync {
    fn read_store(&self) -> HostFuture<'_>;
    fn update_store(&self, change: StoreChange) -> HostFuture<'_>;
}
pub fn commands() -> &'static [&'static str] {
    &["count-numbers"]
}
const SESSION: &str = "countNumbers";
const MAX_ID: u64 = 9_007_199_254_740_991;
const MAIN: &[(&str, &str)] = &[
    ("count", "Count"),
    ("settings", "Settings"),
    ("decrement", "Decrement Count"),
    ("reset", "Reset Counter"),
    ("switch", "Switch Counter"),
    ("create", "Create Counter"),
    ("delete", "Delete Counter"),
];
fn failure_checked(result: Result) -> Result {
    let value = result?;
    if value["ok"] == false {
        return Err(value["message"]
            .as_str()
            .or_else(|| value["error"].as_str())
            .unwrap_or("Host storage request failed")
            .to_owned());
    }
    Ok(value)
}
fn default_data() -> Value {
    json!({"counters":[{"id":0,"name":"Counter","count":0,"increment":1,"modulo":1}],"currentCounter":0})
}
fn validate(store: &Value) -> std::result::Result<&Value, String> {
    let data = store
        .as_object()
        .and_then(|s| s.get("data"))
        .ok_or("Counter model is missing; reopen the command")?;
    let counters = data["counters"]
        .as_array()
        .filter(|c| !c.is_empty())
        .ok_or("Corrupt counter storage: counters must be a nonempty array")?;
    let generation = store
        .get("counterGeneration")
        .map(|v| {
            v.as_u64()
                .filter(|n| *n <= MAX_ID)
                .ok_or("Corrupt counter storage: invalid generation")
        })
        .transpose()?
        .unwrap_or(0);
    let mut ids = HashSet::new();
    for counter in counters {
        let id = counter["id"]
            .as_u64()
            .filter(|i| *i <= MAX_ID)
            .ok_or("Corrupt counter storage: invalid ID")?;
        if !ids.insert(id) || !counter["name"].is_string() {
            return Err("Corrupt counter storage: duplicate ID or invalid name".into());
        }
        if let Some(instance) = counter.get("nativeInstance") {
            if !instance.as_u64().is_some_and(|n| n > 0 && n <= generation) {
                return Err("Corrupt counter storage: invalid counter instance".into());
            }
        }
        for key in ["count", "increment", "modulo"] {
            if !counter[key].as_f64().is_some_and(f64::is_finite) {
                return Err(format!(
                    "Corrupt counter storage: {key} must be a finite number"
                ));
            }
        }
    }
    if !data["currentCounter"]
        .as_u64()
        .is_some_and(|id| ids.contains(&id))
    {
        return Err("Corrupt counter storage: current counter does not exist".into());
    }
    Ok(data)
}
fn initialize(store: &mut Value) -> std::result::Result<(), String> {
    let object = store
        .as_object_mut()
        .ok_or("Corrupt counter storage: expected an object")?;
    // Null, partial and malformed existing models are corruption, never defaults.
    object.entry("data").or_insert_with(default_data);
    validate(store)?;
    Ok(())
}
fn counter(data: &Value, id: u64) -> std::result::Result<&Value, String> {
    data["counters"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == id)
        .ok_or_else(|| "Counter no longer exists. Go Back or refresh the counter view.".into())
}
fn instance(counter: &Value) -> u64 {
    counter["nativeInstance"].as_u64().unwrap_or(0)
}
fn intended(data: &Value, id: u64, token: u64) -> std::result::Result<&Value, String> {
    let counter = counter(data, id)?;
    if instance(counter) != token {
        return Err(
            "Counter no longer exists (its ID was reused). Go Back or refresh the counter view."
                .into(),
        );
    }
    Ok(counter)
}
fn js_whitespace(c: char) -> bool {
    matches!(c, '\u{0009}'..='\u{000d}' | '\u{0020}' | '\u{00a0}' | '\u{1680}' |
        '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' |
        '\u{205f}' | '\u{3000}' | '\u{feff}')
}
fn number_text(n: f64) -> String {
    ryu_js::Buffer::new().format(n).to_owned()
}
/// ECMAScript parseInt(text) || fallback: signs, hexadecimal prefix, decimal
/// prefix, truncation and zero/NaN fallback. Reject infinity instead of saving null.
fn parse_setting(value: &Value, fallback: f64) -> std::result::Result<f64, String> {
    let input = value
        .as_str()
        .ok_or("Counter fields must be text")?
        .trim_start_matches(js_whitespace);
    let (sign, input) = match input.as_bytes().first() {
        Some(b'-') => (-1.0, &input[1..]),
        Some(b'+') => (1.0, &input[1..]),
        _ => (1.0, input),
    };
    let hex = input.starts_with("0x") || input.starts_with("0X");
    let input = if hex { &input[2..] } else { input };
    let digits = input
        .bytes()
        .take_while(|b| {
            if hex {
                b.is_ascii_hexdigit()
            } else {
                b.is_ascii_digit()
            }
        })
        .count();
    if digits == 0 {
        return Ok(fallback);
    }
    let number = if hex {
        use num_traits::ToPrimitive;
        num_bigint::BigUint::parse_bytes(&input.as_bytes()[..digits], 16)
            .unwrap()
            .to_f64()
            .unwrap_or(f64::INFINITY)
    } else {
        input[..digits]
            .parse::<f64>()
            .map_err(|_| "Count is too large")?
    } * sign;
    if !number.is_finite() {
        return Err("Counter values must be finite numbers".into());
    }
    Ok(if number == 0.0 { fallback } else { number })
}
fn settings(values: &Value) -> Result {
    let name = values["name"].as_str().ok_or("Name must be text")?;
    Ok(
        json!({"name":name,"count":parse_setting(&values["count"],0.0)?,
        "increment":parse_setting(&values["increment"],1.0)?,"modulo":parse_setting(&values["modulo"],1.0)?}),
    )
}
fn action_id(session: &Value, verb: &str) -> String {
    format!(
        "count-numbers:{}:{verb}",
        session["revision"].as_u64().unwrap()
    )
}
fn next_revision(session: &Value) -> std::result::Result<u64, String> {
    session["revision"]
        .as_u64()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| "Counter session revision exhausted; reopen command".into())
}
fn session(state: &Value) -> std::result::Result<Value, String> {
    let s = &state[SESSION];
    if !matches!(
        s["screen"].as_str(),
        Some("detail" | "settings" | "switch" | "create")
    ) || s["revision"].as_u64().is_none()
        || s["counterId"].as_u64().is_none()
        || s["instance"].as_u64().is_none()
    {
        return Err("Counter session is missing or expired; reopen command".into());
    }
    Ok(s.clone())
}
fn store_session(state: &mut Value, session: Value) {
    state[SESSION] = session;
}
fn action(s: &Value, verb: &str, title: &str, kind: &str) -> Value {
    json!({"id":action_id(s,verb),"title":title,"kind":kind})
}
fn view(title: &str, root: Value, actions: Vec<Value>) -> Value {
    json!({"ok":true,"mode":"view","title":title,"root":root,"actions":actions})
}
fn unchanged() -> Value {
    json!({"ok":true,"mode":"unchanged"})
}
fn encode(text: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            use std::fmt::Write;
            write!(out, "%{b:02X}").unwrap();
        }
    }
    out
}
fn detail(data: &Value, s: &Value) -> Value {
    let c = counter(data, s["counterId"].as_u64().unwrap()).unwrap();
    let text = number_text(c["count"].as_f64().unwrap());
    // Only finite, formatted numeric text enters the image; names stay in JSON titles.
    // Explicit SVG colors adapt inside the self-contained image, including light mode.
    let size = (600.0 / text.len().max(1) as f64).min(200.0);
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 700 300"><style>text{{fill:#111}}@media(prefers-color-scheme:dark){{text{{fill:#fff}}}}</style><text x="350" y="150" text-anchor="middle" dominant-baseline="central" font-family="-apple-system,Segoe UI,sans-serif" font-size="{size}">{text}</text></svg>"##
    );
    let markdown = format!(
        r#"<img width="700" height="300" alt="{text}" src="data:image/svg+xml,{}" />"#,
        encode(&svg)
    );
    let actions = MAIN
        .iter()
        .map(|(verb, title)| {
            let mut a = action(
                s,
                verb,
                title,
                if ["settings", "switch", "create"].contains(verb) {
                    "push"
                } else {
                    "action"
                },
            );
            match *verb {
                "reset" | "delete" => a["style"] = json!("destructive"),
                _ => {}
            }
            match *verb {
                "decrement" => {
                    a["shortcut"] = json!({"modifiers":["cmd","shift"],"key":"arrowDown"})
                }
                "switch" => a["shortcut"] = json!({"modifiers":["cmd"],"key":"s"}),
                "create" => a["shortcut"] = json!({"modifiers":["cmd"],"key":"n"}),
                "delete" => a["shortcut"] = json!({"modifiers":["cmd","shift"],"key":"delete"}),
                _ => {}
            }
            a
        })
        .collect();
    view(
        c["name"].as_str().unwrap(),
        json!({"type":"Detail","props":{"markdown":markdown}}),
        actions,
    )
}
fn form(data: &Value, s: &mut Value, screen: &str) -> Value {
    s["screen"] = json!(screen);
    let (title, submit, children) = if screen == "switch" {
        // Session-owned allowlist prevents a forged dropdown submission, and a
        // transaction rechecks existence if an option was deleted in another session.
        s["switchOptions"] = Value::Object(
            data["counters"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| (c["id"].as_u64().unwrap().to_string(), json!(instance(c))))
                .collect(),
        );
        (
            "Switch Counter",
            "Switch",
            vec![
                json!({"type":"Form.Dropdown","props":{"id":"counter","title":"Counter","value":s["counterId"].as_u64().unwrap().to_string()},"children":data["counters"].as_array().unwrap().iter().map(|c| json!({"type":"Form.Dropdown.Item","props":{"title":c["name"],"value":c["id"].as_u64().unwrap().to_string()}})).collect::<Vec<_>>()}),
            ],
        )
    } else {
        let initial = if screen == "settings" {
            counter(data, s["counterId"].as_u64().unwrap())
                .unwrap()
                .clone()
        } else {
            json!({"name":format!("Counter {}",data["counters"].as_array().unwrap().len()+1),"count":0,"increment":1,"modulo":1})
        };
        let children = [
            ("name", "Name"),
            ("count", "Count"),
            ("increment", "Increment"),
            ("modulo", "Modulo"),
        ]
        .iter()
        .map(|(id, label)| {
            let value = if *id == "name" {
                initial[*id].as_str().unwrap().to_owned()
            } else {
                number_text(initial[*id].as_f64().unwrap())
            };
            json!({"type":"Form.TextField","props":{"id":id,"title":label,"value":value}})
        })
        .collect();
        if screen == "settings" {
            ("Settings", "Save", children)
        } else {
            ("Create Counter", "Create", children)
        }
    };
    view(
        title,
        json!({"type":"Form","props":{"backActionId":action_id(s,"back")},"children":children}),
        vec![
            action(s, "submit", submit, "submit-form"),
            action(s, "back", "Cancel", "pop"),
        ],
    )
}
fn detail_session(data: &Value, s: &mut Value, preferred: u64) -> Value {
    let id = if counter(data, preferred).is_ok() {
        preferred
    } else {
        data["currentCounter"].as_u64().unwrap()
    };
    s["screen"] = json!("detail");
    s["counterId"] = json!(id);
    s["instance"] = json!(instance(counter(data, id).unwrap()));
    s.as_object_mut().unwrap().remove("switchOptions");
    detail(data, s)
}

pub async fn handle<H: Host + ?Sized>(
    host: &H,
    command: &str,
    operation: &str,
    payload: &Value,
    state: &mut Value,
) -> Result {
    if !commands().contains(&command) {
        return Err(format!("Unsupported counter command: {command}"));
    }
    if !state.is_object() {
        return Err("Counter session state must be an object".into());
    }
    match operation {
        "extension:run-command" => {
            let revision = next_revision(&state[SESSION])?;
            let mut store = failure_checked(host.read_store().await)?;
            if store.as_object().is_some_and(|v| !v.contains_key("data")) {
                store = failure_checked(host.update_store(Box::new(initialize)).await)?;
            }
            let data = validate(&store)?;
            let s = json!({"screen":"detail","counterId":data["currentCounter"],"instance":instance(counter(data,data["currentCounter"].as_u64().unwrap())?),"revision":revision});
            let result = detail(data, &s);
            store_session(state, s);
            Ok(result)
        }
        "extension:refresh-session" => {
            let mut s = session(state)?;
            if s["screen"] != "detail" {
                return Ok(unchanged());
            }
            let store = failure_checked(host.read_store().await)?;
            let data = validate(&store)?;
            let id = s["counterId"].as_u64().unwrap();
            if intended(data, id, s["instance"].as_u64().unwrap()).is_err() {
                s["revision"] = json!(next_revision(&s)?);
            }
            let result = detail_session(data, &mut s, id);
            store_session(state, s);
            Ok(result)
        }
        "extension:dispose-session" => {
            // Retain only the revision so a reused state cannot revive an old
            // action ID after dispose/relaunch. No counter or form remains active.
            state[SESSION] = json!({"revision":next_revision(&state[SESSION])?});
            Ok(unchanged())
        }
        "extension:invoke-action" => invoke(host, payload, state).await,
        _ => Err(format!("Unsupported counter operation: {operation}")),
    }
}
async fn invoke<H: Host + ?Sized>(host: &H, payload: &Value, state: &mut Value) -> Result {
    let mut s = session(state)?;
    let action = payload["actionId"]
        .as_str()
        .ok_or("Action ID is required")?;
    let verbs: Vec<&str> = if s["screen"] == "detail" {
        MAIN.iter().map(|(v, _)| *v).collect()
    } else {
        vec!["submit", "back"]
    };
    let verb = verbs
        .into_iter()
        .find(|v| action_id(&s, v) == action)
        .ok_or("Unknown, stale, or wrong-screen counter action; refresh the view")?;
    let revision = next_revision(&s)?;
    let id = s["counterId"].as_u64().unwrap();
    let token = s["instance"].as_u64().unwrap();
    if ["settings", "create", "switch", "back"].contains(&verb) {
        let store = failure_checked(host.read_store().await)?;
        let data = validate(&store)?;
        if verb != "back" {
            intended(data, id, token)?;
        }
        s["revision"] = json!(revision);
        let result = if verb == "back" {
            detail_session(data, &mut s, id)
        } else {
            form(data, &mut s, verb)
        };
        store_session(state, s);
        return Ok(result);
    }
    let screen = s["screen"].as_str().unwrap().to_owned();
    let input = if verb == "submit" && screen != "switch" {
        Some(settings(&payload["formValues"])?)
    } else {
        None
    };
    let target = if verb == "submit" && screen == "switch" {
        let text = payload["formValues"]["counter"]
            .as_str()
            .ok_or("Choose a counter")?;
        let target = text
            .parse::<u64>()
            .map_err(|_| "Invalid counter selection")?;
        let offered = s["switchOptions"].get(text).and_then(Value::as_u64);
        if text != target.to_string() || offered.is_none() {
            return Err("Counter was not offered by this form".into());
        }
        Some((target, offered.unwrap()))
    } else {
        None
    };
    let store = failure_checked(
        host.update_store(Box::new(move |store| {
            let data = validate(store)?;
            // Always resolve the captured session ID against the latest model, never
            // the global currentCounter or an action payload supplied by the renderer.
            intended(data, id, token)?;
            if let Some((target, token)) = target {
                intended(data, target, token)?;
            }
            let new_instance = if verb == "submit" && screen == "create" {
                let next = store["counterGeneration"]
                    .as_u64()
                    .unwrap_or(0)
                    .checked_add(1)
                    .filter(|n| *n <= MAX_ID)
                    .ok_or("Counter instance IDs exhausted")?;
                store["counterGeneration"] = json!(next);
                Some(next)
            } else {
                None
            };
            let data = &mut store["data"];
            let counters = data["counters"].as_array_mut().unwrap();
            let index = counters.iter().position(|c| c["id"] == id).unwrap();
            match verb {
                "count" | "decrement" => {
                    let c = &mut counters[index];
                    let step = c["increment"].as_f64().unwrap()
                        * if verb == "decrement" { -1.0 } else { 1.0 };
                    let mut n = c["count"].as_f64().unwrap() + step;
                    if c["modulo"].as_f64().unwrap() > 1.0 {
                        n %= c["modulo"].as_f64().unwrap();
                    }
                    if !n.is_finite() {
                        return Err("Counter arithmetic must produce a finite number".into());
                    }
                    c["count"] = json!(n);
                }
                "reset" => counters[index]["count"] = json!(0),
                "delete" => {
                    if counters.len() == 1 {
                        return Err("Cannot delete the only counter".into());
                    }
                    counters.remove(index);
                    let next = counters[index.min(counters.len() - 1)]["id"].clone();
                    data["currentCounter"] = next;
                }
                "submit" => match screen.as_str() {
                    "settings" => {
                        for (key, value) in input.as_ref().unwrap().as_object().unwrap() {
                            counters[index][key] = value.clone();
                        }
                    }
                    "create" => {
                        let max = counters
                            .iter()
                            .map(|c| c["id"].as_u64().unwrap())
                            .max()
                            .unwrap();
                        let new = max
                            .checked_add(1)
                            .filter(|n| *n <= MAX_ID)
                            .ok_or("Counter IDs exhausted")?;
                        let mut c = input.unwrap();
                        c["id"] = json!(new);
                        c["nativeInstance"] = json!(new_instance.unwrap());
                        counters.push(c);
                        data["currentCounter"] = json!(new);
                    }
                    "switch" => data["currentCounter"] = json!(target.unwrap().0),
                    _ => return Err("Invalid counter form".into()),
                },
                _ => return Err("Invalid counter mutation".into()),
            }
            Ok(())
        }))
        .await,
    )?;
    let data = validate(&store)?;
    s["revision"] = json!(revision);
    let preferred = if verb == "delete"
        || (verb == "submit" && (s["screen"] == "create" || s["screen"] == "switch"))
    {
        data["currentCounter"].as_u64().unwrap()
    } else {
        id
    };
    let result = detail_session(data, &mut s, preferred);
    store_session(state, s);
    Ok(result)
}
