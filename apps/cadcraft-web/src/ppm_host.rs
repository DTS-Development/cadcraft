//! The PPM build's bridge to the page that frames CADCraft in an `<iframe>` (`?host=parent`).
//! It replaces `host.rs`, which talks the generic `craft-web-host` contract; this one adds what
//! the PPM page needs and refuses everything else. Messages are plain objects with a `type`:
//!
//! - `cadcraft:ready` (to the parent): `{ version, openOptions }`, the app is listening. The
//!   `openOptions` (`readOnly`, `chrome`, `zoomControls`) are the ones this build understands.
//! - `cadcraft:open` (from the parent): `{ name, bytes, policy, readOnly, chrome, zoomControls }`,
//!   `bytes` an `ArrayBuffer` or `Uint8Array` holding a DXF drawing (DWG is converted to DXF on
//!   the server first). The three booleans belong to that drawing; a missing one is today's
//!   behaviour. `readOnly` lets nothing edit or save it; `chrome: false` shows the drawing alone;
//!   `zoomControls` keeps a zoom bar then. `policy.overrides` is `{ "<command id>": true | false }`
//!   (see `commands.json`); a malformed policy changes nothing. An open **replaces** the drawing
//!   (the page has one drawing, so one lock): the earlier drawing and its unsaved changes are
//!   closed, and an open that fails leaves it. Several opens in one frame: the last one is
//!   opened and the others fail with `superseded:`.
//! - `cadcraft:failed` (to the parent): `{ error }` when the app couldn't start or a drawing
//!   couldn't be opened (the `error` starts with a code such as `unsupported-format:`). A failed
//!   open leaves the earlier drawing and its options as they were.
//! - `cadcraft:dirty` (to the parent): `{ dirty }`, never true for a read-only drawing.
//! - `cadcraft:request-bytes` (from the parent): `{ id, purpose }`, `purpose` `save`, `check` or
//!   `preview`: the drawing as it is now, as DXF. `cadcraft:bytes`: `{ id, purpose, name, bytes }`.
//!   With `save` the drawing counts as saved. `cadcraft:bytes-failed`: `{ id, purpose, reason }`,
//!   `reason` `not-ready`, `read-only`, `bad-purpose` or an error text.
//! - `cadcraft:opened` (to the parent): `{}`, a drawing was opened and is drawn from the next frame.
//! - `cadcraft:request-info` (from the parent): `{ id }`, `cadcraft:info`: `{ id, info }`, `info`
//!   the JSON text of the drawing summary ([`gate::drawing_info`]); `cadcraft:info-failed`:
//!   `{ id, reason }`.
//! - `cadcraft:action` (from the parent): `{ id, action, name?, visible? }`, `action`
//!   `set-layer-visible` (`name`, `visible`), `set-layout` (`name`, `Model` or a layout) or
//!   `zoom-extents`; `cadcraft:action-result`: `{ id, ok, reason? }`, `reason` `not-ready`,
//!   `read-only` (a layer change in a read-only drawing), `not-found`, `bad-action` or an error
//!   text. These calls ignore the host's menu overrides; only a read-only drawing refuses.
//! - `cadcraft:add-fonts` (from the parent): `{ id, fonts: [{ family, bytes, format }] }` and
//!   `cadcraft:fonts-added`: `{ id, installed, failed }`.
//!
//! Only the parent window on the page's own origin is heard or answered. The app never saves or
//! downloads by itself: the host's lock (`cadcraft_engine::hostlock`) blocks open, save, print
//! and external links, and the page stores what it asks for with `request-bytes`.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use cadcraft_engine::hostlock::{self, Lock};
use cadcraft_ui_egui::{CadApp, HostView};
use wasm_bindgen::JsCast as _;
use wasm_bindgen::JsValue;
use wasm_bindgen::closure::Closure;

use crate::gate::{self, ViewOptions};

/// A `cadcraft:open` waiting for the next frame.
struct PendingOpen {
    name: String,
    bytes: Vec<u8>,
    view: ViewOptions,
    overrides: Vec<(String, bool)>,
}

thread_local! {
    /// Lower-cased families added by `cadcraft:add-fonts` (a family is kept once).
    static ADDED_FONTS: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
}

