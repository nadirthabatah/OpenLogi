//! JSON-RPC 2.0 framing and legacy MCP version negotiation.
//!
//! "Legacy" is the Model Context Protocol's own term for the revisions up to
//! `2025-11-25`, which open a stdio session with an `initialize` request —
//! the flow every MCP client shipping today uses. The `2026-07-28` "modern"
//! revision replaces the handshake with per-request metadata; this server
//! does not speak it yet, and answers its `server/discover` probe with
//! method-not-found so a dual-era client falls back to `initialize`.

use serde_json::{Value, json};

/// Legacy protocol revisions this server accepts, newest first.
///
/// [`negotiate`] echoes the client's revision when it is listed here; the
/// entries themselves only mark revisions whose initialize/tools surface
/// this server implements compatibly (the tools surface has been wire-stable
/// across all four).
pub const SUPPORTED_VERSIONS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

/// JSON-RPC: the frame was not valid JSON.
pub const PARSE_ERROR: i64 = -32700;
/// JSON-RPC: the frame is not a well-formed request.
pub const INVALID_REQUEST: i64 = -32600;
/// JSON-RPC: the method is not implemented.
pub const METHOD_NOT_FOUND: i64 = -32601;
/// JSON-RPC: the params are invalid for the method (an unknown tool name
/// lands here too, per the MCP tools specification).
pub const INVALID_PARAMS: i64 = -32602;

/// Pick the revision to answer an `initialize` with: the client's own when
/// this server supports it, otherwise the newest supported one — the legacy
/// negotiation contract, under which the client then decides whether it can
/// proceed or disconnects.
pub fn negotiate(requested: &str) -> &'static str {
    SUPPORTED_VERSIONS
        .into_iter()
        .find(|supported| *supported == requested)
        .unwrap_or(SUPPORTED_VERSIONS[0])
}

/// Wrap a successful outcome in a JSON-RPC response frame.
pub fn result(id: &Value, outcome: &Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": outcome })
}

/// Wrap a protocol failure in a JSON-RPC error frame. `id` is `Null` when
/// the request was too malformed to carry one (a parse error).
pub fn error(id: &Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

#[cfg(test)]
mod tests {
    use super::{SUPPORTED_VERSIONS, negotiate};

    #[test]
    fn every_supported_revision_is_echoed() {
        for revision in SUPPORTED_VERSIONS {
            assert_eq!(negotiate(revision), revision);
        }
    }

    #[test]
    fn unknown_revisions_get_the_newest_supported_one() {
        assert_eq!(negotiate("1900-01-01"), SUPPORTED_VERSIONS[0]);
        assert_eq!(negotiate(""), SUPPORTED_VERSIONS[0]);
        // The modern era is not a legacy revision — a modern-only client that
        // somehow sends `initialize` must be offered legacy, not an echo.
        assert_eq!(negotiate("2026-07-28"), SUPPORTED_VERSIONS[0]);
    }
}
