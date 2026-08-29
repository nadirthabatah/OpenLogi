//! Elgato Stream Decks over this host's HID stack.
//!
//! The counterpart of [`openlogi_streamdeck`], which holds the protocol and
//! knows no host. This module is the host: enumeration, opening the right
//! collection, and the report traffic. It mirrors how `openlogi-device` and
//! this crate split the HID++ side, so a second implementation of the
//! protocol layer stays possible and the protocol stays testable without a
//! device.
//!
//! # What is verified, and what is not
//!
//! The encodings this drives are unit-tested in `openlogi-streamdeck`, but no
//! part of this path has run against physical hardware. Two things need a real
//! device to confirm, and `openlogi streamdeck verify` exists to confirm them:
//! which HID collection carries the key and image traffic (see
//! [`Attached::is_preferred_collection`]), and the gen 1 report layouts.

use async_hid::{
    AsyncHidFeatureHandle as _, AsyncHidRead as _, Device, DeviceFeatureHandle, DeviceReader,
};
use openlogi_device::backend::BackendError;
use openlogi_streamdeck::model::{ELGATO_VENDOR_ID, Model, identify};
use openlogi_streamdeck::report::{self, Brightness, KeyEvent, KeyStates};

use crate::transport::enumerate_devices;

/// Largest input report this module will read.
///
/// The biggest key report in the catalog is the XL's four-byte header plus 32
/// keys; the surplus is headroom for a model that pads or reports more, since
/// a short read is decoded from its filled prefix anyway.
const INPUT_BUFFER: usize = 512;

/// A Stream Deck collection the OS is reporting.
///
/// One physical Stream Deck usually appears as several of these — a HID device
/// exposes one node per top-level collection — so this is "a way in", not "a
/// device". [`preferred`] picks between them.
pub struct Attached {
    /// Which Stream Deck this is.
    pub model: &'static Model,
    /// The OS-reported product name.
    pub name: String,
    /// Serial number, when the OS reports one.
    pub serial_number: Option<String>,
    /// HID usage page of this collection.
    pub usage_page: u16,
    /// HID usage id of this collection.
    pub usage_id: u16,
    device: Device,
}

impl Attached {
    /// Whether this collection is the one [`preferred`] would choose.
    ///
    /// Vendor-specific traffic conventionally lives on a vendor-defined usage
    /// page (`0xFF00` and above), and the Stream Deck's key and image reports
    /// are vendor-specific. That is the rule applied here.
    ///
    /// It is a convention rather than a fact read off the device, which is
    /// exactly why `openlogi streamdeck verify` prints every collection it
    /// found and which one was chosen: if the choice is wrong on some model,
    /// that output says so immediately instead of leaving a silent
    /// misbehaviour to diagnose.
    #[must_use]
    pub fn is_preferred_collection(&self) -> bool {
        self.usage_page >= 0xff00
    }
}

impl std::fmt::Debug for Attached {
    /// Hand-written because the open handle is not `Debug` and printing it
    /// would say nothing useful anyway; the usage pair is rendered in hex
    /// because that is how every HID reference writes it.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Attached")
            .field("model", &self.model.name)
            .field("name", &self.name)
            .field("serial_number", &self.serial_number)
            .field("usage_page", &format_args!("{:#06x}", self.usage_page))
            .field("usage_id", &format_args!("{:#06x}", self.usage_id))
            .finish_non_exhaustive()
    }
}

/// Every Stream Deck collection this host currently reports.
///
/// Returns all of them, including collections [`preferred`] would not choose, so a
/// caller diagnosing a device can see what the OS actually offers.
///
/// # Errors
///
/// Fails if the host's HID stack cannot be enumerated.
pub async fn attached() -> Result<Vec<Attached>, BackendError> {
    let devices = enumerate_devices().await?;
    Ok(devices
        .into_iter()
        .filter_map(|device| {
            let model = identify(device.vendor_id, device.product_id)?;
            Some(Attached {
                model,
                name: device.name.clone(),
                serial_number: device.serial_number.clone(),
                usage_page: device.usage_page,
                usage_id: device.usage_id,
                device,
            })
        })
        .collect())
}

/// How a physical device is told apart from another of the same model.
///
/// The serial number where the OS reports one; the product id otherwise, in
/// which case two identical serial-less decks collapse to one entry. That is
/// the honest outcome for hardware the host gives no way to distinguish, and
/// it is why `verify` prints serials.
fn identity(attached: &Attached) -> String {
    attached
        .serial_number
        .clone()
        .unwrap_or_else(|| format!("pid:{:04x}", attached.model.product_id))
}

/// Choose one collection per physical device, preferring the vendor page.
///
/// Borrows rather than clones: an open handle is not duplicable, and a
/// caller wants to open the collection this picked, not a copy of its
/// description.
#[must_use]
pub fn preferred(collections: &[Attached]) -> Vec<&Attached> {
    let mut chosen: Vec<&Attached> = Vec::new();
    for candidate in collections {
        match chosen
            .iter_mut()
            .find(|held| identity(held) == identity(candidate))
        {
            // A vendor-page collection replaces a generic one; otherwise the
            // first seen stands.
            Some(held)
                if !held.is_preferred_collection() && candidate.is_preferred_collection() =>
            {
                *held = candidate;
            }
            Some(_) => {}
            None => chosen.push(candidate),
        }
    }
    chosen
}

