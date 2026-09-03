//! The half of `roadie light` that is an Elgato light on USB.
//!
//! A Key Light Neo has a USB data port and speaks the same protocol over it
//! as the Wi-Fi lights do over HTTP, so it belongs in the same list and
//! answers to the same verbs. This desk's Neo is the reason the path exists:
//! its vendor app exposes nothing to the accessibility system, so USB
//! control through this command is what makes the light usable at all
//! without sighted help.
//!
//! A Neo that is on Wi-Fi *and* plugged in would appear once per transport.
//! That is stated rather than deduplicated: the two entries answer through
//! different paths, and hiding one would hide the path someone is debugging.
//!
//! [`Found`] carries values rather than a device handle, the same choice the
//! network half makes, and for the same two reasons: the strings and the
//! selection are then testable without hardware, and a write re-finds its
//! light — so a light unplugged between the list and the verb becomes a
//! sentence naming it, not a stale handle failing strangely.

use anyhow::anyhow;
use roadie_hid::elgato_light::{Attached, Session, attached};
use roadie_keylight::Light;

/// One light on USB, and whatever it said about itself.
///
/// The state is optional for the same reason as the network half: presence
/// and readability are different questions, and a light that is plugged in
/// but not answering is worth a line rather than an omission.
#[derive(Debug, Clone)]
pub struct Found {
    /// The name the light reports over USB.
    pub name: String,
    /// The serial number, which is what tells two identical lights apart.
    pub serial_number: Option<String>,
    /// What it is doing, or why that could not be read.
    pub state: Result<Light, String>,
}

impl Found {
    /// The name the light reports over USB.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Every Elgato light on USB, with its current state.
///
/// A failure to enumerate is traced and returns an empty list, the same
/// choice the network half makes: the rest of the command must still work.
pub async fn find() -> Vec<Found> {
    let lights = match attached().await {
        Ok(lights) => lights,
        Err(error) => {
            tracing::debug!(%error, "could not look for Elgato lights on USB");
            return Vec::new();
        }
    };
    let mut found = Vec::new();
    for light in lights {
        let state = read(&light).await;
        found.push(Found {
            name: light.name.clone(),
            serial_number: light.serial_number.clone(),
            state,
        });
    }
    found
}

/// Ask one light what it is doing.
async fn read(light: &Attached) -> Result<Light, String> {
    let mut session = Session::open(light)
        .await
        .map_err(|error| error.to_string())?;
    let lights = session.lights().await.map_err(|error| error.to_string())?;
    lights.first().map_err(|error| error.to_string())
}

/// What one USB light's line says.
///
/// A free function over the name and state rather than a method over
/// [`Found`], so the strings that ship are the strings the tests sweep.
///
/// The words match the network half's, with the transport said out loud —
/// the same light can appear on both, and two identical lines would leave
/// no way to tell which one is which.
#[must_use]
pub fn line(name: &str, state: &Result<Light, String>) -> String {
    match state {
        Ok(light) if light.is_on() => format!(
            "  {} on USB is on at {} percent, {} kelvin.\n",
            name,
            light.brightness,
            light.kelvin()
        ),
        Ok(_) => format!("  {name} on USB is off.\n"),
        Err(why) => format!("  {name} is plugged in but did not answer: {why}\n"),
    }
}

/// The line for one found light.
#[must_use]
pub fn describe(found: &Found) -> String {
    line(found.name(), &found.state)
}

/// Apply a change and say what the light now holds.
///
/// Read-modify-write, like the network half: the light wants whole state,
/// and the reply below is the light's own account of what it accepted —
/// the firmware clamps of its own accord, so echoing the request would be
/// a claim.
///
/// # Errors
///
/// Fails when the light's earlier read already failed — a change applied to
/// unknown state would zero the fields the caller did not mean to touch —
/// or when the light went away or stopped answering between list and verb.
pub async fn write(found: &Found, change: impl FnOnce(Light) -> Light) -> anyhow::Result<()> {
    let light = apply(found, change).await?;
    print!("{}", line(found.name(), &Ok(light)));
    Ok(())
}

/// Apply a change and return what the light now holds.
///
/// The half of [`write`] with no output, for the MCP server — the two
/// surfaces share one implementation so they cannot drift apart.
///
/// # Errors
///
/// As [`write`].
pub async fn apply(found: &Found, change: impl FnOnce(Light) -> Light) -> anyhow::Result<Light> {
    let current = found
        .state
        .as_ref()
        .map_err(|why| anyhow!("{} did not answer: {why}", found.name()))?;
    let mut session = reopen(found).await?;
    let after = session
        .set_lights(&roadie_keylight::Lights::one(change(*current)))
        .await?;
    Ok(after.first()?)
}

/// Find the listed light again and open it.
///
/// Matched by serial number when the light reports one, and by name
/// otherwise. Re-finding rather than holding a handle is what turns "it was
/// unplugged since the list" into this sentence instead of an opaque I/O
/// failure.
async fn reopen(found: &Found) -> anyhow::Result<Session> {
    let lights = attached().await?;
    let wanted = lights
        .into_iter()
        .find(|light| match (&found.serial_number, &light.serial_number) {
            (Some(listed), Some(seen)) => listed == seen,
            _ => light.name == found.name,
        })
        .ok_or_else(|| {
            anyhow!(
                "{} is no longer on USB. It was there when it was listed; check its \
                 cable and run roadie light list again.",
                found.name()
            )
        })?;
    Ok(Session::open(&wanted).await?)
}
