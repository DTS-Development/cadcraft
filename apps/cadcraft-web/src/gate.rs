//! The PPM build's host rules that need no browser: which parent page is trusted, which drawings
//! and open options are taken, and how the host's policy is read. Kept apart from `ppm_host.rs`
//! so `cargo test` covers them on the desktop.
#![cfg_attr(not(feature = "ppm-host"), allow(dead_code))]

/// The interface language of the PPM build, from the framing page's `?lang=` value: English for
/// `en` (or an `en-…` tag), Korean otherwise, whatever the browser or the saved preferences say.
/// Only these two: the build embeds no font for the other CJK interfaces, and has no Japanese
/// catalog.
pub fn host_language(lang: Option<&str>) -> cadcraft_ui_egui::i18n::Preference {
    use cadcraft_ui_egui::i18n::Preference;
    let primary = lang.and_then(|l| l.trim().split(['-', '_']).next());
    if primary.is_some_and(|l| l.eq_ignore_ascii_case("en")) { Preference::English } else { Preference::Korean }
}

/// The open options of the host's `cadcraft:open` this build understands (sent with
/// `cadcraft:ready`). A host asking for one that is missing (an older build) is turned away
/// instead of getting a drawing it can still edit.
pub const OPEN_OPTIONS: &[&str] = &["readOnly", "chrome", "zoomControls"];

/// How a drawing is shown, from the booleans of the host's open message. One that is not given
/// keeps today's behaviour: editable, with the chrome and without the zoom bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewOptions {
    pub read_only: bool,
    pub chrome: bool,
    pub zoom_controls: bool,
}

impl Default for ViewOptions {
    fn default() -> Self {
        Self { read_only: false, chrome: true, zoom_controls: false }
    }
}

/// The view options of one open message (`None` = not given).
pub fn view_options(read_only: Option<bool>, chrome: Option<bool>, zoom_controls: Option<bool>) -> ViewOptions {
    let plain = ViewOptions::default();
    ViewOptions {
        read_only: read_only.unwrap_or(plain.read_only),
        chrome: chrome.unwrap_or(plain.chrome),
        zoom_controls: zoom_controls.unwrap_or(plain.zoom_controls),
    }
}

/// Is the parent on the page's own origin? `parent` is what the page could read of the parent's
/// origin: `None` when the browser refused (a page from another origin).
pub fn same_origin(parent: Option<&str>, own: &str) -> bool {
    parent == Some(own)
}

/// Whether the page starts CADCraft. The PPM build works only inside a same-origin parent page
/// that talks to it; opened directly it shows a notice and nothing else.
#[derive(Debug, PartialEq, Eq)]
pub enum Start {
    Run,
    OpenFromPpm,
}

/// Shown in place of the app when the PPM build is not framed by the PPM page.
pub const OPEN_FROM_PPM: &str = "<p>PPM에서 열어 주세요</p>";

/// `host_param`: the page's `?host=` value; `has_bridge`: a same-origin parent page was found.
pub fn decide(host_param: Option<&str>, has_bridge: bool) -> Start {
    if host_param == Some("parent") && has_bridge { Start::Run } else { Start::OpenFromPpm }
}

/// The only kind of drawing the browser editor opens. DWG is converted to DXF on the server
/// first, and `.lsp` (which the desktop app would run as a program) is never taken from a host.
pub fn is_drawing(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, ext)| ext.eq_ignore_ascii_case("dxf"))
}

/// Why `name` is refused, or `None` when the editor opens it.
pub fn open_refusal(name: &str) -> Option<&'static str> {
    if is_drawing(name) {
        return None;
    }
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
    Some(match ext.as_deref() {
        Some("dwg") => "unsupported-format: DWG is converted to DXF on the server before it is opened",
        _ => "unsupported-format: only .dxf drawings open",
    })
}

