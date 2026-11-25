//! The app's side of the PPM host lock (the engine's is `cadcraft_engine::hostlock`): the list the
//! administrator's screen reads, `ppm/commands.json`. Every engine command and every interface
//! command has a grade: `locked` ones never run in the PPM build; the rest are `on`, and a
//! parent's `policy.overrides` can switch them off.

use cadcraft_engine::hostlock::{self, Lock};

use crate::{CadApp, HostView};

/// A link to `url`, or only its text when a host page holds the app (an external link leaves the
/// document, which the lock keeps with the host).
pub(crate) fn link(ui: &mut egui::Ui, text: impl Into<egui::WidgetText>, url: &str) -> egui::Response {
    if hostlock::current().is_some() { ui.label(text) } else { ui.hyperlink_to(text, url) }
}

/// What a host page does with the one drawing it opened (PPM build). The page's open replaces the
/// drawing, so there is a single drawing, and the lock (one per app) always describes it. Calls
/// that name the drawing by its `uid` find nothing once the user has closed it.
impl CadApp {
    /// Open `bytes` as the page's drawing under `lock` and `view`, and close every other drawing.
    /// A failed open leaves the earlier drawing, lock and view as they were. Returns the new
    /// drawing's `uid`.
    pub fn host_open(&mut self, name: &str, bytes: &[u8], lock: Lock, view: HostView) -> Result<u64, String> {
        let earlier_lock = hostlock::install(Some(lock));
        let earlier_view = std::mem::replace(&mut self.host_view, view);
        let before: Vec<u64> = self.session.docs.iter().map(|d| d.uid).collect();
        self.take_open_errors();
        hostlock::host_entry(|| self.open_bytes(name, bytes));
        let error = self.take_open_errors().into_iter().next();
        let opened = self.session.state().ok().map(|d| d.uid).filter(|uid| !before.contains(uid));
        match (error, opened) {
            (None, Some(uid)) => {
                self.session.keep_only(uid);
                Ok(uid)
            }
            (error, _) => {
                // Whatever got added before the failure goes too: the earlier drawing is the page's.
                let kept: Vec<u64> = before;
                self.session.docs.retain(|d| kept.contains(&d.uid));
                self.session.active = self.session.active.min(self.session.docs.len().saturating_sub(1));
                hostlock::install(earlier_lock);
                self.host_view = earlier_view;
                Err(error.unwrap_or_else(|| "open: no drawing was opened".to_string()))
            }
        }
    }

    /// Is `uid` the drawing on screen? False once the user closed it or another one is active.
    pub fn host_has(&self, uid: u64) -> bool {
        self.session.state().is_ok_and(|d| d.uid == uid)
    }

    /// Keep the page's drawing the only one: a command that adds a drawing anyway is undone here.
    pub fn host_only(&mut self, uid: u64) -> bool {
        self.session.keep_only(uid)
    }

    /// Does the page's drawing hold unsaved changes? A read-only drawing never does.
    pub fn host_dirty(&self, uid: u64) -> bool {
        !hostlock::is_read_only() && self.session.docs.iter().any(|d| d.uid == uid && d.is_dirty())
    }

    /// The page's drawing as DXF bytes and its title. With `save` the drawing counts as saved
    /// (the page stores what it is handed) and a read-only drawing refuses. The page's call is not
    /// a menu action: the administrator's switches (a disabled save menu) do not apply.
    pub fn host_bytes(&mut self, uid: u64, save: bool) -> Result<(String, Vec<u8>), String> {
        if !self.host_has(uid) {
            return Err("not-ready".into());
        }
        if save && hostlock::is_read_only() {
            return Err("read-only".into());
        }
        let (bytes, _) = hostlock::as_host(|| cadcraft_engine::cmd::file::document_bytes(&mut self.session, &serde_json::json!({ "format": "dxf" })))
            .map_err(|e| e.to_string())?;
        let state = self.session.state_mut().map_err(|e| e.to_string())?;
        let title = state.title.clone();
        if save {
            state.saved = state.doc.clone();
        }
        Ok((title, bytes))
    }
}

