//! `compare` prints what changed between two versions of a drawing: an added entity is listed, a
//! file against itself (or against its own re-save, whose time stamps differ) has no differences.

use std::process::{Command, Output};

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cadcraft-cli")).args(args).output().unwrap()
}

fn dxf(lines: usize) -> Vec<u8> {
    let mut dxf = String::from("0\nSECTION\n2\nENTITIES\n");
    for i in 0..lines {
        dxf += &format!("0\nLINE\n8\n0\n10\n{i}\n20\n0\n11\n{i}\n21\n10\n");
    }
    dxf += "0\nENDSEC\n0\nEOF\n";
    let d = cadcraft_io::read(dxf.as_bytes(), "lines.dxf").unwrap();
    cadcraft_io::write(&d, "lines.dxf").unwrap()
}

fn compare(before: &std::path::Path, after: &std::path::Path) -> serde_json::Value {
    let out = cli(&["compare", before.to_str().unwrap(), after.to_str().unwrap(), "--compact"]);
    assert!(out.status.success(), "{:?}: {}", out.status, String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn compare_lists_an_added_entity_and_ignores_resaves() {
    let dir = std::env::temp_dir().join(format!("cc-cli-compare-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (a, b, c) = (dir.join("a.dxf"), dir.join("b.dxf"), dir.join("c.dxf"));
    std::fs::write(&a, dxf(2)).unwrap();
    std::fs::write(&b, dxf(3)).unwrap();
    // A re-save of `a`: same drawing, new time stamps.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::write(&c, dxf(2)).unwrap();

    let v = compare(&a, &b);
    let diffs = v["differences"].as_array().unwrap();
    assert!(diffs.iter().any(|d| d["change"] == "added" && d["category"] == "count" && d["count"] == 1), "{v}");
    assert_eq!(v["totals"]["count"]["added"], 1, "{v}");
    assert!(v["notes"].as_array().unwrap().is_empty(), "{v}");

    // A save renumbers the handles of untouched objects: not a change.
    assert!(diffs.iter().all(|d| d["category"] != "handle"), "{v}");

    for same in [compare(&a, &a), compare(&a, &c)] {
        assert!(same["differences"].as_array().unwrap().is_empty(), "{same}");
    }

    // A file that is not a drawing fails with exit 1 and a message.
    let bad = dir.join("bad.dxf");
    std::fs::write(&bad, b"not a drawing").unwrap();
    let out = cli(&["compare", a.to_str().unwrap(), bad.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("error:"));
    let _ = std::fs::remove_dir_all(&dir);
}