/// The command overrides in the `policy` of a parent's open message, given as the JSON text of
/// `policy`: `{ "overrides": { "<command id>": true | false } }`. Anything else (not an object,
/// no `overrides`, an array, values that are no boolean) is left out, so a malformed policy changes
/// nothing. There is no cap here: the server limits the count.
pub fn policy_overrides(policy_json: &str) -> Vec<(String, bool)> {
    let Ok(serde_json::Value::Object(policy)) = serde_json::from_str::<serde_json::Value>(policy_json) else { return Vec::new() };
    let Some(serde_json::Value::Object(overrides)) = policy.get("overrides") else { return Vec::new() };
    overrides.iter().filter_map(|(id, on)| Some((id.clone(), on.as_bool()?))).collect()
}

/// The longest `id` of a request (a bytes request or an add-fonts message).
pub const REQUEST_ID_MAX: usize = 128;

/// The id of a request when it is usable: not empty and at most [`REQUEST_ID_MAX`] characters.
pub fn request_id(id: &str) -> Option<String> {
    (!id.is_empty() && id.chars().count() <= REQUEST_ID_MAX).then(|| id.to_string())
}

/// Why a `cadcraft:request-bytes` is asked: `save` stores the drawing (it counts as saved),
/// `check` and `preview` only read it. Anything else is no purpose.
pub fn bytes_purpose(purpose: &str) -> Option<&'static str> {
    ["save", "check", "preview"].into_iter().find(|p| *p == purpose)
}

/// `purpose` cut to the longest an answer repeats, so an odd request can't make a big reply.
pub fn echo_purpose(purpose: &str) -> String {
    purpose.chars().take(200).collect()
}

/// The file name a drawing is handed to the host under: the title with a `.dxf` extension
/// (the bytes are DXF whatever the drawing was opened from).
pub fn dxf_name(title: &str) -> String {
    let title = title.trim();
    let stem = match title.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && ext.len() <= 4 && ext.chars().all(|c| c.is_ascii_alphanumeric()) => stem,
        _ => title,
    };
    if stem.is_empty() { "Drawing.dxf".to_string() } else { format!("{stem}.dxf") }
}

/// Does `bytes` start like one TrueType or OpenType font file? (A `ttcf` collection holds several
/// faces and is not taken.)
pub fn is_font_file(bytes: &[u8]) -> bool {
    matches!(bytes.get(..4), Some([0, 1, 0, 0]) | Some(b"OTTO") | Some(b"true"))
}

/// What the host can ask the open drawing to do besides handing out bytes: layer visibility,
/// the layout shown and the zoom. `name` is at most [`NAME_MAX`] characters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    SetLayerVisible { name: String, visible: bool },
    SetLayout { name: String },
    ZoomExtents,
}

/// The longest layer or layout name taken.
pub const NAME_MAX: usize = 255;

/// The action of one `cadcraft:action` message, or why it is refused (`bad-action`).
pub fn parse_action(kind: &str, name: Option<String>, visible: Option<bool>) -> Result<Action, &'static str> {
    let name = name.filter(|n| !n.is_empty() && n.chars().count() <= NAME_MAX);
    match (kind, name, visible) {
        ("set-layer-visible", Some(name), Some(visible)) => Ok(Action::SetLayerVisible { name, visible }),
        ("set-layout", Some(name), _) => Ok(Action::SetLayout { name }),
        ("zoom-extents", _, _) => Ok(Action::ZoomExtents),
        _ => Err("bad-action"),
    }
}

impl Action {
    /// The engine command and its parameters.
    pub fn command(&self) -> (&'static str, serde_json::Value) {
        match self {
            Self::SetLayerVisible { name, visible } => ("layer.set", serde_json::json!({ "name": name, "on": visible })),
            Self::SetLayout { name } => ("layout.set", serde_json::json!({ "name": name })),
            Self::ZoomExtents => ("zoom.extents", serde_json::json!({})),
        }
    }
}