#[derive(Clone)]
pub struct Bridge {
    window: web_sys::Window,
    parent: web_sys::Window,
    origin: String,
    opens: Rc<RefCell<Vec<PendingOpen>>>,
    byte_requests: Rc<RefCell<Vec<(String, &'static str)>>>,
    actions: Rc<RefCell<Vec<(String, gate::Action)>>>,
    info_requests: Rc<RefCell<Vec<String>>>,
    /// The `uid` of the drawing the parent opened: the one `request-bytes`, `request-info` and
    /// `action` answer for. A parent's open replaces the drawing, so it is the only one.
    host_doc: Rc<Cell<Option<u64>>>,
    /// Fonts were added: the next frame regenerates the drawings with them.
    regen: Rc<Cell<bool>>,
    /// The unsaved-changes state last reported to the parent.
    reported: Rc<Cell<Option<bool>>>,
}

impl Bridge {
    /// The bridge, or `None` when the page isn't `?host=parent` inside a same-origin parent (the
    /// PPM build then shows a notice and nothing else).
    pub fn connect() -> Option<Self> {
        let window = web_sys::window()?;
        let params = web_sys::UrlSearchParams::new_with_str(&window.location().search().unwrap_or_default()).ok()?;
        let parent = window.parent().ok().flatten().filter(|p| !js_sys::Object::is(p, &window));
        let origin = window.location().origin().ok()?;
        let same = parent.as_ref().is_some_and(|p| gate::same_origin(p.location().origin().ok().as_deref(), &origin));
        if gate::decide(params.get("host").as_deref(), same) != gate::Start::Run {
            return None;
        }
        Some(Self {
            window,
            parent: parent?,
            origin,
            opens: Rc::default(),
            byte_requests: Rc::default(),
            actions: Rc::default(),
            info_requests: Rc::default(),
            host_doc: Rc::default(),
            regen: Rc::default(),
            reported: Rc::default(),
        })
    }

    pub fn install(&self) {}

    pub fn post_ready(&self) {
        let options: js_sys::Array = gate::OPEN_OPTIONS.iter().map(|o| JsValue::from_str(o)).collect();
        self.send(&[("type", "cadcraft:ready".into()), ("version", env!("CARGO_PKG_VERSION").into()), ("openOptions", options.into())]);
    }

    pub fn post_failed(&self, error: &str) {
        self.send(&[("type", "cadcraft:failed".into()), ("error", error.into())]);
    }

    /// Before each frame: open what arrived, answer the byte requests, and take back any edit that
    /// got past a read-only drawing's command check.
    pub fn pump(&self, app: &mut CadApp) {
        // Opens that pile up in one frame: the last one replaces the drawing, and the ones it
        // replaced are told so (each open gets exactly one answer, and a big drawing is not
        // parsed only to be thrown away).
        let mut opens = std::mem::take(&mut *self.opens.borrow_mut());
        if let Some(last) = opens.pop() {
            for _ in 0..opens.len() {
                self.post_failed("superseded: a newer open arrived before this one was drawn");
            }
            self.open(app, last);
        }
        // The host's drawing is the only one: a command that added another (a locked command is
        // refused, but not every path is a command) is closed again, so no lock, tab or byte
        // request can reach a second drawing.
        if let Some(uid) = self.host_doc.get() {
            app.host_only(uid);
        }
        if hostlock::is_read_only() {
            app.session.discard_unsaved_changes();
        }
        if self.regen.take() {
            // Drawings take the added fonts; a refusal (a read-only drawing) leaves them as drawn.
            // The page's own call: the menu switches do not apply.
            let _ = hostlock::as_host(|| app.run("regenall", serde_json::json!({})));
        }
        for (id, purpose) in std::mem::take(&mut *self.byte_requests.borrow_mut()) {
            self.answer_bytes(app, id, purpose);
        }
        for id in std::mem::take(&mut *self.info_requests.borrow_mut()) {
            self.answer_info(app, id);
        }
        for (id, action) in std::mem::take(&mut *self.actions.borrow_mut()) {
            self.answer_action(app, id, &action);
        }
    }

    /// Open one drawing under the options that came with it: it replaces the drawing before it.
    /// A failed open leaves the earlier drawing, lock and view as they were and tells the parent.
    fn open(&self, app: &mut CadApp, open: PendingOpen) {
        let lock = Lock { read_only: open.view.read_only, overrides: open.overrides.into_iter().collect() };
        let view = HostView { chrome: open.view.chrome, zoom_controls: open.view.zoom_controls };
        match app.host_open(&open.name, &open.bytes, lock, view) {
            Ok(uid) => {
                self.host_doc.set(Some(uid));
                self.send(&[("type", "cadcraft:opened".into())]);
            }
            Err(error) => {
                self.post_failed(&error);
            }
        }
    }

