//! The host's lock (PPM build): a page that embeds CADCraft limits what a drawing can do.
//!
//! Two things are held here, both off until a host installs a [`Lock`] with [`install`] (so the
//! desktop app, the CLI and the upstream web build are not affected):
//!
//! - **Document entry and exit.** The commands that open or create a drawing, store it, print or
//!   export it ([`LOCKED`], plus the interface's `ui.open`-style ids) never run: the host page owns
//!   those. Editing inside the drawing is not touched, even when it picks a local file (INSERT,
//!   IMAGE). `policy.overrides` of the host's open message can switch any other command off
//!   (`false`) or back on; a locked command stays locked whatever the policy says.
//! - **Read only.** With [`Lock::read_only`] only the commands on [`READ_ONLY_OK`] (view, query,
//!   copy, measure) and the interface's own `ui.*` commands run. Everything that records an undo
//!   step is an edit and is refused by default, so a command added later is blocked until someone
//!   classifies it (`every_command_is_classified`).
//!
//! [`refusal`] is asked where a command starts: [`Session::start`](crate::Session::start) and
//! `Session::execute`, together with the automation roots' own check
//! ([`crate::file_access::commands::refusal`]), and by the interface for its `ui.*` commands.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

/// What a host asks of the drawing it opens.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Lock {
    /// Only viewing and querying commands run.
    pub read_only: bool,
    /// The host's `policy.overrides`: `{ command id: on }`. A locked command ignores it.
    pub overrides: BTreeMap<String, bool>,
}

thread_local! {
    /// The lock in force on this thread (the app is single threaded).
    static LOCK: RefCell<Option<Lock>> = const { RefCell::new(None) };
    /// The host page is opening a drawing it sent ([`host_entry`]).
    static ENTRY: Cell<bool> = const { Cell::new(false) };
}

/// Run `f`, which opens the drawing the host page sent, with OPEN allowed: the host owns document
/// entry, so its own open is the one that must get through the lock.
pub fn host_entry<R>(f: impl FnOnce() -> R) -> R {
    let before = ENTRY.with(|e| e.replace(true));
    let r = f();
    ENTRY.with(|e| e.set(before));
    r
}

/// Put `lock` in force (`None` lifts it); returns the one it replaced, so a failed open can put
/// the earlier one back.
pub fn install(lock: Option<Lock>) -> Option<Lock> {
    LOCK.with(|l| std::mem::replace(&mut *l.borrow_mut(), lock))
}

/// The lock in force, if any.
pub fn current() -> Option<Lock> {
    LOCK.with(|l| l.borrow().clone())
}

/// Is the drawing read only?
pub fn is_read_only() -> bool {
    LOCK.with(|l| l.borrow().as_ref().is_some_and(|l| l.read_only))
}

/// The menu grade of a command: locked (never runs), off (the host switched it off) or on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grade {
    Locked,
    Off,
    On,
}

/// Commands that open or create a drawing, store it, or print and export it: the host's business.
/// Engine command ids; the interface's ids are in [`is_locked`].
pub const LOCKED: &[&str] = &[
    "new",
    "open",
    "xopen",
    "qsave",
    "saveas",
    "recover",
    "drawingrecovery",
    "layout.saveas",
    "plot",
    "plotwindow",
    "plottermanager",
    "-plottermanager",
    "preview",
    "publish",
    "pdfbook",
    "exportpdf",
    "export",
    "share",
    "wblock",
    "-wblock",
    "tableexport",
    "eattext",
    "dataextraction",
    "compareexport",
];

/// The interface's own commands that open, store, print or leave: `ui.open`, `ui.saveas`,
/// `ui.newtemplate`, `ui.sample`, `ui.quit`, File ▸ Open Recent and the update check.
const LOCKED_UI: &[&str] = &["ui.open", "ui.saveas", "ui.newtemplate", "ui.sample", "ui.quit", "ui.updates.check"];

/// The prefix of File ▸ Open Recent's items (`ui.recent.<n>`, `ui.recent.clear`).
const LOCKED_UI_PREFIX: &str = "ui.recent.";

/// Is `id` one the host never lets run?
pub fn is_locked(id: &str) -> bool {
    LOCKED.contains(&id) || LOCKED_UI.contains(&id) || id.starts_with(LOCKED_UI_PREFIX)
}

