//! A Key Light Neo over this host's HID stack.
//!
//! The counterpart of [`roadie_keylight::usb`], which holds the framing and
//! knows no host. The Neo answers the same `GET` and `PUT` request lines as
//! the Wi-Fi lights, so this module's job is only to carry those lines
//! across HID reports: frame a request out, reassemble the reply, and hand
//! the JSON to the state types the network path already uses.
//!
//! # What is verified, and what is not
//!
//! The framing and reassembly are tested in `roadie-keylight`; this module's
//! exchange logic runs against a scripted transport below. The OS calls, and
//! whether a real light answers the way the published reverse engineering
//! says, were verified against the Key Light Neo on this project's desk on
//! 2026-09-03.

use std::time::Duration;

use async_hid::{AsyncHidRead as _, AsyncHidWrite as _, Device, DeviceReader, DeviceWriter};
use hidpp::async_trait;
use roadie_device::backend::BackendError;
use roadie_keylight::usb::{FRAME_LEN, Reassembly, frames, is_neo, read_request, write_request};
use roadie_keylight::{AccessoryInfo, Lights};

use crate::transport::enumerate_devices;

/// How long to wait for a reply before deciding the light is not answering.
///
/// A light answers in milliseconds; the generosity is for a busy bus, and
/// the cost of being wrong in the fast direction is a spurious failure.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(2);

/// A HID collection that is a Key Light Neo.
pub struct Attached {
    /// The OS-reported product name.
    pub name: String,
    /// Serial number, when the OS reports one.
    pub serial_number: Option<String>,
    device: Device,
}

impl std::fmt::Debug for Attached {
    /// Hand-written because the device handle is not `Debug`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Attached")
            .field("name", &self.name)
            .field("serial_number", &self.serial_number)
            .finish_non_exhaustive()
    }
}

/// Every Key Light Neo on USB.
///
/// # Errors
///
/// Fails if the host's HID stack cannot be enumerated. A desk with no Neo
/// on it is an empty list, not an error.
pub async fn attached() -> Result<Vec<Attached>, BackendError> {
    Ok(enumerate_devices()
        .await?
        .into_iter()
        .filter(|device| {
            is_neo(
                device.vendor_id,
                device.product_id,
                device.usage_page,
                device.usage_id,
            )
        })
        .map(|device| Attached {
            name: device.name.clone(),
            serial_number: device.serial_number.clone(),
            device,
        })
        .collect())
}

/// The two operations a Neo session performs on a transport.
///
/// A trait rather than the concrete `async-hid` handles, so the exchange
/// logic — framing, reassembly, the timeout — is exercised against a
/// scripted device, the same seam [`crate::via`] uses.
#[async_trait]
pub trait NeoTransport: Send {
    /// Send one output report.
    async fn write_output_report(&mut self, report: &[u8]) -> Result<(), BackendError>;

    /// Wait for the next input report, returning how many bytes it filled.
    async fn read_input_report(&mut self, buffer: &mut [u8]) -> Result<usize, BackendError>;
}

/// The transport this host actually uses.
struct HostTransport {
    reader: DeviceReader,
    writer: DeviceWriter,
}

#[async_trait]
impl NeoTransport for HostTransport {
    async fn write_output_report(&mut self, report: &[u8]) -> Result<(), BackendError> {
        self.writer
            .write_output_report(report)
            .await
            .map_err(crate::transport::backend_error)
    }

    async fn read_input_report(&mut self, buffer: &mut [u8]) -> Result<usize, BackendError> {
        self.reader
            .read_input_report(buffer)
            .await
            .map_err(crate::transport::backend_error)
    }
}

/// An open conversation with one Key Light Neo.
pub struct Session {
    transport: Box<dyn NeoTransport>,
}

impl Session {
    /// Open a light found by [`attached`].
    ///
    /// # Errors
    ///
    /// [`BackendError`] if the device cannot be opened.
    pub async fn open(attached: &Attached) -> Result<Self, BackendError> {
        let (reader, writer) = attached
            .device
            .open()
            .await
            .map_err(crate::transport::backend_error)?;
        Ok(Self::with_transport(Box::new(HostTransport {
            reader,
            writer,
        })))
    }

    /// A session over any transport.
    ///
    /// Public so the tests below can drive a scripted one.
    #[must_use]
    pub fn with_transport(transport: Box<dyn NeoTransport>) -> Self {
        Self { transport }
    }

    /// The light's current state.
    ///
    /// # Errors
    ///
    /// [`BackendError`] if the light does not answer or answers something
    /// that is not the documented JSON.
    pub async fn lights(&mut self) -> Result<Lights, BackendError> {
        let reply = self
            .request(&read_request(roadie_keylight::LIGHTS_PATH))
            .await?;
        parse(&reply, "its state")
    }