/// The grade name of command `id` in `commands.json`.
fn grade(id: &str) -> &'static str {
    if hostlock::is_locked(id) { "locked" } else { "on" }
}

/// The command list (`ppm/commands.json`, copied to `dist/web` by `ppm/build-web.sh`):
/// `{ app, schema, commands }` with `{ id, label, labelKo, location, grades }` for every command,
/// by id. `labelKo` is null where the app has no Korean text for the label.
pub fn commands_json() -> String {
    let korean = |label: &str| {
        let ko = crate::i18n::tr("ko", label);
        (ko != label).then(|| ko.to_string())
    };
    let mut rows: std::collections::BTreeMap<String, serde_json::Value> = std::collections::BTreeMap::new();
    let mut add = |id: &str, label: &str, location: &[&str]| {
        rows.entry(id.to_string()).or_insert_with(|| {
            serde_json::json!({
                "id": id,
                "label": label,
                "labelKo": korean(label),
                "location": location,
                "grades": { "builder": grade(id) },
            })
        });
    };
    for c in cadcraft_engine::cmd::command_specs() {
        add(c.id, c.label, c.menu);
    }
    for (id, label, menu, _) in crate::menus::UI_COMMANDS {
        add(id, label, menu);
    }
    // File ▸ Open Recent's items are one locked command per entry; the list names the group.
    add("ui.recent.clear", crate::menus::CLEAR_RECENT, &["File", crate::menus::OPEN_RECENT]);
    let list: Vec<serde_json::Value> = rows.into_values().collect();
    serde_json::to_string_pretty(&serde_json::json!({ "app": "cad", "schema": 1, "commands": list })).unwrap_or_default() + "\n"
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    /// `ppm/commands.json` is the table: run with `UPDATE_COMMANDS_JSON=1` to write it again.
    #[test]
    fn commands_json_matches_the_tables() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ppm/commands.json");
        let want = commands_json();
        if std::env::var_os("UPDATE_COMMANDS_JSON").is_some() {
            std::fs::write(&path, &want).unwrap();
        }
        let have = std::fs::read_to_string(&path).unwrap_or_default();
        assert_eq!(have, want, "ppm/commands.json is out of date: run the tests with UPDATE_COMMANDS_JSON=1");
    }

    /// Whatever the engine locks is `locked` in the list, and nothing else is.
    #[test]
    fn the_list_marks_exactly_the_locked_commands() {
        let list: serde_json::Value = serde_json::from_str(&commands_json()).unwrap();
        let rows = list["commands"].as_array().unwrap();
        for id in hostlock::LOCKED {
            let row = rows.iter().find(|r| r["id"] == *id).unwrap_or_else(|| panic!("{id} is not listed"));
            assert_eq!(row["grades"]["builder"], "locked", "{id}");
        }
        for row in rows {
            let id = row["id"].as_str().unwrap();
            assert_eq!(row["grades"]["builder"] == "locked", hostlock::is_locked(id), "{id}");
        }
        assert!(rows.iter().any(|r| r["id"] == "ui.open" && r["grades"]["builder"] == "locked"));
        assert!(rows.iter().any(|r| r["id"] == "line" && r["grades"]["builder"] == "on"));
        assert_eq!(list["app"], "cad");
    }

    /// Ids are unique and sorted, so the file diffs cleanly.
    #[test]
    fn ids_are_unique_and_sorted() {
        let list: serde_json::Value = serde_json::from_str(&commands_json()).unwrap();
        let ids: Vec<&str> = list["commands"].as_array().unwrap().iter().filter_map(|r| r["id"].as_str()).collect();
        assert!(ids.windows(2).all(|w| w[0] < w[1]));
    }
}
