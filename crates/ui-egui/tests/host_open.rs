//! A host page's open replaces the drawing: there is one drawing and one lock, so a read-only
//! drawing cannot be edited or handed out through a later open, an earlier drawing's unsaved
//! edits are not mixed into the next one, and the page's byte requests answer for the drawing it
//! opened and no other.

use cadcraft_engine::Session;
use cadcraft_engine::hostlock::{self, Lock};
use cadcraft_ui_egui::{CadApp, HostView, Services};
use serde_json::json;

/// The file readers and writers the app registers at start (the engine has none of its own).
fn file_io() {
    cadcraft_engine::cmd::file::set_io(cadcraft_engine::cmd::file::IoHooks {
        read: |b, name| cadcraft_io::read(b, name).map_err(|e| e.to_string()),
        write: |d, name| cadcraft_io::write(d, name).map_err(|e| e.to_string()),
        plot: None,
    });
}

/// A DXF with `lines` lines in it.
fn dxf(lines: usize) -> Vec<u8> {
    let mut s = Session::new();
    for i in 0..lines {
        s.execute("line", &json!({"points": [[0, i], [10, i]]})).expect("line");
    }
    cadcraft_engine::cmd::file::document_bytes(&mut s, &json!({"format": "dxf"})).expect("dxf bytes").0
}

fn app() -> CadApp {
    file_io();
    hostlock::install(None);
    CadApp::new(Session::empty(), Services::default())
}

fn plain() -> (Lock, HostView) {
    (Lock::default(), HostView::default())
}

fn read_only() -> (Lock, HostView) {
    (Lock { read_only: true, ..Lock::default() }, HostView::default())
}

fn open(app: &mut CadApp, name: &str, bytes: &[u8], (lock, view): (Lock, HostView)) -> u64 {
    app.host_open(name, bytes, lock, view).expect("open")
}

fn edit(app: &mut CadApp) {
    app.session.execute("line", &json!({"points": [[0, 0], [5, 5]]})).expect("edit");
}

/// Entities in the drawing the page's save would hand out.
fn entities(app: &CadApp) -> usize {
    app.session.doc().expect("a drawing").entity_count()
}

#[test]
fn read_only_a_then_default_b_leaves_only_b_and_unlocks_nothing_of_a() {
    let mut app = app();
    let (a_bytes, b_bytes) = (dxf(1), dxf(3));
    let a = open(&mut app, "a.dxf", &a_bytes, read_only());
    assert!(hostlock::is_read_only());
    let b = open(&mut app, "b.dxf", &b_bytes, plain());
    assert_ne!(a, b);
    assert!(!hostlock::is_read_only(), "b was opened without read-only");
    assert_eq!(app.session.docs.len(), 1, "one drawing");
    assert!(!app.host_has(a), "a is gone, so no tab can switch back to it");
    assert!(app.host_has(b));
    let (title, bytes) = app.host_bytes(b, true).expect("save b");
    assert_eq!(title, "b.dxf");
    assert!(!bytes.is_empty());
    assert_eq!(entities(&app), 3, "the drawing handed out is b");
    assert_eq!(app.host_bytes(a, true).unwrap_err(), "not-ready");
}

#[test]
fn unsaved_a_then_read_only_b_replaces_a_and_b_is_never_dirty() {
    let mut app = app();
    let (a_bytes, b_bytes) = (dxf(1), dxf(2));
    let a = open(&mut app, "a.dxf", &a_bytes, plain());
    edit(&mut app);
    assert!(app.host_dirty(a));
    let b = open(&mut app, "b.dxf", &b_bytes, read_only());
    assert_eq!(app.session.docs.len(), 1);
    assert!(!app.host_has(a));
    assert!(!app.host_dirty(b), "a read-only drawing is never dirty");
    assert!(!app.host_dirty(a), "a's edits are not reported on b's behalf");
    assert_eq!(app.host_bytes(b, true).unwrap_err(), "read-only", "a read-only drawing is not saved");
    let (title, _) = app.host_bytes(b, false).expect("check b");
    assert_eq!(title, "b.dxf");
    // The backstop only has b to take back, and b has nothing.
    assert!(!app.session.discard_unsaved_changes());
    assert_eq!(entities(&app), 2);
}

#[test]
fn byte_requests_answer_only_for_the_drawing_the_host_opened() {
    let mut app = app();
    let a = open(&mut app, "a.dxf", &dxf(1), plain());
    // A drawing the user adds some other way (a new tab) is not the host's.
    app.session.new_drawing(false);
    assert!(!app.host_has(a));
    assert_eq!(app.host_bytes(a, false).unwrap_err(), "not-ready");
    assert_eq!(app.host_bytes(a, true).unwrap_err(), "not-ready");
    assert!(app.host_only(a), "the extra drawing is closed");
    assert!(app.host_has(a));
    assert_eq!(app.session.docs.len(), 1);
    assert!(!app.host_only(a), "nothing more to close");
    app.host_bytes(a, true).expect("a again");
}

#[test]
fn closing_the_hosts_drawing_leaves_nothing_to_hand_out() {
    let mut app = app();
    let a = open(&mut app, "a.dxf", &dxf(1), plain());
    app.session.execute("closeall", &json!({})).expect("closeall");
    assert_eq!(app.host_bytes(a, false).unwrap_err(), "not-ready");
    assert!(!app.host_only(a), "an unknown drawing closes nothing");
}

#[test]
fn a_failed_open_keeps_the_earlier_drawing_lock_and_view() {
    let mut app = app();
    let a = open(&mut app, "a.dxf", &dxf(1), read_only());
    let err = app.host_open("broken.dxf", b"this is not a drawing", Lock::default(), HostView { chrome: false, zoom_controls: true }).unwrap_err();
    assert!(!err.is_empty());
    assert!(app.host_has(a));
    assert_eq!(app.session.docs.len(), 1);
    assert!(hostlock::is_read_only(), "the earlier lock is back");
    assert_eq!(app.host_view, HostView::default());
}

#[test]
fn the_pages_save_works_with_the_save_menu_switched_off() {
    let mut app = app();
    let overrides = [("document.bytes".to_string(), false), ("qsave".to_string(), false), ("regenall".to_string(), false)].into();
    let a = open(&mut app, "a.dxf", &dxf(1), (Lock { read_only: false, overrides }, HostView::default()));
    assert!(app.run("document.bytes", json!({"format": "dxf"})).is_err(), "the menu switch holds for the user");
    edit(&mut app);
    assert!(app.host_dirty(a));
    let (_, bytes) = app.host_bytes(a, true).expect("the page's call ignores the menu switch");
    assert!(!bytes.is_empty());
    assert!(!app.host_dirty(a), "a save counts as saved");
    assert!(app.run("document.bytes", json!({"format": "dxf"})).is_err(), "and the switch is still on afterwards");
}

#[test]
fn the_bytes_are_what_document_bytes_writes() {
    let mut app = app();
    let a = open(&mut app, "a.dxf", &dxf(2), plain());
    let (_, bytes) = app.host_bytes(a, false).expect("bytes");
    let via_command = app.run("document.bytes", json!({"format": "dxf"})).expect("command");
    let data = via_command["data"].as_str().expect("data");
    assert_eq!(cadcraft_engine::cmd::file::base64_decode(data).expect("base64"), bytes);
}
