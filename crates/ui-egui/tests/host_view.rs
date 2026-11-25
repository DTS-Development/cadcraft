//! What a host page's open options do to the interface: the lock keeps document entry and exit
//! out of the menus and the commands, a failed open is reported, and `chrome` / `zoomControls`
//! decide what is drawn around the drawing.

use cadcraft_engine::Session;
use cadcraft_engine::hostlock::{self, Lock};
use cadcraft_ui_egui::menus::{self, Entry};
use cadcraft_ui_egui::{CadApp, HostView, Services};
use egui::accesskit::Role;
use egui::vec2;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

fn ids(entries: &[Entry], out: &mut Vec<String>) {
    for e in entries {
        match e {
            Entry::Item { id, .. } => out.push(id.clone()),
            Entry::Sub { children, .. } => ids(children, out),
        }
    }
}

/// The file readers and writers the app registers at start (the engine has none of its own).
fn file_io() {
    cadcraft_engine::cmd::file::set_io(cadcraft_engine::cmd::file::IoHooks {
        read: |b, name| cadcraft_io::read(b, name).map_err(|e| e.to_string()),
        write: |d, name| cadcraft_io::write(d, name).map_err(|e| e.to_string()),
        plot: None,
    });
}

fn menu_ids(app: &CadApp) -> Vec<String> {
    let mut out = Vec::new();
    for (_, entries) in menus::tree(app) {
        ids(&entries, &mut out);
    }
    out
}

#[test]
fn the_host_view_defaults_to_the_plain_interface() {
    let app = CadApp::new(Session::default(), Services::default());
    assert_eq!(app.host_view, HostView { chrome: true, zoom_controls: false });
}

#[test]
fn a_lock_takes_document_entry_and_exit_out_of_the_menus_and_stops_the_commands() {
    let app = CadApp::new(Session::default(), Services::default());
    let open = menu_ids(&app);
    assert!(open.iter().any(|i| i == "ui.newtemplate") && open.iter().any(|i| i == "qsave"), "{open:?}");

    hostlock::install(Some(Lock::default()));
    let locked = menu_ids(&app);
    for id in ["qsave", "plot", "ui.newtemplate", "ui.sample"] {
        assert!(!locked.iter().any(|i| i == id), "{id} is still in the menus");
    }
    assert!(locked.iter().any(|i| i == "line"), "editing commands stay");

    let mut app = app;
    let refused = menus::run_ui_command(&mut app, "ui.open", &serde_json::json!({}));
    assert!(matches!(refused, Some(Err(ref why)) if why.contains("host page")), "{refused:?}");
    assert!(app.run("qsave", serde_json::json!({})).is_err());
    hostlock::install(None);
}

#[test]
fn a_command_the_policy_switches_off_leaves_the_menus() {
    let app = CadApp::new(Session::default(), Services::default());
    hostlock::install(Some(Lock { read_only: false, overrides: [("circle".to_string(), false)].into() }));
    let ids = menu_ids(&app);
    hostlock::install(None);
    assert!(!ids.iter().any(|i| i == "circle") && ids.iter().any(|i| i == "line"));
}

#[test]
fn a_failed_open_is_reported_once_and_a_good_one_is_not() {
    file_io();
    let mut app = CadApp::new(Session::empty(), Services::default());
    hostlock::host_entry(|| app.open_bytes("broken.dxf", b"this is not a drawing"));
    let errors = app.take_open_errors();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(app.take_open_errors().is_empty(), "taken once");

    let dxf = {
        let mut s = Session::new();
        s.execute("line", &serde_json::json!({"from": [0, 0], "to": [10, 0]})).ok();
        let r = s.execute("document.bytes", &serde_json::json!({"format": "dxf"})).expect("dxf bytes");
        cadcraft_engine::cmd::file::base64_decode(r["data"].as_str().expect("data")).expect("base64")
    };
    hostlock::host_entry(|| app.open_bytes("good.dxf", &dxf));
    assert!(app.take_open_errors().is_empty());
}

fn shown(view: HostView) -> Harness<'static, CadApp> {
    let mut app = CadApp::new(Session::default(), Services::default());
    app.run("new", serde_json::json!({})).expect("new");
    app.ui.start_tab = false;
    app.host_view = view;
    let mut h = Harness::builder().with_size(vec2(1200.0, 760.0)).build_ui_state(
        |ui, app: &mut CadApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(5);
    h
}

#[test]
fn chrome_off_draws_the_drawing_alone_and_zoom_controls_bring_back_the_zoom_bar() {
    let plain = shown(HostView { chrome: true, zoom_controls: false });
    assert!(plain.query_by_role_and_label(Role::Button, "Extents").is_none());
    assert!(plain.query_all_by_role(Role::Tab).count() > 0, "the file tabs are drawn with the chrome");

    let alone = shown(HostView { chrome: false, zoom_controls: false });
    assert_eq!(alone.query_all_by_role(Role::Tab).count(), 0, "no file tabs without the chrome");
    assert!(alone.query_by_role_and_label(Role::Button, "Extents").is_none());

    let zoom = shown(HostView { chrome: false, zoom_controls: true });
    assert_eq!(zoom.query_all_by_role(Role::Tab).count(), 0);
    assert!(zoom.query_by_role_and_label(Role::Button, "Extents").is_some(), "the zoom bar is drawn");
}