/// The `reason` of a failed action from the engine's error text: a name that isn't there is
/// `not-found`, a read-only refusal `read-only`, anything else the text itself.
pub fn action_reason(error: &str) -> String {
    let lower = error.to_ascii_lowercase();
    if lower.contains("read only") {
        "read-only".to_string()
    } else if lower.contains("no layer") || lower.contains("no layout") || lower.contains("not found") || lower.contains("unknown layout") {
        "not-found".to_string()
    } else {
        error.chars().take(200).collect()
    }
}

/// The most layers, layouts and read notes one `cadcraft:info` carries.
pub const INFO_LAYERS_MAX: usize = 5000;
pub const INFO_LAYOUTS_MAX: usize = 500;
pub const INFO_NOTES_MAX: usize = 50;

/// The drawing summary a host gets, cut from the engine's `drawing.inspect` (no entity list, no
/// selection or undo history) and capped so a huge drawing can't make a huge message.
pub fn drawing_info(inspect: &serde_json::Value) -> serde_json::Value {
    use serde_json::{Value, json};
    let list = |key: &str, max: usize| -> Vec<Value> { inspect.get(key).and_then(Value::as_array).map(|a| a.iter().take(max).cloned().collect()).unwrap_or_default() };
    let get = |key: &str| inspect.get(key).cloned().unwrap_or(Value::Null);
    json!({
        "title": get("title"),
        "space": get("space"),
        "entityCount": get("entityCount"),
        "counts": get("counts"),
        "extents": get("extents"),
        "currentLayer": get("currentLayer"),
        "layers": list("layers", INFO_LAYERS_MAX),
        "layouts": list("layouts", INFO_LAYOUTS_MAX),
        "readNotes": list("readNotes", INFO_NOTES_MAX),
    })
}

/// The most fonts one `cadcraft:add-fonts` takes; the rest are reported as failed.
pub const MAX_FONTS_PER_MESSAGE: usize = 64;
/// The most font bytes one `cadcraft:add-fonts` takes in all.
pub const MAX_FONTS_TOTAL_BYTES: usize = 200 << 20;
/// The longest font file taken (bigger ones are reported as failed).
pub const MAX_FONT_BYTES: usize = 32 << 20;

