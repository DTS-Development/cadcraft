//! The app's side of the PPM host lock (the engine's is `cadcraft_engine::hostlock`): the list the
//! administrator's screen reads, `ppm/commands.json`. Every engine command and every interface
//! command has a grade: `locked` ones never run in the PPM build; the rest are `on`, and a
//! parent's `policy.overrides` can switch them off.

use cadcraft_engine::hostlock;

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