    /// After each logic callback: tell the parent when unsaved changes appear or go away. A
    /// read-only drawing is never dirty.
    pub fn report_dirty(&self, app: &CadApp) {
        let dirty = self.host_doc.get().is_some_and(|uid| app.host_dirty(uid));
        if self.reported.get() != Some(dirty) && self.send(&[("type", "cadcraft:dirty".into()), ("dirty", dirty.into())]) {
            self.reported.set(Some(dirty));
        }
    }

    /// The parent's drawing, when it is still the one on screen (the user can close it).
    fn host_uid(&self, app: &CadApp) -> Option<u64> {
        self.host_doc.get().filter(|uid| app.host_has(*uid))
    }

    fn answer_info(&self, app: &mut CadApp, id: String) {
        let result = if self.host_uid(app).is_none() {
            Err("not-ready".to_string())
        } else {
            hostlock::as_host(|| app.run("drawing.inspect", serde_json::json!({ "entities": false })))
        };
        match result {
            Ok(inspect) => {
                let info = gate::drawing_info(&inspect).to_string();
                self.send(&[("type", "cadcraft:info".into()), ("id", id.as_str().into()), ("info", info.as_str().into())]);
            }
            Err(reason) => {
                self.send(&[("type", "cadcraft:info-failed".into()), ("id", id.as_str().into()), ("reason", reason.as_str().into())]);
            }
        }
    }

    fn answer_action(&self, app: &mut CadApp, id: String, action: &gate::Action) {
        let result = self.run_action(app, action);
        let mut fields = vec![("type", "cadcraft:action-result".into()), ("id", id.as_str().into()), ("ok", result.is_ok().into())];
        if let Err(reason) = &result {
            fields.push(("reason", reason.as_str().into()));
        }
        self.send(&fields);
    }

    fn run_action(&self, app: &mut CadApp, action: &gate::Action) -> Result<(), String> {
        if self.host_uid(app).is_none() {
            return Err("not-ready".into());
        }
        let (command, params) = action.command();
        if hostlock::is_read_only() && !hostlock::read_only_ok(command) {
            return Err("read-only".into());
        }
        hostlock::as_host(|| app.run(command, params)).map(|_| ()).map_err(|e| gate::action_reason(&e))
    }

    fn answer_bytes(&self, app: &mut CadApp, id: String, purpose: &'static str) {
        let sent = match self.drawing_bytes(app, purpose) {
            Ok((name, bytes)) => {
                let buffer = js_sys::Uint8Array::from(bytes.as_slice()).buffer();
                message(&[
                    ("type", "cadcraft:bytes".into()),
                    ("id", id.as_str().into()),
                    ("purpose", purpose.into()),
                    ("name", name.as_str().into()),
                    ("bytes", buffer.clone().into()),
                ])
                .is_some_and(|m| self.parent.post_message_with_transfer(&m, &self.origin, &js_sys::Array::of1(&buffer)).is_ok())
            }
            Err(reason) => self.send(&[
                ("type", "cadcraft:bytes-failed".into()),
                ("id", id.as_str().into()),
                ("purpose", purpose.into()),
                ("reason", reason.as_str().into()),
            ]),
        };
        if !sent {
            log::error!("couldn't answer the bytes request {id}");
        }
    }

    /// The parent's drawing as DXF and the name it goes under. `save` also counts the drawing as
    /// saved (the host stores what it is handed); `check` and `preview` change nothing. The bytes
    /// go out as written: no base64 round trip through the command's JSON reply.
    fn drawing_bytes(&self, app: &mut CadApp, purpose: &str) -> Result<(String, Vec<u8>), String> {
        let uid = self.host_uid(app).ok_or("not-ready")?;
        let (title, bytes) = app.host_bytes(uid, purpose == "save")?;
        Ok((gate::dxf_name(&title), bytes))
    }