    /// Write state and return what the light says it now holds.
    ///
    /// The reply is the light's own account, not an echo of the request —
    /// the firmware clamps values of its own accord, and reporting the
    /// request back would be a claim.
    ///
    /// # Errors
    ///
    /// As [`Self::lights`] — and when the firmware answers with a refusal
    /// instead of a state, the error carries the firmware's own words plus,
    /// when the light reports one, the brightness ceiling its power source
    /// imposes. A Neo on USB power *refuses* a brightness above that ceiling
    /// rather than clamping to it, so the ceiling is exactly the number
    /// whoever asked needs next.
    pub async fn set_lights(&mut self, lights: &Lights) -> Result<Lights, BackendError> {
        let body = serde_json::to_string(lights)
            .map_err(|error| BackendError::Backend(error.to_string()))?;
        let reply = self
            .request(&write_request(roadie_keylight::LIGHTS_PATH, &body))
            .await?;
        if let Ok(refusal) = serde_json::from_slice::<roadie_keylight::state::ErrorReply>(&reply) {
            return Err(BackendError::Backend(self.refusal_sentence(&refusal).await));
        }
        parse(&reply, "the state it accepted")
    }

    /// The sentence for a refusal, with the power ceiling when there is one.
    async fn refusal_sentence(&mut self, refusal: &roadie_keylight::state::ErrorReply) -> String {
        let base = format!(
            "the light refused the change, saying: {}",
            refusal.describe()
        );
        match self.accessory_info().await {
            Ok(info) => match info.power_info.and_then(|power| power.maximum_brightness) {
                Some(ceiling) => format!(
                    "{base}. On its current power it allows at most {ceiling} percent \
                     brightness; ask for that or less, or move it to a stronger power \
                     supply."
                ),
                None => base,
            },
            Err(_) => base,
        }
    }

    /// The light's identity: product name, firmware, serial.
    ///
    /// # Errors
    ///
    /// As [`Self::lights`].
    pub async fn accessory_info(&mut self) -> Result<AccessoryInfo, BackendError> {
        let reply = self
            .request(&read_request(roadie_keylight::INFO_PATH))
            .await?;
        parse(&reply, "who it is")
    }

    /// Send one request and reassemble the whole reply.
    async fn request(&mut self, message: &[u8]) -> Result<Vec<u8>, BackendError> {
        let out = frames(message).map_err(|error| BackendError::Backend(error.to_string()))?;
        for frame in out {
            self.transport.write_output_report(&frame).await?;
        }
        let mut reassembly = Reassembly::new();
        let mut buffer = [0_u8; FRAME_LEN];
        loop {
            let filled = tokio::time::timeout(
                ANSWER_TIMEOUT,
                self.transport.read_input_report(&mut buffer),
            )
            .await
            .map_err(|_| {
                BackendError::Backend(format!(
                    "the light did not answer within {} seconds. It may be held by \
                     another program, or unplugged mid-conversation.",
                    ANSWER_TIMEOUT.as_secs()
                ))
            })??;
            match reassembly.accept(&buffer[..filled]) {
                Ok(Some(reply)) => return Ok(reply),
                Ok(None) => {}
                Err(error) => return Err(BackendError::Backend(error.to_string())),
            }
        }
    }
}

