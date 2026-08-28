//! Serve the agent to AI assistants over the Model Context Protocol.
//!
//! `openlogi mcp` turns this process into a local MCP server speaking
//! newline-delimited JSON-RPC 2.0 on stdin/stdout — the standard stdio
//! transport every MCP client (Claude Code, Claude Desktop, and others)
//! launches and drives itself. Registering it is one configuration entry
//! naming the command `openlogi mcp`; no network port is opened and nothing
//! leaves the machine, so the local-only promise holds by construction.
//!
//! The server is a pure IPC client of the running agent, exactly like the
//! other CLI subcommands: every tool call connects, performs the protocol
//! handshake, declares itself [`ClientKind::Cli`] so a dormant agent serves
//! it without arming, issues the RPC, and reports the outcome as tool-result
//! text the model can read and act on. No agent reachable is a tool-level
//! error, not a startup failure — the server keeps serving so the assistant
//! can retry once the agent is up.
//!
//! [`ClientKind::Cli`]: openlogi_ipc::ClientKind::Cli

mod protocol;
mod tools;

use anyhow::Result;
use clap::Args;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// Arguments for `openlogi mcp`. None yet: the server's tool surface is
/// read/write device control only, and needs no configuration.
#[derive(Debug, Args)]
pub struct McpArgs {}

/// Serve MCP over stdio until the client closes stdin.
///
/// Tracing is already routed to stderr by `run()` in `lib.rs`, which is
/// load-bearing here: stdout carries only protocol frames, one JSON-RPC
/// message per line.
///
/// # Errors
///
/// Fails only on an stdio transport error (a broken pipe while replying,
/// or unreadable stdin). Protocol-level problems are answered in-band as
/// JSON-RPC errors and do not end the process.
pub async fn run(_args: McpArgs) -> Result<()> {
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut stdout = tokio::io::stdout();
    while let Some(line) = lines.next_line().await? {
        let Some(reply) = handle_line(&line).await else {
            continue;
        };
        let mut frame = serde_json::to_vec(&reply)?;
        frame.push(b'\n');
        stdout.write_all(&frame).await?;
        stdout.flush().await?;
    }
    Ok(())
}

/// Handle one inbound line; `None` means nothing is sent back (blank line,
/// notification, or a stray response frame).
async fn handle_line(line: &str) -> Option<Value> {
    if line.trim().is_empty() {
        return None;
    }
    let Ok(message) = serde_json::from_str::<Value>(line) else {
        return Some(protocol::error(
            &Value::Null,
            protocol::PARSE_ERROR,
            "the line is not valid JSON",
        ));
    };
    let id = message.get("id").cloned();
    let Some(method) = message.get("method").and_then(Value::as_str) else {
        // No method + an id is a response frame; this server never issues
        // requests of its own, so there is nothing to correlate it with.
        return None;
    };
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    match id {
        // A method without an id is a notification (`notifications/initialized`
        // and friends): acted on if needed, never answered.
        None => None,
        Some(id) => Some(handle_request(&id, method, &params).await),
    }
}

/// Dispatch one request and always produce a response frame for it.
async fn handle_request(id: &Value, method: &str, params: &Value) -> Value {
    match method {
        "initialize" => protocol::result(id, &initialize_result(params)),
        "ping" => protocol::result(id, &Value::Object(serde_json::Map::new())),
        "tools/list" => protocol::result(id, &serde_json::json!({ "tools": tools::catalog() })),
        "tools/call" => match tools::call(params).await {
            Ok(outcome) => protocol::result(id, &outcome),
            Err(unknown) => protocol::error(id, protocol::INVALID_PARAMS, &unknown),
        },
        // `server/discover` lands here too: it is the modern-era (2026-07-28)
        // probe, and method-not-found is exactly what tells a dual-era client
        // to fall back to the `initialize` flow this server speaks.
        _ => protocol::error(
            id,
            protocol::METHOD_NOT_FOUND,
            &format!("method {method} is not supported"),
        ),
    }
}

/// Build the `initialize` result for the revision the client asked for.
fn initialize_result(params: &Value) -> Value {
    let requested = params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or_default();
    serde_json::json!({
        "protocolVersion": protocol::negotiate(requested),
        "capabilities": { "tools": {} },
        "serverInfo": {
            "name": "openlogi",
            "title": "OpenLogi",
            "version": env!("CARGO_PKG_VERSION"),
        },
        "instructions": "Local control of the peripherals managed by the \
            OpenLogi agent on this machine. Start with list_devices: its \
            output includes, for each device, the exact `route` object the \
            other tools take as their `route` argument. All control is \
            local IPC to the agent; nothing touches the network.",
    })
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{handle_line, protocol};

    /// Drive one line through the server and decode the reply it must produce.
    async fn reply_to(line: &str) -> Value {
        handle_line(line).await.expect("a reply is produced")
    }

    #[tokio::test]
    async fn initialize_echoes_a_supported_revision() {
        let reply = reply_to(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#,
        )
        .await;
        assert_eq!(reply["id"], json!(1));
        assert_eq!(reply["result"]["protocolVersion"], json!("2025-06-18"));
        assert!(reply["result"]["capabilities"]["tools"].is_object());
    }

    #[tokio::test]
    async fn initialize_offers_the_latest_revision_to_an_unknown_one() {
        let reply = reply_to(
            r#"{"jsonrpc":"2.0","id":2,"method":"initialize","params":{"protocolVersion":"1900-01-01"}}"#,
        )
        .await;
        assert_eq!(
            reply["result"]["protocolVersion"],
            json!(protocol::SUPPORTED_VERSIONS[0])
        );
    }

    #[tokio::test]
    async fn notifications_are_never_answered() {
        let reply = handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).await;
        assert!(reply.is_none(), "notifications must not produce a frame");
    }

    #[tokio::test]
    async fn unknown_methods_get_method_not_found() {
        let reply = reply_to(r#"{"jsonrpc":"2.0","id":3,"method":"server/discover"}"#).await;
        assert_eq!(reply["error"]["code"], json!(protocol::METHOD_NOT_FOUND));
    }

    #[tokio::test]
    async fn malformed_json_gets_a_parse_error_with_null_id() {
        let reply = reply_to("this is not json").await;
        assert_eq!(reply["error"]["code"], json!(protocol::PARSE_ERROR));
        assert!(reply["id"].is_null());
    }

    #[tokio::test]
    async fn tools_list_names_every_tool_exactly_once() {
        let reply = reply_to(r#"{"jsonrpc":"2.0","id":4,"method":"tools/list"}"#).await;
        let tools = reply["result"]["tools"]
            .as_array()
            .expect("tools is an array");
        let mut names: Vec<&str> = tools
            .iter()
            .map(|tool| tool["name"].as_str().expect("every tool is named"))
            .collect();
        let total = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), total, "tool names must be unique");
        assert!(names.contains(&"list_devices"));
        for tool in tools {
            assert!(
                tool["inputSchema"]["type"] == json!("object"),
                "every input schema is an object schema"
            );
            assert!(
                tool["description"].as_str().is_some_and(|d| !d.is_empty()),
                "every tool carries a description"
            );
        }
    }

    #[tokio::test]
    async fn calling_an_unknown_tool_is_invalid_params() {
        let reply = reply_to(
            r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"no_such_tool"}}"#,
        )
        .await;
        assert_eq!(reply["error"]["code"], json!(protocol::INVALID_PARAMS));
    }
}
