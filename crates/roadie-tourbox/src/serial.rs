//! The host half: finding a TourBox's serial port and reading events off it.
//!
//! A TourBox appears as a USB CDC serial port. Reading it needs no
//! handshake and no write access — the device streams events from the
//! moment it is plugged in, whether or not anything has ever talked to it.
//! That is the whole reason this module is as small as it is.
//!
//! Discovery is deliberately narrow. A serial port is not a safe thing to
//! open speculatively: the same interface carries modems, microcontrollers
//! and industrial equipment, and writing to the wrong one can do real harm.
//! So [`ports`] returns only ports whose USB identity is a TourBox this
//! build recognises, and anything else has to be named explicitly through
//! [`TourBox::open_path`].

use std::io::{ErrorKind, Read};
use std::time::Duration;

use serialport::{SerialPort, SerialPortType};
use thiserror::Error;

use crate::ProtocolError;
use crate::event::{Event, decode};
use crate::model::{Model, identify};

/// The line rate a TourBox is driven at.
///
/// A USB CDC port carries bytes over USB regardless of what rate is asked
/// for, so this matters less than it would on a real UART. It is set anyway
/// because the value is what every other driver uses, and a port left at
/// whatever the last program chose is a difference nobody wants to debug.
pub const BAUD_RATE: u32 = 115_200;

/// How long a read waits before reporting that nothing arrived.
///
/// Short enough that a caller polling for events stays responsive to a
/// cancel, long enough not to spin.
const READ_TIMEOUT: Duration = Duration::from_millis(100);

/// Why a TourBox could not be found, opened or read.
#[derive(Debug, Error)]
pub enum SerialError {
    /// The host's serial ports could not be listed at all.
    #[error("could not list this computer's serial ports: {reason}")]
    Enumeration {
        /// What the platform reported.
        reason: String,
    },
    /// A port was found but would not open.
    #[error("could not open the TourBox at {path}: {reason}")]
    Open {
        /// The port that would not open.
        path: String,
        /// What the platform reported.
        reason: String,
    },
    /// The port opened but a read failed.
    #[error("could not read from the TourBox at {path}: {reason}")]
    Read {
        /// The port being read.
        path: String,
        /// What the platform reported.
        reason: String,
    },
    /// The port closed underneath us, which is what unplugging looks like.
    #[error("the TourBox at {path} was disconnected")]
    Disconnected {
        /// The port that went away.
        path: String,
    },
    /// A byte arrived that the protocol does not explain.
    #[error("the TourBox at {path} sent something unexpected: {source}")]
    Protocol {
        /// The port it came from.
        path: String,
        /// What was wrong with it.
        source: ProtocolError,
    },
}

/// A serial port that a TourBox is behind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Port {
    /// The device path to open.
    pub path: String,
    /// Which TourBox it is.
    pub model: &'static Model,
    /// The serial number the USB descriptor carries, when it carries one.
    pub serial_number: Option<String>,
}

impl Port {
    /// A sentence naming this port, for a screen reader.
    #[must_use]
    pub fn describe(&self) -> String {
        match &self.serial_number {
            Some(serial) => format!("{} at {}, serial {serial}", self.model.name, self.path),
            None => format!("{} at {}", self.model.name, self.path),
        }
    }
}

/// Every TourBox this build recognises, by its USB identity.
///
/// A TourBox Neo reaches the host through a general-purpose serial bridge
/// and so cannot be told apart from any other device using the same bridge;
/// it will not appear here. See [`crate::model::CP210X_VENDOR_ID`].
///
/// # Errors
///
/// [`SerialError::Enumeration`] if the platform's port list cannot be read.
pub fn ports() -> Result<Vec<Port>, SerialError> {
    let found = serialport::available_ports().map_err(|error| SerialError::Enumeration {
        reason: error.to_string(),
    })?;

    let mut ports = Vec::new();
    for port in found {
        let SerialPortType::UsbPort(usb) = &port.port_type else {
            continue;
        };
        let Some(model) = identify(usb.vid, usb.pid) else {
            continue;
        };
        if !is_usable_path(&port.port_name) {
            continue;
        }
        tracing::debug!(
            path = %port.port_name,
            model = model.name,
            "found a TourBox"
        );
        ports.push(Port {
            path: port.port_name.clone(),
            model,
            serial_number: usb.serial_number.clone(),
        });
    }
    ports.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(ports)
}

/// Whether a port path is the one to actually open.
///
/// macOS publishes every serial device twice: `/dev/tty.NAME` is the
/// dial-in side and `/dev/cu.NAME` is the call-out side. They are the same
/// hardware, so listing both would report one TourBox as two, and the
/// `tty.` half is the wrong one to hand anybody — opening it blocks waiting
/// for a carrier signal that a controller never asserts. Found on a TourBox
/// Elite on 2026-08-31, where enumeration returned both.
///
/// Every other platform names a serial port once, so there is nothing to
/// choose between and this is the identity function there.
#[cfg(target_os = "macos")]
fn is_usable_path(path: &str) -> bool {
    !path.rsplit('/').next().unwrap_or(path).starts_with("tty.")
}

/// Whether a port path is the one to actually open. See the macOS version:
/// only macOS publishes a serial device under two names.
#[cfg(not(target_os = "macos"))]
fn is_usable_path(_path: &str) -> bool {
    true
}

/// An open TourBox.
pub struct TourBox {
    port: Box<dyn SerialPort>,
    path: String,
    model: Option<&'static Model>,
}

impl std::fmt::Debug for TourBox {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TourBox")
            .field("path", &self.path)
            .field("model", &self.model.map(|model| model.name))
            .finish_non_exhaustive()
    }
}