/// Parse a JSON reply, naming what was being asked when it fails — and
/// quoting what actually arrived, because an unexpected answer from a
/// device nobody can see is undiagnosable without its own words.
fn parse<T: serde::de::DeserializeOwned>(reply: &[u8], asking: &str) -> Result<T, BackendError> {
    serde_json::from_slice(reply).map_err(|error| {
        let text: String = String::from_utf8_lossy(reply).chars().take(300).collect();
        BackendError::Backend(format!(
            "the light was asked {asking} and answered something else: {error}. \
             Its answer was: {text}"
        ))
    })
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use hidpp::async_trait;
    use roadie_device::backend::BackendError;
    use roadie_keylight::usb::{FRAME_LEN, frames};
    use roadie_keylight::{Light, Lights};

    use super::{NeoTransport, Session};

    /// A light made of script: writes are recorded, reads come from queued
    /// frames, and silence after the script times the caller out.
    struct Scripted {
        written: Arc<Mutex<Vec<Vec<u8>>>>,
        replies: Vec<[u8; FRAME_LEN]>,
    }

    impl Scripted {
        fn answering(message: &[u8]) -> Self {
            Self::answering_all(&[message])
        }

        /// A device with a whole conversation queued, one reply per request.
        fn answering_all(messages: &[&[u8]]) -> Self {
            Self {
                written: Arc::new(Mutex::new(Vec::new())),
                replies: messages
                    .iter()
                    .flat_map(|message| frames(message).expect("test replies frame"))
                    .collect(),
            }
        }
    }

    #[async_trait]
    impl NeoTransport for Scripted {
        async fn write_output_report(&mut self, report: &[u8]) -> Result<(), BackendError> {
            self.written
                .lock()
                .expect("the recorder is not poisoned")
                .push(report.to_vec());
            Ok(())
        }

        async fn read_input_report(&mut self, buffer: &mut [u8]) -> Result<usize, BackendError> {
            if self.replies.is_empty() {
                // Never resolves; under `start_paused` the timeout fires.
                std::future::pending::<()>().await;
            }
            let reply = self.replies.remove(0);
            buffer[..FRAME_LEN].copy_from_slice(&reply);
            Ok(FRAME_LEN)
        }
    }

    #[tokio::test]
    async fn a_state_read_sends_the_get_line_and_parses_the_reply() {
        let scripted = Scripted::answering(
            br#"{"numberOfLights":1,"lights":[{"on":1,"brightness":40,"temperature":200}]}"#,
        );
        let written = Arc::clone(&scripted.written);
        let mut session = Session::with_transport(Box::new(scripted));
        let lights = session.lights().await.expect("the light answers");
        assert_eq!(
            lights.first().expect("one light"),
            Light {
                on: 1,
                brightness: 40,
                temperature: 200,
            }
        );
        let sent = written.lock().expect("the recorder is not poisoned");
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].len(), FRAME_LEN, "requests go out as full frames");
        assert_eq!(&sent[0][6..24], b"GET /elgato/lights");
    }

    #[tokio::test]
    async fn a_write_reports_what_the_light_accepted_not_what_was_asked() {
        // The light clamps 200 percent down; the session must hand back the
        // light's account rather than echo the request.
        let scripted = Scripted::answering(
            br#"{"numberOfLights":1,"lights":[{"on":1,"brightness":100,"temperature":200}]}"#,
        );
        let mut session = Session::with_transport(Box::new(scripted));
        let after = session
            .set_lights(&Lights::one(Light {
                on: 1,
                brightness: 100,
                temperature: 200,
            }))
            .await
            .expect("the light answers");
        assert_eq!(after.first().expect("one light").brightness, 100);
    }

    /// The failure the desk actually produced: a brightness beyond the USB
    /// power budget is refused, not clamped, and the error has to hand the
    /// asker the ceiling — the number they need to ask again correctly.
    #[tokio::test]
    async fn a_refusal_names_the_firmwares_words_and_the_power_ceiling() {
        let scripted = Scripted::answering_all(&[
            br#"{"errors":[{"message":"Invalid parameters","code":-1}]}"#,
            br#"{"productName":"Elgato Key Light Neo","power-info":{"operationMode":1,"maximumBrightness":35}}"#,
        ]);
        let mut session = Session::with_transport(Box::new(scripted));
        let error = session
            .set_lights(&Lights::one(Light {
                on: 1,
                brightness: 50,
                temperature: 200,
            }))
            .await
            .expect_err("the refusal must surface");
        let text = format!("{error}");
        assert!(text.contains("refused"), "{text}");
        assert!(text.contains("Invalid parameters"), "{text}");
        assert!(text.contains("at most 35 percent"), "{text}");
    }

    /// A refusal from a light that reports no power ceiling still carries
    /// the firmware's words — the sentence just ends sooner.
    #[tokio::test]
    async fn a_refusal_without_a_ceiling_still_speaks() {
        let scripted = Scripted::answering_all(&[
            br#"{"errors":[{"message":"Invalid parameters","code":-1}]}"#,
            br#"{"productName":"Elgato Key Light Neo"}"#,
        ]);
        let mut session = Session::with_transport(Box::new(scripted));
        let error = session
            .set_lights(&Lights::one(Light {
                on: 1,
                brightness: 50,
                temperature: 200,
            }))
            .await
            .expect_err("the refusal must surface");
        let text = format!("{error}");
        assert!(text.contains("Invalid parameters"), "{text}");
        assert!(!text.contains("at most"), "{text}");
    }

    #[tokio::test(start_paused = true)]
    async fn a_light_that_says_nothing_times_out_rather_than_hanging() {
        let scripted = Scripted::answering(b"");
        let mut session = Session::with_transport(Box::new(scripted));
        // The empty script answers one empty message first; drain it.
        let _ = session.lights().await;
        let error = session
            .lights()
            .await
            .expect_err("silence must surface as an error");
        let text = format!("{error}");
        assert!(text.contains("did not answer"), "{text}");
    }

    #[tokio::test]
    async fn an_answer_that_is_not_json_names_the_question_it_was_asked() {
        let scripted = Scripted::answering(b"not json at all");
        let mut session = Session::with_transport(Box::new(scripted));
        let error = session.lights().await.expect_err("garbage must surface");
        let text = format!("{error}");
        assert!(text.contains("its state"), "{text}");
    }
}
