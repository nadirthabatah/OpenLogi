//! What is attached, and re-reading the config that drives it.

use openlogi_core::hid::DeviceRoute;
use serde_json::{Value, json};
use tarpc::context;

use super::{agent, no_arguments_schema, rendered, rpc};

/// Tool descriptors owned by this module.
pub fn tools() -> Vec<Value> {
    vec![
        json!({
            "name": "list_devices",
            "description": "List every peripheral the OpenLogi agent currently sees: \
                receivers with their paired devices, directly attached and standalone \
                devices, agent health, and whether a camera is in use. Each device \
                entry includes the `route` object every other tool takes as its \
                `route` argument. Start here.",
            "inputSchema": no_arguments_schema(),
        }),
        json!({
            "name": "reload_config",
            "description": "Ask the agent to re-read its config.toml and rebuild its \
                live bindings, after the file was edited on disk.",
            "inputSchema": no_arguments_schema(),
        }),
    ]
}

/// One snapshot, rendered as JSON with a ready-made route attached to every
/// paired device.
pub async fn list_devices() -> Result<String, String> {
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
    rendered(&json!({
        "status": snapshot.status,
        "receivers": receivers,
        "standalone": snapshot.standalone,
        "camera_active": snapshot.camera_active,
    }))
}

/// Have the agent re-read `config.toml`.
pub async fn reload_config() -> Result<String, String> {
    let client = agent().await?;
    rpc(client.reload_config(context::current()))
        .await?
        .map_err(|error| format!("the agent rejected the config on disk: {}", error.message))?;
    Ok("the agent reloaded its configuration".to_string())
}