impl TourBox {
    /// Open a TourBox found by [`ports`].
    ///
    /// # Errors
    ///
    /// [`SerialError::Open`] if the port will not open. On macOS that is
    /// usually another program already holding it.
    pub fn open(port: &Port) -> Result<Self, SerialError> {
        Self::open_path(&port.path, Some(port.model))
    }

    /// Open a serial port that is believed to be a TourBox.
    ///
    /// For a model whose USB identity cannot be recognised — see [`ports`]
    /// — where the caller has named the port themselves.
    ///
    /// # Errors
    ///
    /// [`SerialError::Open`] if the port will not open.
    pub fn open_path(path: &str, model: Option<&'static Model>) -> Result<Self, SerialError> {
        let port = serialport::new(path, BAUD_RATE)
            .timeout(READ_TIMEOUT)
            .open()
            .map_err(|error| SerialError::Open {
                path: path.to_owned(),
                reason: error.to_string(),
            })?;
        tracing::debug!(path, "opened a TourBox");
        Ok(Self {
            port,
            path: path.to_owned(),
            model,
        })
    }

    /// The port this was opened on.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Which TourBox this is, when that is known.
    #[must_use]
    pub fn model(&self) -> Option<&'static Model> {
        self.model
    }

    /// Wait for the next event.
    ///
    /// `Ok(None)` means the wait expired with nothing to report, which is
    /// the ordinary state of a controller nobody is touching — not an
    /// error, and not a reason to stop polling.
    ///
    /// # Errors
    ///
    /// [`SerialError::Disconnected`] when the device goes away,
    /// [`SerialError::Read`] for any other I/O failure, and
    /// [`SerialError::Protocol`] for a byte the protocol does not explain.
    /// A protocol error is worth reporting and is not worth stopping for:
    /// the next byte is a fresh event, because the encoding has no framing
    /// to resynchronise.
    pub fn read_event(&mut self) -> Result<Option<Event>, SerialError> {
        let mut byte = [0u8; 1];
        match self.port.read(&mut byte) {
            Ok(0) => Ok(None),
            Ok(_) => {
                let event = decode(byte[0]).map_err(|source| SerialError::Protocol {
                    path: self.path.clone(),
                    source,
                })?;
                tracing::trace!(byte = byte[0], event = %event.describe(), "TourBox event");
                Ok(Some(event))
            }
            Err(error) if error.kind() == ErrorKind::TimedOut => Ok(None),
            // An unplugged USB serial port stops answering rather than
            // reporting a closed file, and the kind it reports for that
            // differs per platform. Both mean the same thing to a caller.
            Err(error)
                if matches!(
                    error.kind(),
                    ErrorKind::BrokenPipe | ErrorKind::NotConnected | ErrorKind::UnexpectedEof
                ) =>
            {
                Err(SerialError::Disconnected {
                    path: self.path.clone(),
                })
            }
            Err(error) => Err(SerialError::Read {
                path: self.path.clone(),
                reason: error.to_string(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::MODELS;

    /// The description is what a screen reader is handed, so it has to be a
    /// sentence and it has to name the port someone would type.
    #[test]
    fn a_port_describes_itself_with_its_path() {
        let port = Port {
            path: "/dev/cu.usbmodem000000011".to_owned(),
            model: &MODELS[0],
            serial_number: None,
        };
        assert_eq!(
            port.describe(),
            "TourBox Elite at /dev/cu.usbmodem000000011"
        );
    }

    /// A serial number is worth saying when there is one, because it is how
    /// two identical controllers are told apart.
    #[test]
    fn a_port_with_a_serial_number_says_it() {
        let port = Port {
            path: "/dev/cu.usbmodem000000011".to_owned(),
            model: &MODELS[0],
            serial_number: Some("00000001".to_owned()),
        };
        assert_eq!(
            port.describe(),
            "TourBox Elite at /dev/cu.usbmodem000000011, serial 00000001"
        );
    }

    /// macOS names one serial device twice. Listing both reported a single
    /// TourBox as two on real hardware, and the `tty.` name is the one that
    /// blocks on open, so it is the one dropped.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_dial_in_half_of_a_macos_serial_port_is_not_listed() {
        assert!(is_usable_path("/dev/cu.usbmodem000000011"));
        assert!(!is_usable_path("/dev/tty.usbmodem000000011"));
    }

    /// The filter keys on the file name, not on the string anywhere. A
    /// directory that happens to contain "tty." must not disqualify a port.
    #[cfg(target_os = "macos")]
    #[test]
    fn only_the_file_name_decides_whether_a_port_is_the_dial_in_half() {
        assert!(is_usable_path("/dev/tty.d/cu.usbmodem1"));
    }

    /// Enumeration must not fail on a machine with no TourBox attached, and
    /// must not invent one. This runs on CI hosts with no hardware at all.
    /// When hardware *is* attached it also holds the rule above: one
    /// controller is one entry.
    #[test]
    fn enumeration_on_a_machine_with_no_tourbox_finds_nothing_and_does_not_fail() {
        match ports() {
            Ok(found) => {
                // One controller is one entry. Keyed on the serial number
                // rather than on the path, because the duplicate this
                // guards against is the same device under two paths.
                let mut serials: Vec<&str> = found
                    .iter()
                    .filter_map(|port| port.serial_number.as_deref())
                    .collect();
                let before = serials.len();
                serials.sort_unstable();
                serials.dedup();
                assert_eq!(
                    before,
                    serials.len(),
                    "one TourBox was listed more than once: {found:?}"
                );
                for port in &found {
                    assert_eq!(port.model.name, "TourBox Elite");
                }
            }
            // A container with no serial subsystem at all is a legitimate
            // outcome here, and not a failure of this crate.
            Err(SerialError::Enumeration { .. }) => {}
            Err(other) => panic!("enumeration reported the wrong kind of failure: {other}"),
        }
    }
}
