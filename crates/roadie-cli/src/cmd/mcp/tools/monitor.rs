//! Watching physical input, so a control can be identified by pressing it.
//!
//! This exists for the case a device list cannot answer: *which* button is
//! the one under your thumb. Rather than reading indices off a diagram, press
//! it and let the agent's hook say what it saw — the fastest path for anyone,
//! and the only workable one without sight.

use std::time::{Duration, Instant};

use roadie_ipc::MonitorEvent;
use serde_json::{Value, json};
use tarpc::context;

use super::{agent, rpc};

/// Default watch window when the caller does not pick one.
const DEFAULT_SECONDS: u64 = 5;

/// Longest watch window allowed, bounding how long an MCP client sits on a
/// tool call that cannot answer early.
const MAX_SECONDS: u64 = 30;

/// How often to poll the agent while watching.
///
/// This is a correctness constraint, not a tuning knob. The agent's event
/// monitor is kept alive *by being polled*: its janitor
/// (`roadie_agent_core::event_monitor`, `IDLE_TICK`, three seconds) walks an
/// unpolled monitor `Polled -> Idle -> Disabled` and **discards the buffer** on
/// the last step. A single sleep across the whole window therefore loses every
/// event the user produced — the exact press this tool exists to catch — for
/// any window past about six seconds, and races the janitor even at five.
/// Polling once a second holds the monitor at `Polled` with margin to spare.
///
/// If upstream ever shortens `IDLE_TICK`, this must shrink with it.
const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Tool descriptors owned by this module.
pub fn tools() -> Vec<Value> {
    vec![json!({
        "name": "watch_input",
        "description": "Watch for physical mouse input for a few seconds and report \
            what was pressed or scrolled. Use this to identify a button by feel — ask \
            the person to press the button they mean, then call this — instead of \
            guessing at button numbers.",
        "inputSchema": json!({
            "type": "object",
            "properties": {
                "seconds": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": MAX_SECONDS,
                    "description": "How long to watch. Defaults to 5 seconds.",
                },
            },
            "additionalProperties": false,
        }),
    })]
}

/// Watch the agent's input hook for a fixed window and report what it saw.
pub async fn watch_input(arguments: &Value) -> Result<String, String> {
    let seconds = watch_window(arguments)?;
    let client = agent().await?;

    // The first poll is what enables monitoring, and it drains whatever the
    // hook happened to be holding from before the caller asked. Discarding it
    // is what keeps a stale press from being reported as the answer to "which
    // button did you just push".
    rpc(client.poll_event_monitor(context::current())).await?;

    // Poll repeatedly rather than sleeping through the window: see
    // [`POLL_INTERVAL`]. Each poll both keeps the monitor alive and drains
    // what has arrived, so events are accumulated here rather than left to
    // survive in the agent's bounded buffer.
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut events = Vec::new();
    while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
        if remaining.is_zero() {
            break;
        }
        tokio::time::sleep(remaining.min(POLL_INTERVAL)).await;
        events.extend(rpc(client.poll_event_monitor(context::current())).await?);
    }
    Ok(summarize(&events, seconds))
}

/// Read and bound the `seconds` argument.
fn watch_window(arguments: &Value) -> Result<u64, String> {
    match arguments.get("seconds") {
        None | Some(Value::Null) => Ok(DEFAULT_SECONDS),
        Some(Value::Number(number)) => number
            .as_u64()
            .filter(|seconds| (1..=MAX_SECONDS).contains(seconds))
            .ok_or_else(|| format!("`seconds` must be between 1 and {MAX_SECONDS}, not {number}")),
        Some(other) => Err(format!("`seconds` must be a number, not {other}")),
    }
}

/// Turn observed events into something worth reading aloud.
fn summarize(events: &[MonitorEvent], seconds: u64) -> String {
    if events.is_empty() {
        return format!(
            "nothing was pressed in {seconds} seconds. The agent's input hook may not be \
             running, or no button was touched — ask for the button to be pressed, then \
             call watch_input again."
        );
    }
    let mut lines = vec![format!("in {seconds} seconds:")];
    lines.extend(events.iter().map(describe));
    lines.join("\n")
}

/// One event, in words.
fn describe(event: &MonitorEvent) -> String {
    match event {
        MonitorEvent::Button { button, pressed } => {
            let action = if *pressed { "pressed" } else { "released" };
            format!("  button {button} {action}")
        }
        MonitorEvent::Scroll { delta_x, delta_y } => {
            format!("  scrolled (horizontal {delta_x}, vertical {delta_y})")
        }
        MonitorEvent::CaptureInterrupted => {
            "  capture was interrupted by the OS — some input may be missing".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use roadie_ipc::MonitorEvent;
    use serde_json::json;

    use std::time::Duration;

    use super::{DEFAULT_SECONDS, MAX_SECONDS, POLL_INTERVAL, summarize, watch_window};

    #[test]
    fn the_window_defaults_and_is_bounded() {
        assert_eq!(watch_window(&json!({})).expect("valid"), DEFAULT_SECONDS);
        assert_eq!(watch_window(&json!({ "seconds": 1 })).expect("valid"), 1);
        assert_eq!(
            watch_window(&json!({ "seconds": MAX_SECONDS })).expect("valid"),
            MAX_SECONDS
        );
        watch_window(&json!({ "seconds": 0 })).expect_err("zero is not a window");
        watch_window(&json!({ "seconds": MAX_SECONDS + 1 })).expect_err("too long");
        watch_window(&json!({ "seconds": "five" })).expect_err("not a number");
    }

    #[test]
    fn an_empty_window_explains_itself_rather_than_reporting_success() {
        let summary = summarize(&[], 5);
        assert!(summary.contains("nothing was pressed"));
        assert!(
            summary.contains("watch_input again"),
            "the model needs a next step, not just a negative"
        );
    }

    #[test]
    fn observed_events_are_described_in_words() {
        let events = vec![
            MonitorEvent::Button {
                button: "Back".to_string(),
                pressed: true,
            },
            MonitorEvent::Button {
                button: "Back".to_string(),
                pressed: false,
            },
        ];
        let summary = summarize(&events, 5);
        assert!(summary.contains("button Back pressed"));
        assert!(summary.contains("button Back released"));
    }

    /// The agent's janitor disables an unpolled monitor and drops its buffer
    /// after two three-second ticks. A watch that polls less often than that
    /// silently loses the press it was asked to catch, so the margin is
    /// asserted rather than left to a comment.
    #[test]
    fn the_poll_interval_stays_well_inside_the_agents_idle_tick() {
        const AGENT_IDLE_TICK: Duration = Duration::from_secs(3);
        assert!(
            POLL_INTERVAL < AGENT_IDLE_TICK,
            "polling must refresh the monitor before the janitor can idle it"
        );
        assert!(
            POLL_INTERVAL.as_millis() * 2 <= AGENT_IDLE_TICK.as_millis(),
            "leave room for scheduling jitter and RPC latency, not just a bare inequality"
        );
    }

    #[test]
    fn an_interrupted_capture_is_surfaced_not_hidden() {
        let summary = summarize(&[MonitorEvent::CaptureInterrupted], 5);
        assert!(summary.contains("interrupted"));
    }
}
