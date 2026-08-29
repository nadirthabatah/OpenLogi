//! Talking to a light over the network.
//!
//! Three requests, no authentication, no session: read the identity, read the
//! state, write the state. The whole client is thin enough to read in one
//! sitting, and the parts worth care are the timeouts and what happens when a
//! light is asleep.
//!
//! # A light that does not answer is the normal case
//!
//! These are Wi-Fi devices on a desk. They get unplugged, they drop off the
//! network, and their address changes when the router feels like it. So the
//! timeouts here are short and every failure says which light and what was
//! being attempted — an operation that hangs for thirty seconds while someone
//! waits to hear whether their light came on is worse than one that fails in
//! two and says so.

use std::net::IpAddr;
use std::time::Duration;

use crate::info::AccessoryInfo;
use crate::state::{Light, Lights};
use crate::{INFO_PATH, LIGHTS_PATH, PORT};

/// How long to wait for a light to answer.
///
/// Deliberately short. A light on the same network answers in milliseconds; a
/// light that is unplugged never answers at all, and the useful thing to do
/// about that is say so quickly rather than hold the terminal.
const TIMEOUT: Duration = Duration::from_secs(2);

/// One light, at an address.
#[derive(Debug, Clone)]
pub struct KeyLight {
    address: IpAddr,
    port: u16,
    /// The name to use in messages before the light has been asked for one.
    name: String,
}

impl KeyLight {
    /// A light at `address`, on the standard port.
    #[must_use]
    pub fn at(address: IpAddr) -> Self {
        Self {
            address,
            port: PORT,
            name: address.to_string(),
        }
    }

    /// A light at `address` on a given port, for a unit behind a forward.
    #[must_use]
    pub fn at_port(address: IpAddr, port: u16) -> Self {
        Self {
            address,
            port,
            name: format!("{address}:{port}"),
        }
    }

    /// Give this light a name for messages, from discovery or from its own
    /// accessory info.
    #[must_use]
    pub fn named(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// The name this light is known by so far.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Where this light is.
    #[must_use]
    pub const fn address(&self) -> IpAddr {
        self.address
    }

    /// The port it answers on.
    ///
    /// Nearly always [`PORT`](crate::PORT), but a light is addressed by both
    /// halves, and a caller storing a handle to come back to needs the pair.
    #[must_use]
    pub const fn port(&self) -> u16 {
        self.port
    }

    /// The URL of one of the light's two endpoints.
    ///
    /// A literal IPv6 address needs brackets in a URL, and forgetting them
    /// produces a parse error rather than a connection failure — which sends
    /// someone looking at their network instead of at this line.
    fn url(&self, path: &str) -> String {
        match self.address {
            IpAddr::V4(address) => format!("http://{address}:{}{path}", self.port),
            IpAddr::V6(address) => format!("http://[{address}]:{}{path}", self.port),
        }
    }

    /// A configured agent.
    ///
    /// Built per request rather than kept. These are three short calls to a
    /// device on the local network, made seconds or minutes apart, so a
    /// pooled connection would be closed by the light long before it was
    /// reused and holding one open would keep a socket alive for nothing.
    fn agent() -> ureq::Agent {
        ureq::Agent::config_builder()
            .timeout_global(Some(TIMEOUT))
            .build()
            .into()
    }

    /// Shape a failure so it names the light and what was being done to it.
    fn failed(&self, doing: &str, error: &impl std::fmt::Display) -> NetError {
        NetError::Unreachable {
            light: self.name.clone(),
            doing: doing.to_owned(),
            reason: error.to_string(),
        }
    }

    /// Ask the light what it is.
    ///
    /// # Errors
    ///
    /// Returns [`NetError`] if the light did not answer or answered with
    /// something that is not its accessory info.
    pub fn info(&self) -> Result<AccessoryInfo, NetError> {
        let body = Self::agent()
            .get(self.url(INFO_PATH))
            .call()
            .map_err(|error| self.failed("asking what it is", &error))?
            .body_mut()
            .read_to_string()
            .map_err(|error| self.failed("reading what it is", &error))?;
        serde_json::from_str(&body).map_err(|error| self.malformed("its identity", &error))
    }

    /// Read the light's current state.
    ///
    /// # Errors
    ///
    /// Returns [`NetError`] if the light did not answer, answered with
    /// something unparseable, or listed no lights.
    pub fn read(&self) -> Result<Light, NetError> {
        let body = Self::agent()
            .get(self.url(LIGHTS_PATH))
            .call()
            .map_err(|error| self.failed("reading its state", &error))?
            .body_mut()
            .read_to_string()
            .map_err(|error| self.failed("reading its state", &error))?;
        let lights: Lights =
            serde_json::from_str(&body).map_err(|error| self.malformed("its state", &error))?;
        lights.first().map_err(|error| NetError::Refused {
            light: self.name.clone(),
            reason: error.to_string(),
        })
    }

    /// Write a light's state.
    ///
    /// The light answers a `PUT` with its resulting state, which is the whole
    /// reason this returns one: it is the only way to know that what was asked
    /// for is what happened, and the light clamps values of its own accord.
    ///
    /// # Errors
    ///
    /// Returns [`NetError`] if the light did not answer or refused the change.
    pub fn write(&self, light: Light) -> Result<Light, NetError> {
        let body = serde_json::to_string(&Lights::one(light))
            .map_err(|error| self.malformed("the change being sent", &error))?;
        let answer = Self::agent()
            .put(self.url(LIGHTS_PATH))
            .content_type("application/json")
            .send(&body)
            .map_err(|error| self.failed("changing its settings", &error))?
            .body_mut()
            .read_to_string()
            .map_err(|error| self.failed("reading back the change", &error))?;
        let lights: Lights = serde_json::from_str(&answer)
            .map_err(|error| self.malformed("its answer to the change", &error))?;
        lights.first().map_err(|error| NetError::Refused {
            light: self.name.clone(),
            reason: error.to_string(),
        })
    }

    /// Shape a parse failure.
    fn malformed(&self, what: &str, error: &impl std::fmt::Display) -> NetError {
        NetError::Malformed {
            light: self.name.clone(),
            what: what.to_owned(),
            reason: error.to_string(),
        }
    }
}

/// Why talking to a light did not work.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NetError {
    /// The light did not answer.
    ///
    /// By far the commonest, and it is usually not a fault: these are Wi-Fi
    /// devices that get unplugged and get new addresses.
    #[error("{light} did not answer when {doing}: {reason}")]
    Unreachable {
        /// The light, by whatever name it is known.
        light: String,
        /// What was being attempted, in words fit to read aloud.
        doing: String,
        /// What the network said.
        reason: String,
    },

    /// The light answered with something that is not what it should be.
    #[error("{light} sent {what} in a form this build could not read: {reason}")]
    Malformed {
        /// The light.
        light: String,
        /// Which of its answers was wrong.
        what: String,
        /// What went wrong reading it.
        reason: String,
    },

    /// The light answered, and the answer was a refusal.
    #[error("{light} answered but would not do it: {reason}")]
    Refused {
        /// The light.
        light: String,
        /// What it said.
        reason: String,
    },
}

#[cfg(test)]
mod tests;
