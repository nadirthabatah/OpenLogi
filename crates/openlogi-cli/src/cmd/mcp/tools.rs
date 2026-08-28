//! The MCP tool surface, executed against the running agent over IPC.
//!
//! Two error channels, deliberately distinct: an unknown tool name is a
//! protocol-level failure (the caller maps it to a JSON-RPC error), while
//! everything that can go wrong *doing* the work — no agent running, a
//! malformed `route` argument, a device write failure — is reported as
//! tool-result text with `isError` set, because that text is what the
//! language model reads to correct itself and retry.

use std::time::Duration;

use openlogi_core::hid::{DeviceRoute, Dpi};
use openlogi_ipc::{AgentClient, ClientKind, PROTOCOL_VERSION, client};
use serde_json::{Value, json};
use tarpc::context;

/// How long to wait for the agent's socket and handshake.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
/// How long to wait for any single RPC to answer.
const RPC_TIMEOUT: Duration = Duration::from_secs(5);

/// Wording every tool that takes a `route` shares, teaching the model where
/// route objects come from instead of letting it invent one.
const ROUTE_HELP: &str = "Pass the `route` object exactly as returned by list_devices; \
     do not construct one by hand.";

/// The tool catalog served by `tools/list`.
pub fn catalog() -> Value {
    let route_schema = json!({
        "type": "object",
        "description": ROUTE_HELP,
    });
    json!([
        {
            "name": "list_devices",
            "description": "List every peripheral the OpenLogi agent currently sees: \
                receivers with their paired devices, directly attached and standalone \
                devices, agent health, and whether a camera is in use. Each device \
                entry includes the `route` object the other tools take as their \
                `route` argument.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false },
        },
        {
            "name": "read_dpi",
            "description": "Read a mouse's current pointer resolution (DPI) and the \
                values the sensor supports.",
            "inputSchema": {
                "type": "object",
                "properties": { "route": route_schema },
                "required": ["route"],
                "additionalProperties": false,
            },
        },
        {
            "name": "set_dpi",
            "description": "Set a mouse's pointer resolution (DPI). Read read_dpi first \
                when unsure which values the sensor supports.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "route": route_schema,
                    "dpi": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 65535,
                        "description": "The DPI value to apply, e.g. 800 or 1600.",
                    },
                },
                "required": ["route", "dpi"],
                "additionalProperties": false,
            },
        },
        {
            "name": "reload_config",
            "description": "Ask the agent to re-read its config.toml and rebuild its \
                live bindings, after the file was edited on disk.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false },
        },
    ])
}

/// Execute one `tools/call`.
///
/// # Errors
///
/// Only an unrecognized tool name (or a missing one) is an `Err`; the caller
/// turns it into a JSON-RPC invalid-params error. Execution failures come
/// back as `Ok` tool results with `isError` set.
pub async fn call(params: &Value) -> Result<Value, String> {
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return Err("tools/call params carry no tool name".to_string());
    };
    let arguments = params.get("arguments").cloned().unwrap_or(Value::Null);
    let outcome = match name {
        "list_devices" => list_devices().await,
        "read_dpi" => read_dpi(&arguments).await,
        "set_dpi" => set_dpi(&arguments).await,
        "reload_config" => reload_config().await,
        other => return Err(format!("unknown tool: {other}")),
    };
    Ok(match outcome {
        Ok(text) => json!({ "content": [{ "type": "text", "text": text }], "isError": false }),
        Err(text) => json!({ "content": [{ "type": "text", "text": text }], "isError": true }),
    })
}

/// Connect to the agent, complete the handshake, and declare this process a
/// CLI client so a dormant agent serves it without arming its input stack.
async fn agent() -> Result<AgentClient, String> {
    let conn = tokio::time::timeout(CONNECT_TIMEOUT, client::connect())
        .await
        .map_err(|_| no_agent("the agent's socket did not answer in time"))?
        .map_err(|error| no_agent(&error.to_string()))?;
    if conn.version != PROTOCOL_VERSION {
        return Err(format!(
            "the running agent speaks IPC protocol v{}, this CLI expects v{PROTOCOL_VERSION}; \
             they normally ship together, so one side is a stale install",
            conn.version
        ));
    }
    rpc(conn
        .client
        .declare_client(context::current(), ClientKind::Cli))
    .await?;
    Ok(conn.client)
}

