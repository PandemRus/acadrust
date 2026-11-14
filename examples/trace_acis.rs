//! Field trace of DWG modeler-geometry records (3DSOLID, REGION, BODY and
//! the SURFACE family), see `acadrust::io::dwg::acis_trace`.
//!
//! Usage:
//!   cargo run --release --features trace-acis-records --example trace_acis -- \
//!       <file.dwg> [handle ...]
//!
//! Handles are hex (`0x381E3` or `381E3`). With no handles the example only
//! lists the modeler records that fail to decode; `all` traces every modeler
//! record. The trace goes to stderr; flags, counts and bit positions are
//! printed, geometry values are not.

use acadrust::io::dwg::acis_trace;
use acadrust::{DwgReadOptions, DwgReader};

/// 3DSOLID, REGION, BODY and the internal codes of the SURFACE family.
const MODELER_TYPE_CODES: [i32; 10] = [37, 38, 39, -5, -6, -7, -8, -9, -10, -11];

fn parse_handle(text: &str) -> Option<u64> {
    let text = text
        .strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))
        .unwrap_or(text);
    u64::from_str_radix(text, 16).ok()
}

/// `(handle, type_code)` of an "Invalid record at handle …" message.
fn failed_record(message: &str) -> Option<(u64, i32)> {
    let rest = message.split("Invalid record at handle ").nth(1)?;
    let (handle, rest) = rest.split_once(", type_code=")?;
    let type_code = rest.split(':').next()?.trim().parse().ok()?;
    Some((parse_handle(handle)?, type_code))
}

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: trace_acis <file.dwg> [handle ... | all]");
        std::process::exit(2);
    };
    let rest: Vec<String> = args.collect();
    if rest.iter().any(|arg| arg == "all") {
        acis_trace::set_handle_filter(None);
    } else {
        let handles = rest
            .iter()
            .map(|arg| parse_handle(arg).unwrap_or_else(|| panic!("not a hex handle: {arg}")))
            .collect();
        acis_trace::set_handle_filter(Some(handles));
    }

    let mut reader = DwgReader::from_file_with_options(&path, DwgReadOptions::failsafe())
        .expect("cannot open the drawing");
    let outcome = reader.read_with_stats().expect("failsafe read failed");

    let stats = &outcome.stats;
    println!(
        "{}: {} records, {} decoded, {} skipped, {} diagnostics",
        stats.source_version,
        stats.source_records,
        stats.decoded_source_records,
        stats.skipped_source_records,
        stats.diagnostics.len()
    );
    for diagnostic in &stats.diagnostics {
        eprintln!("diagnostic: {} {}", diagnostic.code, diagnostic.message);
    }
    let mut failed: Vec<(u64, i32)> = outcome
        .stats
        .diagnostics
        .iter()
        .filter_map(|diagnostic| failed_record(&diagnostic.message))
        .filter(|(_, type_code)| MODELER_TYPE_CODES.contains(type_code))
        .collect();
    failed.sort_unstable();
    failed.dedup();
    println!("{} modeler records failed to decode", failed.len());
    for (handle, type_code) in failed {
        println!("  {handle:#X} type_code={type_code}");
    }
}