    /// Hear the parent. Only its own origin and window count.
    pub fn listen(&self, ctx: egui::Context) {
        let (parent, origin) = (self.parent.clone(), self.origin.clone());
        let (opens, byte_requests, regen) = (self.opens.clone(), self.byte_requests.clone(), self.regen.clone());
        let (actions, info_requests) = (self.actions.clone(), self.info_requests.clone());
        let me = self.clone();
        let on_message = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |event: web_sys::MessageEvent| {
            if event.origin() != origin || !event.source().is_some_and(|s| js_sys::Object::is(&s, &parent)) {
                return;
            }
            let data = event.data();
            match string(&data, "type").as_deref() {
                Some("cadcraft:open") => {
                    let name = string(&data, "name").map(|n| n.trim().chars().take(255).collect::<String>()).filter(|n| !n.is_empty());
                    let name = name.unwrap_or_else(|| "Drawing.dxf".to_string());
                    // Refused before the bytes are copied.
                    if let Some(why) = gate::open_refusal(&name) {
                        return me.post_failed(why);
                    }
                    if let Some(why) = byte_len(&data).and_then(|len| CadApp::open_size_error(&name, len as u64)) {
                        return me.post_failed(&why);
                    }
                    let Some(bytes) = bytes(&data) else { return me.post_failed("bad-open: the message has no bytes") };
                    let view = gate::view_options(boolean(&data, "readOnly"), boolean(&data, "chrome"), boolean(&data, "zoomControls"));
                    opens.borrow_mut().push(PendingOpen { name, bytes, view, overrides: policy_overrides(&data) });
                }
                Some("cadcraft:request-bytes") => {
                    let Some(id) = string(&data, "id").and_then(|id| gate::request_id(&id)) else { return };
                    let raw = string(&data, "purpose").unwrap_or_default();
                    match gate::bytes_purpose(&raw) {
                        Some(purpose) => byte_requests.borrow_mut().push((id, purpose)),
                        None => {
                            let fields = [
                                ("type", "cadcraft:bytes-failed".into()),
                                ("id", id.as_str().into()),
                                ("purpose", gate::echo_purpose(&raw).as_str().into()),
                                ("reason", "bad-purpose".into()),
                            ];
                            me.send(&fields);
                        }
                    }
                }
                Some("cadcraft:request-info") => {
                    let Some(id) = string(&data, "id").and_then(|id| gate::request_id(&id)) else { return };
                    info_requests.borrow_mut().push(id);
                }
                Some("cadcraft:action") => {
                    let Some(id) = string(&data, "id").and_then(|id| gate::request_id(&id)) else { return };
                    let kind = string(&data, "action").unwrap_or_default();
                    match gate::parse_action(&kind, string(&data, "name"), boolean(&data, "visible")) {
                        Ok(action) => actions.borrow_mut().push((id, action)),
                        Err(reason) => {
                            me.send(&[("type", "cadcraft:action-result".into()), ("id", id.as_str().into()), ("ok", false.into()), ("reason", reason.into())]);
                        }
                    }
                }
                Some("cadcraft:add-fonts") => {
                    let Some(id) = string(&data, "id").and_then(|id| gate::request_id(&id)) else { return };
                    let (installed, failed) = add_fonts(&data);
                    if !installed.is_empty() {
                        regen.set(true);
                    }
                    let list = |names: Vec<String>| names.iter().map(|n| JsValue::from_str(n)).collect::<js_sys::Array>();
                    me.send(&[
                        ("type", "cadcraft:fonts-added".into()),
                        ("id", id.as_str().into()),
                        ("installed", list(installed).into()),
                        ("failed", list(failed).into()),
                    ]);
                }
                _ => return,
            }
            ctx.request_repaint();
        });
        if self.window.add_event_listener_with_callback("message", on_message.as_ref().unchecked_ref()).is_ok() {
            // The listener lives as long as the page.
            on_message.forget();
        }
        // Ctrl/Cmd+S is the host's to answer, never the browser's "save page".
        let on_key = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(|event: web_sys::KeyboardEvent| {
            if (event.ctrl_key() || event.meta_key()) && event.key().eq_ignore_ascii_case("s") {
                event.prevent_default();
            }
        });
        if self.window.add_event_listener_with_callback("keydown", on_key.as_ref().unchecked_ref()).is_ok() {
            on_key.forget();
        }
    }

    /// Post a message to the parent; false if it couldn't be sent.
    fn send(&self, fields: &[(&str, JsValue)]) -> bool {
        message(fields).is_some_and(|m| self.parent.post_message(&m, &self.origin).is_ok())
    }
}