/// Is `format` one `cadcraft:add-fonts` knows? (`ttf` and `otf`: one face per file.)
pub fn font_format_ok(format: &str) -> bool {
    matches!(format, "ttf" | "otf")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_not_given_keep_todays_behaviour() {
        assert_eq!(view_options(None, None, None), ViewOptions { read_only: false, chrome: true, zoom_controls: false });
        assert_eq!(view_options(Some(true), Some(false), Some(true)), ViewOptions { read_only: true, chrome: false, zoom_controls: true });
    }

    #[test]
    fn the_page_picks_english_and_anything_else_stays_korean() {
        use cadcraft_ui_egui::i18n::Preference;
        for en in ["en", "EN", "en-US", "en_GB", " en "] {
            assert_eq!(host_language(Some(en)), Preference::English, "{en:?}");
        }
        for other in [None, Some(""), Some("ko"), Some("ja"), Some("zh"), Some("auto"), Some("english"), Some("xx")] {
            assert_eq!(host_language(other), Preference::Korean, "{other:?}");
        }
    }

    #[test]
    fn only_a_same_origin_parent_starts_the_app() {
        assert!(same_origin(Some("https://a.test"), "https://a.test"));
        assert!(!same_origin(None, "https://a.test"));
        assert!(!same_origin(Some("https://b.test"), "https://a.test"));
        assert_eq!(decide(Some("parent"), true), Start::Run);
        assert_eq!(decide(Some("parent"), false), Start::OpenFromPpm);
        assert_eq!(decide(None, true), Start::OpenFromPpm);
    }

    #[test]
    fn only_dxf_drawings_open() {
        assert_eq!(open_refusal("Plan.DXF"), None);
        assert!(open_refusal("plan.dwg").unwrap().contains("DWG"));
        assert!(open_refusal("run.lsp").unwrap().starts_with("unsupported-format"));
        assert!(open_refusal("noext").is_some());
        assert!(open_refusal(".dxf.exe").is_some());
    }

    #[test]
    fn a_malformed_policy_changes_nothing() {
        assert_eq!(policy_overrides(r#"{"overrides":{"line":false,"circle":true,"x":1}}"#), vec![("circle".to_string(), true), ("line".to_string(), false)]);
        assert!(policy_overrides("[]").is_empty());
        assert!(policy_overrides("{}").is_empty());
        assert!(policy_overrides(r#"{"overrides":[1]}"#).is_empty());
        assert!(policy_overrides("not json").is_empty());
    }

    #[test]
    fn a_drawing_goes_to_the_host_as_dxf() {
        assert_eq!(dxf_name("plan.dxf"), "plan.dxf");
        assert_eq!(dxf_name("plan.dwg"), "plan.dxf");
        assert_eq!(dxf_name("Drawing1"), "Drawing1.dxf");
        assert_eq!(dxf_name("a.b.dxf"), "a.b.dxf");
        assert_eq!(dxf_name("  "), "Drawing.dxf");
        assert!(is_font_file(&[0, 1, 0, 0, 9]) && is_font_file(b"OTTOxx") && !is_font_file(b"ttcf") && !is_font_file(b"GIF"));
    }

    #[test]
    fn request_ids_and_purposes_are_checked() {
        assert_eq!(request_id("abc").as_deref(), Some("abc"));
        assert_eq!(request_id(""), None);
        assert_eq!(request_id(&"x".repeat(REQUEST_ID_MAX + 1)), None);
        assert_eq!(bytes_purpose("save"), Some("save"));
        assert_eq!(bytes_purpose("preview"), Some("preview"));
        assert_eq!(bytes_purpose("export"), None);
        assert_eq!(echo_purpose(&"p".repeat(500)).chars().count(), 200);
        assert!(font_format_ok("ttf") && font_format_ok("otf") && !font_format_ok("woff"));
    }

    #[test]
    fn actions_are_parsed_and_mapped_to_commands() {
        assert_eq!(parse_action("zoom-extents", None, None), Ok(Action::ZoomExtents));
        assert_eq!(parse_action("set-layout", Some("Layout1".into()), None).unwrap().command(), ("layout.set", serde_json::json!({ "name": "Layout1" })));
        assert_eq!(
            parse_action("set-layer-visible", Some("WALLS".into()), Some(false)).unwrap().command(),
            ("layer.set", serde_json::json!({ "name": "WALLS", "on": false }))
        );
        assert_eq!(parse_action("set-layer-visible", Some("WALLS".into()), None), Err("bad-action"));
        assert_eq!(parse_action("set-layout", Some(String::new()), None), Err("bad-action"));
        assert_eq!(parse_action("set-layout", Some("x".repeat(NAME_MAX + 1)), None), Err("bad-action"));
        assert_eq!(parse_action("erase-all", None, None), Err("bad-action"));
    }

    #[test]
    fn action_errors_get_a_reason() {
        assert_eq!(action_reason("layer.set: no layer `X`"), "not-found");
        assert_eq!(action_reason("this drawing is read only"), "read-only");
        assert_eq!(action_reason("boom"), "boom");
    }

    #[test]
    fn the_info_is_cut_and_capped() {
        let layers: Vec<_> = (0..INFO_LAYERS_MAX + 10).map(|i| serde_json::json!({ "name": format!("L{i}") })).collect();
        let raw = serde_json::json!({ "title": "a.dxf", "space": "Model", "entityCount": 3, "layers": layers, "layouts": ["Layout1"],
            "selection": ["1A"], "undo": ["x"], "entities": [1] });
        let info = drawing_info(&raw);
        assert_eq!(info["layers"].as_array().unwrap().len(), INFO_LAYERS_MAX);
        assert_eq!(info["layouts"], serde_json::json!(["Layout1"]));
        assert!(info.get("selection").is_none() && info.get("undo").is_none() && info.get("entities").is_none());
        assert_eq!(info["readNotes"], serde_json::json!([]));
    }
}
