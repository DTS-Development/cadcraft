//! The PPM build's host rules that need no browser: which parent page is trusted, which drawings
//! and open options are taken, and how the host's policy is read. Kept apart from `ppm_host.rs`
//! so `cargo test` covers them on the desktop.
#![cfg_attr(not(feature = "ppm-host"), allow(dead_code))]

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
}
