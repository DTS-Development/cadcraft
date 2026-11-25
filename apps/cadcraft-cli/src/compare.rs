//! `compare`: what changed between two versions of a drawing, as JSON.

use super::args::CompareArgs;
use super::error::Result;
use super::output::Streams;
use super::unicode;
use cadcraft_io::roundtrip::{Category, Options, compare as compare_drawings};
use serde_json::json;

/// Read one side: the drawing and what reading it left out (each note says which side it is).
fn read_side(path: &std::path::Path, side: &str) -> Result<(cadcraft_engine::doc::Drawing, Vec<String>)> {
    let bytes = std::fs::read(path)?;
    let d = cadcraft_io::read(&bytes, unicode(path)?)?;
    let mut notes: Vec<String> = d.read_notes.iter().map(|n| format!("{side}: {n}")).collect();
    for (section, n) in &d.skipped_records {
        notes.push(format!("{side}: {n} record(s) of the {section} section couldn't be placed and were skipped"));
    }
    Ok((d, notes))
}

pub(crate) fn compare(a: &CompareArgs, io: &mut Streams) -> Result {
    let (before, mut notes) = read_side(&a.before, "before")?;
    let (after, after_notes) = read_side(&a.after, "after")?;
    notes.extend(after_notes);
    let opts = Options { abs_tol: a.tolerance, max_differences: a.max_differences, ..Options::default() };
    let mut c = compare_drawings(&before, &after, &opts);
    // A save renumbers the handles of objects nobody touched (every re-save of a DWG-origin file
    // does), so handle differences say nothing about what a person changed: left out.
    c.differences.retain(|d| d.category != Category::Handle);
    c.totals.remove(&Category::Handle);
    let mut v = serde_json::to_value(&c)?;
    if let Some(o) = v.as_object_mut() {
        o.insert("notes".into(), json!(notes));
    }
    io.json(&v, a.compact);
    Ok(())
}