/// Commands that change nothing in the drawing and open no file, and run in a read-only drawing:
/// view, selection, queries, copy, measure and the settings that only steer the pointer.
pub const READ_ONLY_OK: &[&str] = &[
    "close",
    "closeall",
    "document.switch",
    "document.bytes",
    "copyclip",
    "copybase",
    "selectall",
    "ai_deselect",
    "select",
    "qselect",
    "qselect.info",
    "selectsimilar",
    "zoom",
    "zoom.realtime",
    "zoom.dynamic",
    "zoom.previous",
    "zoom.window",
    "zoom.in",
    "zoom.out",
    "zoom.all",
    "zoom.extents",
    "zoom.object",
    "pan",
    "-pan",
    "pan.left",
    "pan.right",
    "pan.up",
    "pan.down",
    "regen",
    "regenall",
    "redraw",
    "dview",
    "view.set",
    "view.get",
    "layout.set",
    "vports.activate",
    "blockspalette",
    "blocks.dynamic",
    "blocks.reference",
    "blocks.reference.grips",
    "blocks.list",
    "groups.list",
    "pickstyle",
    "compare",
    "compare.next",
    "compare.previous",
    "compareclose",
    "layerstate.list",
    "layout.list",
    "mspace",
    "pspace",
    "3dorbit",
    "properties",
    "properties.describe",
    "hatch.patterns",
    "style.list",
    "dimstyle.list",
    "dist",
    "id",
    "list",
    "area",
    "time",
    "status",
    "drawing.inspect",
    "entities",
    "shapes",
    "usage",
    "loops",
    "texts",
    "countlist",
    "count",
    "massprop",
    "ortho",
    "grid",
    "snap",
    "polar",
    "otrack",
    "dynmode",
    "lwdisplay",
    "isodraft",
    "isoplane",
    "transparencydisplay",
    "qpmode",
    "selectioncycling",
    "osnap",
    "getvar",
    "sysvars",
    "options",
    "menubar",
    "toolbar",
    "cal",
    "quickcalc.pick",
    "measuregeom",
    "radius.measure",
    "angle.measure",
    "volume.measure",
    "constraints.inspect",
    "constraints.dof",
];

/// Commands that record no undo step yet still change the drawing, run code or read a file, so a
/// read-only drawing refuses them. Together with [`READ_ONLY_OK`] and [`LOCKED`] they classify
/// every command that is not an undoable edit.
pub const NOUNDO_EDITS: &[&str] = &[
    "u",
    "undo",
    "redo",
    "mredo",
    "script",
    "aliasedit",
    "reinit",
    "hatchpick",
    "xref",
    "blocklibrary",
    "blocklibraries",
    "bedit",
    "bsave",
    "bsaveas",
    "bclose",
    "image",
    "appload",
    "lisp",
];

/// Does a read-only drawing run command `id`?
pub fn read_only_ok(id: &str) -> bool {
    // The interface's own commands (panels, themes, zoom bars) only steer the interface; the
    // engine commands they start are asked again.
    (id.starts_with("ui.") && !is_locked(id)) || READ_ONLY_OK.contains(&id)
}

/// The grade of command `id` under the lock in force. Without a lock everything is on.
pub fn grade(id: &str) -> Grade {
    LOCK.with(|l| {
        let l = l.borrow();
        let Some(lock) = l.as_ref() else { return Grade::On };
        if is_locked(id) {
            return Grade::Locked;
        }
        match lock.overrides.get(id) {
            Some(false) => Grade::Off,
            _ => Grade::On,
        }
    })
}

/// Why command `id` may not run now; `None` when it may. Without a lock everything may.
pub fn refusal(id: &str) -> Option<String> {
    let read_only = LOCK.with(|l| l.borrow().as_ref().map(|l| l.read_only))?;
    if id == "open" && ENTRY.with(Cell::get) {
        return None;
    }
    match grade(id) {
        Grade::Locked => return Some("this command is blocked by the host page".to_string()),
        Grade::Off => return Some("this command is switched off by the host page".to_string()),
        Grade::On => {}
    }
    if read_only && !read_only_ok(id) {
        return Some("this drawing is read only".to_string());
    }
    None
}

