//! C# `CustomInlineTag`: the plugin-inline contract in a .docx — a `w:sdt` whose `w:tag` is
//! `wordnext:inline:<type>:<JSON as encodeURIComponent>`. The .NET semantics, not the JS ones
//! of the reader's `parse_custom_inline_tag`: `Uri.UnescapeDataString` keeps malformed escapes
//! (where `decodeURIComponent` throws) and `JsonDocument.Parse` limits depth to 64.

use super::error::{ApiError, ErrorKind, Result};
use super::net;

pub const PREFIX: &str = "wordnext:inline:";

/// `^[a-zA-Z0-9._-]+\z`.
fn type_ok(t: &str) -> bool {
    !t.is_empty() && t.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// The type and the decoded JSON of a plugin inline's tag, or `None` when the tag is not one
/// (or the JSON is not valid).
pub fn try_parse(tag: Option<&str>) -> Option<(String, String)> {
    let payload = tag?.strip_prefix(PREFIX)?;
    let split = payload.find(':')?;
    if split == 0 {
        return None;
    }
    let ty = payload.get(..split)?;
    if !type_ok(ty) {
        return None;
    }
    let decoded = net::unescape_data_string(payload.get(split + 1..)?);
    net::json_valid(&decoded).then(|| (ty.to_owned(), decoded))
}

/// The tag of an inline of type `ty` with the JSON payload (`ArgumentException` for a bad type,
/// `JsonReaderException` for invalid JSON).
pub fn format(ty: &str, json: &str) -> Result<String> {
    if !type_ok(ty) {
        return Err(ApiError::argument(format!("invalid inline type: {ty}")));
    }
    if !net::json_valid(json) {
        return Err(ApiError::new(ErrorKind::Json, "invalid JSON payload"));
    }
    Ok(format!("{PREFIX}{ty}:{}", net::escape_data_string(json)))
}