/// The engine's delivery hook: the PPM build never hands files out by itself (save is locked and
/// the page asks for the bytes), so anything that gets here is refused.
pub fn deliver(_name: &str, _mime: &str, _bytes: &[u8]) -> Result<(), String> {
    Err("this command is blocked by the host page".into())
}

/// Installs the fonts of a `cadcraft:add-fonts` message; returns the families installed (one
/// already there counts) and the ones that failed (no name, wrong format, too big, not a font).
fn add_fonts(data: &JsValue) -> (Vec<String>, Vec<String>) {
    let (mut installed, mut failed) = (Vec::new(), Vec::new());
    let Some(list) = js_sys::Reflect::get(data, &JsValue::from_str("fonts")).ok().filter(js_sys::Array::is_array) else {
        return (installed, failed);
    };
    // Bytes taken so far: the whole message is capped, not only each file.
    let mut taken = 0usize;
    for (i, item) in js_sys::Array::from(&list).iter().enumerate() {
        let family = string(&item, "family").unwrap_or_default();
        let key = family.trim().to_lowercase();
        if key.is_empty() || i >= gate::MAX_FONTS_PER_MESSAGE || !string(&item, "format").is_some_and(|f| gate::font_format_ok(&f)) {
            failed.push(family);
        } else if ADDED_FONTS.with(|f| f.borrow().contains(&key)) {
            installed.push(family);
        } else {
            // The size is read off the JS buffer, so a too-big font is never copied.
            let len = byte_len(&item).filter(|len| *len <= gate::MAX_FONT_BYTES && taken + len <= gate::MAX_FONTS_TOTAL_BYTES);
            match len.and_then(|len| bytes(&item).map(|b| (len, b))).filter(|(_, b)| gate::is_font_file(b)) {
                Some((len, b)) => {
                    cadcraft_fonts::ttf::register(family.trim(), b);
                    ADDED_FONTS.with(|f| f.borrow_mut().insert(key));
                    taken += len;
                    installed.push(family);
                }
                None => failed.push(family),
            }
        }
    }
    (installed, failed)
}

/// The length of `bytes` (an `ArrayBuffer` or `Uint8Array`) without copying it.
fn byte_len(data: &JsValue) -> Option<usize> {
    let bytes = js_sys::Reflect::get(data, &JsValue::from_str("bytes")).ok()?;
    if let Some(array) = bytes.dyn_ref::<js_sys::Uint8Array>() {
        Some(array.length() as usize)
    } else {
        bytes.dyn_ref::<js_sys::ArrayBuffer>().map(|b| b.byte_length() as usize)
    }
}

fn message(fields: &[(&str, JsValue)]) -> Option<js_sys::Object> {
    let m = js_sys::Object::new();
    for (key, value) in fields {
        js_sys::Reflect::set(&m, &JsValue::from_str(key), value).ok()?;
    }
    Some(m)
}

/// `policy.overrides` as `(command id, on)` pairs (see [`gate::policy_overrides`]).
fn policy_overrides(data: &JsValue) -> Vec<(String, bool)> {
    let text = js_sys::Reflect::get(data, &JsValue::from_str("policy"))
        .ok()
        .and_then(|policy| js_sys::JSON::stringify(&policy).ok())
        .and_then(|text| text.as_string());
    text.map(|text| gate::policy_overrides(&text)).unwrap_or_default()
}

/// A field that is a boolean; anything else counts as not given.
fn boolean(data: &JsValue, key: &str) -> Option<bool> {
    js_sys::Reflect::get(data, &JsValue::from_str(key)).ok()?.as_bool()
}

fn string(data: &JsValue, key: &str) -> Option<String> {
    js_sys::Reflect::get(data, &JsValue::from_str(key)).ok()?.as_string()
}

/// `bytes` as an `ArrayBuffer` or `Uint8Array`.
fn bytes(data: &JsValue) -> Option<Vec<u8>> {
    let bytes = js_sys::Reflect::get(data, &JsValue::from_str("bytes")).ok()?;
    if let Some(array) = bytes.dyn_ref::<js_sys::Uint8Array>() {
        Some(array.to_vec())
    } else if bytes.is_instance_of::<js_sys::ArrayBuffer>() {
        Some(js_sys::Uint8Array::new(&bytes).to_vec())
    } else {
        None
    }
}