/// Phrase an unreachable-agent failure so the model can tell the user what
/// to actually do about it.
fn no_agent(detail: &str) -> String {
    format!(
        "no running OpenLogi agent is reachable ({detail}); start the OpenLogi app \
         or the openlogi-agent process on this machine, then retry"
    )
}

/// Await one RPC under [`RPC_TIMEOUT`], flattening timeout and transport
/// failures into tool-result text.
async fn rpc<T>(
    call: impl Future<Output = Result<T, tarpc::client::RpcError>>,
) -> Result<T, String> {
    tokio::time::timeout(RPC_TIMEOUT, call)
        .await
        .map_err(|_| "the agent did not answer the request in time".to_string())?
        .map_err(|error| format!("the agent connection failed: {error}"))
}

/// `list_devices`: one snapshot, rendered as JSON with a ready-made route
/// attached to every paired device.
async fn list_devices() -> Result<String, String> {
    let client = agent().await?;
    let snapshot = rpc(client.snapshot(context::current())).await?;
    let receivers: Vec<Value> = snapshot
        .inventory
        .iter()
        .map(|inventory| {
            let devices: Vec<Value> = inventory
                .paired
                .iter()
                .map(|device| {
                    json!({
                        "device": device,
                        "route": DeviceRoute::device_route_for(inventory, device.slot),
                    })
                })
                .collect();
            json!({ "receiver": inventory.receiver, "devices": devices })
        })
        .collect();
    let listing = json!({
        "status": snapshot.status,
        "receivers": receivers,
        "standalone": snapshot.standalone,
        "camera_active": snapshot.camera_active,
    });
    serde_json::to_string_pretty(&listing)
        .map_err(|error| format!("failed to render the device list: {error}"))
}

/// Decode the `route` argument every per-device tool takes.
fn route_argument(arguments: &Value) -> Result<DeviceRoute, String> {
    let Some(route) = arguments.get("route") else {
        return Err(format!("the `route` argument is missing. {ROUTE_HELP}"));
    };
    serde_json::from_value(route.clone()).map_err(|error| {
        format!("the `route` argument is not a device route ({error}). {ROUTE_HELP}")
    })
}

/// `read_dpi`: current and supported DPI for the routed device.
async fn read_dpi(arguments: &Value) -> Result<String, String> {
    let route = route_argument(arguments)?;
    let client = agent().await?;
    let info = rpc(client.read_dpi(context::current(), route))
        .await?
        .map_err(|error| format!("reading DPI failed: {error}"))?;
    serde_json::to_string_pretty(&info)
        .map_err(|error| format!("failed to render the DPI info: {error}"))
}

/// `set_dpi`: apply a DPI value to the routed device.
async fn set_dpi(arguments: &Value) -> Result<String, String> {
    let route = route_argument(arguments)?;
    let requested = arguments
        .get("dpi")
        .and_then(Value::as_u64)
        .ok_or_else(|| "the `dpi` argument must be a positive integer".to_string())?;
    let dpi = u32::try_from(requested)
        .ok()
        .and_then(|value| Dpi::try_from(value).ok())
        .ok_or_else(|| format!("{requested} is not a representable DPI value"))?;
    let client = agent().await?;
    rpc(client.set_dpi(context::current(), route.clone(), dpi))
        .await?
        .map_err(|error| format!("setting DPI failed: {error}"))?;
    Ok(format!("DPI set to {dpi} for {route}"))
}

/// `reload_config`: have the agent re-read `config.toml`.
async fn reload_config() -> Result<String, String> {
    let client = agent().await?;
    rpc(client.reload_config(context::current()))
        .await?
        .map_err(|error| format!("the agent rejected the config on disk: {}", error.message))?;
    Ok("the agent reloaded its configuration".to_string())
}