/// Should a menu leave command `id` out altogether (locked or switched off)? A read-only
/// drawing's editing commands stay in the menu, disabled by [`refusal`] when they run.
pub fn hidden(id: &str) -> bool {
    matches!(grade(id), Grade::Locked | Grade::Off)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn with_lock<R>(lock: Lock, f: impl FnOnce() -> R) -> R {
        let before = install(Some(lock));
        let r = f();
        install(before);
        r
    }

    /// Every command is on exactly one list the lock reads, so a command added later cannot run
    /// in a read-only drawing (or reach a host's file) unclassified: an undoable command is an
    /// edit; any other must be named here.
    #[test]
    fn every_command_is_classified() {
        for spec in crate::command_specs() {
            let (locked, ok, edit) = (LOCKED.contains(&spec.id), READ_ONLY_OK.contains(&spec.id), NOUNDO_EDITS.contains(&spec.id));
            if spec.undoable {
                assert!(!ok, "`{}` records an undo step, so it is an edit and can't be on READ_ONLY_OK", spec.id);
                assert!(!edit, "`{}` records an undo step: NOUNDO_EDITS is for the ones that do not", spec.id);
            } else {
                let n = [locked, ok, edit].iter().filter(|b| **b).count();
                assert_eq!(n, 1, "`{}` is on {n} of LOCKED, READ_ONLY_OK and NOUNDO_EDITS: put it on one in hostlock.rs", spec.id);
            }
        }
        let specs = crate::command_specs();
        for id in LOCKED.iter().chain(READ_ONLY_OK).chain(NOUNDO_EDITS) {
            assert!(specs.iter().any(|s| s.id == *id), "`{id}` is listed in hostlock.rs but no longer a command");
        }
    }

    /// No file-writing or file-reading command slipped past: every command the automation roots
    /// know as touching files by an export or a print route is locked or refused in a read-only drawing.
    #[test]
    fn nothing_that_stores_a_drawing_is_read_only_ok() {
        for id in ["qsave", "saveas", "open", "plot", "export", "exportpdf", "wblock", "new"] {
            assert!(is_locked(id), "{id}");
            assert!(!READ_ONLY_OK.contains(&id), "{id}");
        }
    }

    #[test]
    fn without_a_lock_everything_runs() {
        install(None);
        assert_eq!(refusal("open"), None);
        assert_eq!(refusal("line"), None);
        assert!(!is_read_only());
        assert_eq!(grade("open"), Grade::On);
    }

    #[test]
    fn document_entry_and_exit_is_locked_and_the_policy_cannot_unlock_it() {
        let lock = Lock { read_only: false, overrides: BTreeMap::from([("open".to_string(), true), ("ui.saveas".to_string(), true)]) };
        with_lock(lock, || {
            assert!(refusal("open").unwrap().contains("host"));
            assert!(refusal("saveas").is_some());
            assert!(refusal("ui.open").is_some());
            assert!(refusal("ui.saveas").is_some());
            assert!(refusal("ui.recent.3").is_some());
            assert!(refusal("plot").is_some());
            assert!(hidden("open") && hidden("ui.recent.clear"));
            // Editing is not touched, even when it picks a file.
            assert_eq!(refusal("line"), None);
            assert_eq!(refusal("insert"), None);
            assert_eq!(refusal("image"), None);
            assert_eq!(refusal("ui.dialog.layers"), None);
            assert_eq!(refusal("document.bytes"), None);
        });
    }

    #[test]
    fn the_policy_switches_commands_off_and_on() {
        let lock = Lock { read_only: false, overrides: BTreeMap::from([("circle".to_string(), false), ("line".to_string(), true)]) };
        with_lock(lock, || {
            assert!(refusal("circle").unwrap().contains("switched off"));
            assert_eq!(grade("circle"), Grade::Off);
            assert!(hidden("circle"));
            assert_eq!(refusal("line"), None);
        });
    }

    #[test]
    fn a_read_only_drawing_runs_only_view_and_query_commands() {
        with_lock(Lock { read_only: true, ..Lock::default() }, || {
            assert!(is_read_only());
            for id in ["zoom.extents", "pan", "list", "dist", "copyclip", "select", "ui.dialog.find", "ui.toggle.statusbar", "document.bytes"] {
                assert_eq!(refusal(id), None, "{id}");
            }
            for id in ["line", "erase", "move", "undo", "lisp", "cutclip", "pasteclip", "setvar", "layer", "ucs", "made.up"] {
                assert!(refusal(id).unwrap().contains("read only"), "{id}");
            }
            assert!(refusal("open").unwrap().contains("host"));
        });
    }

    #[test]
    fn the_hosts_own_open_gets_through_the_lock_and_nothing_else_does() {
        with_lock(Lock { read_only: true, ..Lock::default() }, || {
            assert!(refusal("open").is_some());
            host_entry(|| {
                assert_eq!(refusal("open"), None);
                assert!(refusal("saveas").is_some());
                assert!(refusal("line").is_some());
            });
            assert!(refusal("open").is_some());
        });
    }

    /// A command the lock refuses leaves the drawing as it was; and an edit that got past the check
    /// is taken back by the backstop, so the drawing never counts as changed.
    #[test]
    fn a_read_only_session_refuses_edits_and_discards_what_slipped_through() {
        let mut s = crate::Session::new();
        with_lock(Lock { read_only: true, ..Lock::default() }, || {
            let e = s.execute("line", &serde_json::json!({"points": [[0, 0], [10, 0]]})).unwrap_err();
            assert!(e.to_string().contains("read only"), "{e}");
            assert!(s.start("circle").is_err());
            assert!(!s.state().unwrap().is_dirty());
            assert!(!s.discard_unsaved_changes());
        });
        // Without the lock the edit goes in; the backstop then takes it back.
        s.execute("line", &serde_json::json!({"points": [[0, 0], [10, 0]]})).unwrap();
        assert!(s.state().unwrap().is_dirty());
        assert!(s.discard_unsaved_changes());
        assert!(!s.state().unwrap().is_dirty());
        assert_eq!(s.doc().unwrap().entity_count(), 0);
    }

    #[test]
    fn a_failed_open_can_put_the_earlier_lock_back() {
        let first = Lock { read_only: true, ..Lock::default() };
        install(Some(first.clone()));
        let previous = install(Some(Lock::default()));
        assert_eq!(previous, Some(first.clone()));
        assert!(!is_read_only());
        install(previous);
        assert_eq!(current(), Some(first));
        install(None);
    }
}
