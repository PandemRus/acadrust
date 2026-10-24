//! DXF (Drawing Exchange Format) reading and writing.
//!
//! Supports both **ASCII** and **Binary** DXF for versions R12 (AC1009)
//! through R2018+ (AC1032).
//!
//! # Reading
//!
//! ```rust,ignore
//! use acadrust::DxfReader;
//!
//! let doc = DxfReader::from_file("drawing.dxf")?.read()?;
//! ```
//!
//! # Writing
//!
//! ```rust,ignore
//! use acadrust::DxfWriter;
//!
//! DxfWriter::new(&doc).write_to_file("output.dxf")?;
//! ```

pub mod code_page;
mod dxf_code;
mod group_code_value;
mod reader;
mod spline_flags;
mod writer;

pub use dxf_code::DxfCode;
pub use group_code_value::GroupCodeValueType;
pub use reader::{DxfReader, DxfReaderConfiguration};
pub(crate) use reader::{DxfStreamReader, DxfTextReader};
pub use writer::{value_type_for_code, write_binary_dxf, write_dxf};
pub use writer::{
    DxfBinaryWriter, DxfStreamWriter, DxfStreamWriterExt, DxfTextWriter, DxfWriter, SectionWriter,
};

/// Ellipse parameter, in radians, of an elliptic hatch edge end stored in
/// DXF group 50 or 51.
///
/// DXF stores these ends as circular angles in degrees, measured from the
/// major axis. The model, like DWG, holds ellipse parameters. The two differ
/// when the axis ratio is not 1 (ezdxf `EllipseEdge.start_param`, GDAL
/// `ogrdxf_hatch.cpp`). The result stays in the same turn as the angle, so a
/// full-ellipse end of 360 degrees reads as `TAU`.
pub(crate) fn elliptic_edge_param(axis_ratio: f64, degrees: f64) -> f64 {
    let angle = degrees.to_radians();
    if !(axis_ratio > 0.0 && axis_ratio.is_finite() && angle.is_finite()) {
        return angle;
    }
    same_turn(angle.sin().atan2(angle.cos() * axis_ratio), angle)
}

/// DXF group 50 or 51 value, in degrees, of an elliptic hatch edge end with
/// ellipse parameter `param`. The inverse of [`elliptic_edge_param`].
pub(crate) fn elliptic_edge_degrees(axis_ratio: f64, param: f64) -> f64 {
    if !(axis_ratio > 0.0 && axis_ratio.is_finite() && param.is_finite()) {
        return writer::degrees(param);
    }
    let angle = same_turn((param.sin() * axis_ratio).atan2(param.cos()), param);
    writer::degrees(angle)
}

/// `value` moved by whole turns to the turn of `reference`. A circular angle
/// and its ellipse parameter always lie in the same quadrant, so the nearest
/// turn is the right one.
fn same_turn(value: f64, reference: f64) -> f64 {
    let turn = std::f64::consts::TAU;
    value + ((reference - value) / turn).round() * turn
}

pub(crate) fn split_color_book_name(value: &str) -> (Option<String>, Option<String>) {
    let optional = |value: &str| (!value.is_empty()).then(|| value.to_string());
    match value.split_once('$') {
        Some((book_name, color_name)) => (optional(book_name), optional(color_name)),
        None => (None, optional(value)),
    }
}

pub(crate) fn join_color_book_name(
    book_name: Option<&str>,
    color_name: Option<&str>,
) -> Option<String> {
    let book_name = book_name.filter(|value| !value.is_empty());
    let color_name = color_name.filter(|value| !value.is_empty());
    match (book_name, color_name) {
        (Some(book_name), Some(color_name)) => Some(format!("{book_name}${color_name}")),
        (Some(book_name), None) => Some(format!("{book_name}$")),
        (None, Some(color_name)) => Some(color_name.to_string()),
        (None, None) => None,
    }
}

pub(crate) fn object_has_own_dxf_xdata(object: &crate::objects::ObjectType) -> bool {
    use crate::objects::ObjectType;
    match object {
        ObjectType::XRecord(_)
        | ObjectType::TableStyle(_)
        | ObjectType::UnderlayDefinition(_)
        | ObjectType::Field(_) => true,
        ObjectType::SortEntitiesTable(table) => table.raw_dxf_codes.is_some(),
        // Unknown and Arch objects are written only from their raw groups.
        ObjectType::Unknown { .. } | ObjectType::Arch(_) => true,
        _ => false,
    }
}
