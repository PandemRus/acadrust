//! Field trace of DWG modeler-geometry records (3DSOLID, REGION, BODY and
//! the SURFACE family), behind the `trace-acis-records` feature.
//!
//! Without the feature the macros below expand to the plain read, so the
//! reader is unchanged. With it, every traced field of a selected record is
//! written to stderr as one line: the field name, its main-stream bit
//! position (absolute in the record, and relative to the start of the ACIS
//! data), its width in bits and its value. Geometry (points, vectors,
//! matrices, doubles) is traced by position and width only.
//!
//! Records are selected with [`set_handle_filter`] or, when that is not
//! called, with the `ACADRUST_TRACE_ACIS_HANDLES` environment variable
//! (comma-separated hex handles). With neither, every modeler record is
//! traced. The lines of one record are written together when it ends.

/// Reads `$read` and traces it as `$name` with its value.
macro_rules! traced {
    ($reader:expr, $name:expr, $read:expr) => {{
        #[cfg(feature = "trace-acis-records")]
        let start = $reader.position_in_bits();
        let value = $read;
        #[cfg(feature = "trace-acis-records")]
        $crate::io::dwg::acis_trace::field(&$reader, $name, start, &value);
        value
    }};
}

/// Reads `$read` and traces it as `$name` by position only (geometry).
macro_rules! traced_pos {
    ($reader:expr, $name:expr, $read:expr) => {{
        #[cfg(feature = "trace-acis-records")]
        let start = $reader.position_in_bits();
        let value = $read;
        #[cfg(feature = "trace-acis-records")]
        $crate::io::dwg::acis_trace::field(&$reader, $name, start, &"<not shown>");
        value
    }};
}

/// Traces a note at the current position, with optional format arguments.
macro_rules! trace_note {
    ($reader:expr, $($arg:tt)+) => {{
        #[cfg(feature = "trace-acis-records")]
        $crate::io::dwg::acis_trace::note(&$reader, &format!($($arg)+));
    }};
}

pub(crate) use {trace_note, traced, traced_pos};

#[cfg(feature = "trace-acis-records")]
pub use enabled::*;

#[cfg(feature = "trace-acis-records")]
mod enabled {
    use std::cell::RefCell;
    use std::collections::HashSet;
    use std::fmt::{Debug, Write as _};
    use std::sync::RwLock;

    use crate::io::dwg::dwg_stream_readers::merged_reader::DwgMergedReader;

    /// `None`: not set yet (read the environment). `Some(None)`: trace all.
    static FILTER: RwLock<Option<Option<HashSet<u64>>>> = RwLock::new(None);

    struct Record {
        handle: u64,
        /// Start of the ACIS data of each nested modeler body.
        acis_start: Vec<i64>,
        pass: &'static str,
        error_reported: bool,
        out: String,
    }

    thread_local! {
        static RECORD: RefCell<Option<Record>> = const { RefCell::new(None) };
    }

    /// Trace only the records with these handles (`None`: all records).
    pub fn set_handle_filter(handles: Option<Vec<u64>>) {
        let mut filter = FILTER.write().unwrap_or_else(|e| e.into_inner());
        *filter = Some(handles.map(|h| h.into_iter().collect()));
    }

    fn selected(handle: u64) -> bool {
        {
            let filter = FILTER.read().unwrap_or_else(|e| e.into_inner());
            if let Some(filter) = filter.as_ref() {
                return filter.as_ref().is_none_or(|set| set.contains(&handle));
            }
        }
        let from_env = std::env::var("ACADRUST_TRACE_ACIS_HANDLES")
            .ok()
            .map(|list| {
                list.split(',')
                    .filter_map(|item| {
                        let item = item.trim();
                        let item = item
                            .strip_prefix("0x")
                            .or_else(|| item.strip_prefix("0X"))
                            .unwrap_or(item);
                        u64::from_str_radix(item, 16).ok()
                    })
                    .collect::<HashSet<u64>>()
            });
        let mut filter = FILTER.write().unwrap_or_else(|e| e.into_inner());
        let filter = filter.get_or_insert(from_env);
        filter.as_ref().is_none_or(|set| set.contains(&handle))
    }

    fn with_record(f: impl FnOnce(&mut Record)) {
        RECORD.with(|cell| {
            if let Some(record) = cell.borrow_mut().as_mut() {
                f(record);
            }
        });
    }