/// An open Stream Deck: a feature-report handle for control, and a reader for
/// key events.
pub struct Session {
    model: &'static Model,
    feature: DeviceFeatureHandle,
    reader: DeviceReader,
    states: KeyStates,
}

impl Session {
    /// Open `attached` for control and key events.
    ///
    /// # Errors
    ///
    /// Fails if either handle cannot be opened — on Linux typically a
    /// permissions problem on the `hidraw` node, which is what the udev rules
    /// exist to fix.
    pub async fn open(attached: &Attached) -> Result<Self, BackendError> {
        let feature = attached
            .device
            .open_feature_handle()
            .await
            .map_err(crate::transport::backend_error)?;
        let reader = attached
            .device
            .open_readable()
            .await
            .map_err(crate::transport::backend_error)?;
        Ok(Self {
            model: attached.model,
            feature,
            reader,
            states: KeyStates::released(attached.model),
        })
    }

    /// Which Stream Deck this session drives.
    #[must_use]
    pub fn model(&self) -> &'static Model {
        self.model
    }

    /// Set the key screens' brightness.
    ///
    /// # Errors
    ///
    /// Fails if the feature report cannot be written.
    pub async fn set_brightness(&mut self, brightness: Brightness) -> Result<(), BackendError> {
        let report = report::set_brightness(self.model, brightness);
        self.feature
            .write_feature_report(report.as_bytes())
            .await
            .map_err(crate::transport::backend_error)
    }

    /// Reset the device to its stock standby screen.
    ///
    /// # Errors
    ///
    /// Fails if the feature report cannot be written.
    pub async fn reset(&mut self) -> Result<(), BackendError> {
        let report = report::reset(self.model);
        self.feature
            .write_feature_report(report.as_bytes())
            .await
            .map_err(crate::transport::backend_error)
    }

    /// Wait for the next input report and return the key transitions in it.
    ///
    /// An empty result is normal: the device re-reports its whole keyboard, so
    /// a report in which nothing changed carries no transitions. A report this
    /// model does not use for key state is skipped rather than treated as an
    /// error, since a device may interleave other reports on the same
    /// collection.
    ///
    /// # Errors
    ///
    /// Fails if the read fails, or if a key-state report is malformed — a
    /// short report is an error rather than a row of invented key releases.
    pub async fn next_events(&mut self) -> Result<Vec<KeyEvent>, BackendError> {
        let mut buffer = [0u8; INPUT_BUFFER];
        let read = self
            .reader
            .read_input_report(&mut buffer)
            .await
            .map_err(crate::transport::backend_error)?;
        match report::decode_key_states(self.model, &buffer[..read]) {
            Ok(states) => {
                let events = states.changes_since(&self.states);
                self.states = states;
                Ok(events)
            }
            // Another report kind on the same collection is not this layer's
            // business; only a malformed *key* report is.
            Err(openlogi_streamdeck::ProtocolError::UnexpectedReport { .. }) => Ok(Vec::new()),
            Err(error) => Err(BackendError::Backend(error.to_string())),
        }
    }
}

/// An Elgato device this build does not recognize.
///
/// Reported separately rather than dropped: "my Stream Deck is not detected"
/// and "my Stream Deck is a model this build has never heard of" look
/// identical to a user, and only this distinguishes them. A product id here is
/// the one fact needed to add the model to the catalogue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unrecognized {
    /// USB product id, the value a catalogue entry is keyed on.
    pub product_id: u16,
    /// The OS-reported product name.
    pub name: String,
    /// HID usage page of this collection.
    pub usage_page: u16,
    /// HID usage id of this collection.
    pub usage_id: u16,
}

/// Every Elgato device this host reports that is *not* in the catalogue.
///
/// # Errors
///
/// Fails if the host's HID stack cannot be enumerated.
pub async fn unrecognized() -> Result<Vec<Unrecognized>, BackendError> {
    let devices = enumerate_devices().await?;
    let mut found: Vec<Unrecognized> = devices
        .into_iter()
        .filter(|device| is_elgato(device.vendor_id))
        .filter(|device| identify(device.vendor_id, device.product_id).is_none())
        .map(|device| Unrecognized {
            product_id: device.product_id,
            name: device.name.clone(),
            usage_page: device.usage_page,
            usage_id: device.usage_id,
        })
        .collect();
    found.sort_by_key(|device| (device.product_id, device.usage_page, device.usage_id));
    found.dedup();
    Ok(found)
}

/// Whether a vendor id could be a Stream Deck at all.
///
/// Exposed for callers filtering a device list before doing the fuller
/// [`identify`] lookup.
#[must_use]
pub fn is_elgato(vendor_id: u16) -> bool {
    vendor_id == ELGATO_VENDOR_ID
}

#[cfg(test)]
mod tests {
    use super::is_elgato;
    use openlogi_streamdeck::model::ELGATO_VENDOR_ID;

    #[test]
    fn only_elgato_is_a_candidate() {
        assert!(is_elgato(ELGATO_VENDOR_ID));
        assert!(!is_elgato(0x046d));
    }
}