    fn check_error(reader: &DwgMergedReader, record: &mut Record) {
        if record.error_reported {
            return;
        }
        if let Err(error) = reader.check_error() {
            record.error_reported = true;
            let _ = writeln!(
                record.out,
                "[acis-trace] {:#X}   !! reader error set here: {error}",
                record.handle
            );
        }
    }

    /// Starts the trace of a record whose common entity data has been read.
    pub(crate) fn begin(reader: &DwgMergedReader, handle: u64, type_name: &str) {
        RECORD.with(|cell| {
            let mut cell = cell.borrow_mut();
            *cell = None;
            if !selected(handle) {
                return;
            }
            let position = reader.position_in_bits();
            let mut out = String::new();
            let _ = writeln!(
                out,
                "[acis-trace] {handle:#X} {type_name}: entity data from bit {position}, \
                 main data ends at bit {}",
                position + reader.main_remaining_bits()
            );
            *cell = Some(Record {
                handle,
                acis_start: Vec::new(),
                pass: "",
                error_reported: false,
                out,
            });
        });
    }

    /// Ends the trace of a record and writes it to stderr.
    pub(crate) fn end(reader: &DwgMergedReader) {
        let record = RECORD.with(|cell| cell.borrow_mut().take());
        if let Some(mut record) = record {
            check_error(reader, &mut record);
            let status = match reader.check_error() {
                Ok(()) => format!("{} main bits left, ok", reader.main_remaining_bits()),
                Err(error) => format!("failed: {error}"),
            };
            let _ = writeln!(
                record.out,
                "[acis-trace] {:#X} end at bit {}, {status}",
                record.handle,
                reader.position_in_bits(),
            );
            eprint!("{}", record.out);
        }
    }

    /// Leaves a modeler body when dropped (see [`push_acis_start`]).
    pub(crate) struct AcisScope;

    impl Drop for AcisScope {
        fn drop(&mut self) {
            with_record(|record| {
                record.acis_start.pop();
            });
        }
    }

    /// Marks the start of the ACIS data of a (possibly nested) modeler body
    /// until the returned scope is dropped.
    #[must_use]
    pub(crate) fn push_acis_start(reader: &DwgMergedReader) -> AcisScope {
        let position = reader.position_in_bits();
        with_record(|record| {
            record.acis_start.push(position);
            let _ = writeln!(
                record.out,
                "[acis-trace] {:#X}   ACIS data (level {}) starts at bit {position}",
                record.handle,
                record.acis_start.len()
            );
        });
        AcisScope
    }

    /// Labels the following lines (e.g. the wire colour width of a pass).
    pub(crate) fn set_pass(pass: &'static str) {
        with_record(|record| record.pass = pass);
    }

    /// Re-arms the error marker after a pass is discarded from a clone.
    pub(crate) fn reset_error_marker() {
        with_record(|record| record.error_reported = false);
    }

    fn pass_label(record: &Record) -> String {
        if record.pass.is_empty() {
            String::new()
        } else {
            format!("[{}] ", record.pass)
        }
    }

    /// ` (+n)`: the position relative to the innermost ACIS data start.
    fn relative(record: &Record, position: i64) -> String {
        record
            .acis_start
            .last()
            .map(|base| format!(" (+{})", position - base))
            .unwrap_or_default()
    }

    pub(crate) fn field(reader: &DwgMergedReader, name: &str, start: i64, value: &dyn Debug) {
        let end = reader.position_in_bits();
        with_record(|record| {
            let pass = pass_label(record);
            let relative = relative(record, start);
            let _ = writeln!(
                record.out,
                "[acis-trace] {:#X}   {pass}{name} @{start}{relative} {}b = {value:?}",
                record.handle,
                end - start
            );
            check_error(reader, record);
        });
    }

    pub(crate) fn note(reader: &DwgMergedReader, message: &str) {
        let position = reader.position_in_bits();
        with_record(|record| {
            let pass = pass_label(record);
            let relative = relative(record, position);
            let _ = writeln!(
                record.out,
                "[acis-trace] {:#X}   {pass}-- {message} @{position}{relative}",
                record.handle
            );
            check_error(reader, record);
        });
    }
}
