//! One open conversation with a Focusrite interface.
//!
//! Everything here is ordering and checking; the bytes themselves are
//! [`roadie_scarlett`]'s. Three things in particular are the session's job
//! and are the reason this is not a thin wrapper:
//!
//! **The start-up handshake.** Two commands run before anything else, and
//! the second answers with the firmware version — which decides *which
//! table* the model's settings live in. A session that skipped it would
//! silently address the oldest layout.
//!
//! **The three shapes a write takes.** [`Plan`] says which applies, and they
//! are genuinely different: one writes an address, one reads a byte and puts
//! a single bit back, and one writes a scratch area and activates. The
//! read-modify-write is the one that matters — phantom power is a bit per
//! input inside a shared byte, and writing that byte outright would switch
//! phantom power off on every other input, silently.
//!
//! **The gate in front of 48 volts.** [`roadie_scarlett::risk`] decides what
//! is risky and this module refuses to write it unacknowledged. The
//! acknowledgement names the input it was given for, so agreeing once cannot
//! be spent on the next one.

use roadie_scarlett::config::{ConfigItem, ConfigSet, Descriptor};
use roadie_scarlett::device::Model;
use roadie_scarlett::risk::{Acknowledged, Risk, authorises};
use roadie_scarlett::transaction::{
    ACTIVATE_COMMAND, Plan, READ_COMMAND, WRITE_COMMAND, activate_payload, apply_bit, plan_write,
    read_payload, write_payload,
};
use roadie_scarlett::{INIT_1, INIT_2, INIT_2_RESPONSE_LEN, Request, Sequence, firmware_version};

use crate::transport::{Attached, Transport, UsbTransport};
use crate::{ControlError, Result};

/// An open conversation with one interface.
pub struct Session {
    transport: Box<dyn Transport>,
    sequence: Sequence,
    model: &'static Model,
    table: ConfigSet,
    firmware: u32,
}

impl Session {
    /// Open `attached` and bring it up.
    ///
    /// # Errors
    ///
    /// [`ControlError::Open`] or [`ControlError::Claim`] if the device will not open, and a
    /// protocol error if the handshake does not complete.
    pub fn open(attached: &Attached) -> Result<Self> {
        let transport = UsbTransport::open(attached)?;
        Self::with_transport(Box::new(transport), attached.model)
    }

    /// Bring a device up over any transport.
    ///
    /// Public so a caller can drive an interface over something other than
    /// this host's USB stack, and so the tests can drive a scripted one.
    ///
    /// # Errors
    ///
    /// A protocol error if the handshake does not complete, and
    /// [`ControlError::UnknownModel`] if the model has no table at the firmware
    /// version it reports.
    pub fn with_transport(transport: Box<dyn Transport>, model: &'static Model) -> Result<Self> {
        let mut session = Self {
            transport,
            sequence: Sequence::new(),
            model,
            // Replaced below, once the firmware version says which table
            // applies. Never read before then.
            table: ConfigSet {
                param_buf_addr: 0,
                items: &[],
            },
            firmware: 0,
        };

        session.exchange(INIT_1, &[], 0)?;
        let init_2 = session.exchange(INIT_2, &[], INIT_2_RESPONSE_LEN)?;
        session.firmware = firmware_version(&init_2)?;
        // The device reports a 32-bit version and the tables are keyed on 16.
        // Saturating rather than truncating: a version past 65535 is newer
        // than every threshold, and saturating picks the newest table, which
        // is what "newer than anything this build knows" should select.
        // Truncating could wrap it below every threshold and quietly pick the
        // oldest layout instead.
        let keyed = u16::try_from(session.firmware).unwrap_or(u16::MAX);
        session.table = model.table_for(keyed).ok_or(ControlError::UnknownModel {
            product_id: model.product_id,
        })?;
        tracing::debug!(
            model = model.name,
            firmware = session.firmware,
            "brought up a Focusrite interface"
        );
        Ok(session)
    }

    /// Which interface this is.
    #[must_use]
    pub const fn model(&self) -> &'static Model {
        self.model
    }

    /// The firmware version it reported during start-up.
    #[must_use]
    pub const fn firmware(&self) -> u32 {
        self.firmware
    }

    /// Send one command and check the answer belongs to it.
    fn exchange(&mut self, command: u32, payload: &[u8], expected: usize) -> Result<Vec<u8>> {
        let request = Request::new(command, payload, &mut self.sequence)?;
        self.transport.send(request.bytes())?;
        let bytes = self.transport.fetch(Request::response_len(expected))?;
        let response = request.parse_response(&bytes, expected)?;
        Ok(response.payload().to_vec())
    }

    /// Read `len` bytes from the device's address space.
    fn read_at(&mut self, offset: u16, len: u32) -> Result<Vec<u8>> {
        let payload = read_payload(offset, len);
        self.exchange(READ_COMMAND, &payload, len as usize)
    }

    /// Write bytes at an address.
    fn write_at(&mut self, offset: u16, value: &[u8]) -> Result<()> {
        let payload = write_payload(offset, value);
        self.exchange(WRITE_COMMAND, &payload, 0)?;
        Ok(())
    }

    /// Make a write take effect.
    ///
    /// Not optional where the table names a code. A write that needs
    /// activation and does not get it has changed the stored value and not
    /// the hardware — the interface carries on as it was, while a panel
    /// reading the value back sees the number it just wrote.
    fn activate(&mut self, code: u8) -> Result<()> {
        let payload = activate_payload(code);
        self.exchange(ACTIVATE_COMMAND, &payload, 0)?;
        Ok(())
    }

    /// Where `item` lives on this model, or an error naming what it lacks.
    fn descriptor(&self, item: ConfigItem, spoken: &'static str) -> Result<Descriptor> {
        self.table
            .descriptor(item)
            .ok_or(ControlError::NoSuchSetting {
                model: self.model.name,
                setting: spoken,
            })
    }

    /// Read one instance of a whole-byte setting.
    fn read_byte(&mut self, item: ConfigItem, spoken: &'static str, index: u16) -> Result<u8> {
        let descriptor = self.descriptor(item, spoken)?;
        let bytes = self.read_at(descriptor.address(index), 1)?;
        bytes.first().copied().ok_or(ControlError::Protocol(
            roadie_scarlett::ProtocolError::PayloadLength {
                expected: 1,
                actual: 0,
            },
        ))
    }

    /// Read one bit-sized setting, whose index selects the bit.
    fn read_bit(&mut self, item: ConfigItem, spoken: &'static str, index: u16) -> Result<bool> {
        let descriptor = self.descriptor(item, spoken)?;
        let bytes = self.read_at(descriptor.offset, 1)?;
        let byte = bytes.first().copied().unwrap_or(0);
        let shift = u8::try_from(index).unwrap_or(u8::MAX);
        Ok(shift < 8 && byte & (1 << shift) != 0)
    }

    /// Carry out one change, whichever of the three shapes it takes.
    fn apply(
        &mut self,
        item: ConfigItem,
        spoken: &'static str,
        index: u16,
        value: &[u8],
    ) -> Result<()> {
        let descriptor = self.descriptor(item, spoken)?;
        let plan = plan_write(descriptor, self.table.param_buf_addr, index, value)?;
        match plan {
            Plan::Direct {
                offset,
                value,
                activate,
            } => {
                self.write_at(offset, &value)?;
                if let Some(code) = activate {
                    self.activate(code)?;
                }
            }
            Plan::ModifyBit {
                offset,
                bit,
                enabled,
                activate,
            } => {
                // The read is not optional and its result cannot be assumed:
                // the byte's other bits belong to the neighbouring inputs,
                // and nothing the host has already asked for says what they
                // are set to.
                let current = self.read_at(offset, 1)?.first().copied().unwrap_or(0);
                self.write_at(offset, &[apply_bit(current, bit, enabled)])?;
                if let Some(code) = activate {
                    self.activate(code)?;
                }
            }
            Plan::Buffered {
                value_offset,
                value,
                index_offset,
                index,
                activate,
            } => {
                self.write_at(value_offset, &[value])?;
                self.write_at(index_offset, &[index])?;
                self.activate(activate)?;
            }
        }
        Ok(())
    }

    /// Refuse an input number the model does not have.
    ///
    /// `asked` counts from one, the way the numbers are printed on the box
    /// and the way somebody will say them out loud.
    fn check_input(&self, asked: u16, count: u8) -> Result<u16> {
        if asked == 0 || u16::from(count) < asked {
            return Err(ControlError::NoSuchInput {
                model: self.model.name,
                asked,
                count: u16::from(count),
            });
        }
        Ok(asked - 1)
    }

    /// Whether Mass Storage mode is on.
    ///
    /// # Errors
    ///
    /// [`ControlError::NoSuchSetting`] on a model without it, and a protocol error
    /// if the interface does not answer.
    pub fn msd_mode(&mut self) -> Result<bool> {
        Ok(self.read_byte(ConfigItem::MsdSwitch, "Mass Storage mode", 0)? != 0)
    }

    /// Switch Mass Storage mode off, which is the only useful direction.
    ///
    /// An interface ships in this mode, presenting a small USB disk carrying
    /// registration files. It is not a fault and nothing is broken while it
    /// is on — but it is the manufacturer's out-of-the-box state rather than
    /// the working one, and turning it off is the step that makes the
    /// interface an ordinary audio device.
    ///
    /// **The change needs a power cycle.** The write takes effect in the
    /// interface's stored settings immediately and in its behaviour only
    /// after it is unplugged and plugged back in. Saying so is the caller's
    /// job, and it matters: an interface that looks unchanged afterwards is
    /// the expected outcome, not a failed write.
    ///
    /// # Errors
    ///
    /// As [`Self::msd_mode`].
    pub fn set_msd_mode(&mut self, on: bool) -> Result<()> {
        self.apply(
            ConfigItem::MsdSwitch,
            "Mass Storage mode",
            0,
            &[u8::from(on)],
        )
    }

    /// Whether 48 volt phantom power is on for `input`, counted from one.
    ///
    /// # Errors
    ///
    /// [`ControlError::NoSuchInput`] for an input the model does not have, and
    /// [`ControlError::NoSuchSetting`] on a model whose phantom switch is physical.
    pub fn phantom(&mut self, input: u16) -> Result<bool> {
        let index = self.check_input(input, self.model.phantom_pairs)?;
        let descriptor = self.descriptor(ConfigItem::PhantomSwitch, "phantom power")?;
        let index = index + u16::from(self.model.phantom_first);
        if descriptor.is_whole_bytes() {
            Ok(self.read_byte(ConfigItem::PhantomSwitch, "phantom power", index)? != 0)
        } else {
            self.read_bit(ConfigItem::PhantomSwitch, "phantom power", index)
        }
    }

    /// Switch 48 volt phantom power for `input`, counted from one.
    ///
    /// Switching it **on** requires `acknowledged` to carry exactly the risk
    /// this call would take — the same input, not merely some input. Nothing
    /// else on the interface is gated, because a confirmation in front of
    /// harmless changes is how a real one stops being read.
    ///
    /// # Errors
    ///
    /// [`ControlError::NotAcknowledged`] when switching on without the matching
    /// acknowledgement, carrying the sentence to put to a person.
    pub fn set_phantom(
        &mut self,
        input: u16,
        on: bool,
        acknowledged: Option<Acknowledged>,
    ) -> Result<()> {
        let index = self.check_input(input, self.model.phantom_pairs)?;
        if let Some(risk) = Risk::of_phantom(input, on)
            && !authorises(acknowledged, risk)
        {
            return Err(ControlError::NotAcknowledged(risk.spoken()));
        }
        let index = index + u16::from(self.model.phantom_first);
        self.apply(
            ConfigItem::PhantomSwitch,
            "phantom power",
            index,
            &[u8::from(on)],
        )
    }

    /// The preamp gain on `input`, counted from one.
    ///
    /// # Errors
    ///
    /// [`ControlError::NoSuchInput`] or [`ControlError::NoSuchSetting`] as applicable.
    pub fn gain(&mut self, input: u16) -> Result<u8> {
        let index = self.check_input(input, self.model.gain_inputs)?;
        self.read_byte(ConfigItem::InputGain, "software gain control", index)
    }

    /// Set the preamp gain on `input`, counted from one.
    ///
    /// # Errors
    ///
    /// As [`Self::gain`].
    pub fn set_gain(&mut self, input: u16, value: u8) -> Result<()> {
        let index = self.check_input(input, self.model.gain_inputs)?;
        self.apply(
            ConfigItem::InputGain,
            "software gain control",
            index,
            &[value],
        )
    }

    /// Whether `input` is muted, counted from one.
    ///
    /// # Errors
    ///
    /// As [`Self::gain`].
    pub fn muted(&mut self, input: u16) -> Result<bool> {
        let index = self.check_input(input, self.model.mute_inputs)?;
        Ok(self.read_byte(ConfigItem::InputMuteSwitch, "a mute switch", index)? != 0)
    }

    /// Mute or unmute `input`, counted from one.
    ///
    /// # Errors
    ///
    /// As [`Self::gain`].
    pub fn set_muted(&mut self, input: u16, muted: bool) -> Result<()> {
        let index = self.check_input(input, self.model.mute_inputs)?;
        self.apply(
            ConfigItem::InputMuteSwitch,
            "a mute switch",
            index,
            &[u8::from(muted)],
        )
    }

    /// Everything this model can say about itself right now.
    ///
    /// One call rather than several because the question a person asks is
    /// "what is my interface doing", and answering it in pieces means one
    /// round trip per piece and a list that can disagree with itself halfway
    /// down.
    ///
    /// # Errors
    ///
    /// A protocol error if the interface stops answering partway. A setting
    /// the model does not have is not an error — it is simply absent from
    /// the result.
    pub fn snapshot(&mut self) -> Result<Snapshot> {
        let msd_mode = self.msd_mode().ok();
        let mut inputs = Vec::new();
        let widest = self
            .model
            .gain_inputs
            .max(self.model.mute_inputs)
            .max(self.model.phantom_pairs);
        for number in 1..=u16::from(widest) {
            inputs.push(Settings {
                input: number,
                gain: self.gain(number).ok(),
                muted: self.muted(number).ok(),
                phantom: self.phantom(number).ok(),
            });
        }
        Ok(Snapshot {
            model: self.model.name,
            firmware: self.firmware,
            msd_mode,
            inputs,
        })
    }
}

/// What one input is doing.
///
/// Every field is optional because models differ in which of these they
/// have: a `None` means this interface has no such control on this input,
/// not that the read failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// Which input, counted from one.
    pub input: u16,
    /// Preamp gain, where the model has software gain control.
    pub gain: Option<u8>,
    /// Whether the input is muted.
    pub muted: Option<bool>,
    /// Whether 48 volt phantom power is on.
    pub phantom: Option<bool>,
}

/// Everything an interface reports in one go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// What the model is called on the box.
    pub model: &'static str,
    /// The firmware version it reported.
    pub firmware: u32,
    /// Whether Mass Storage mode is on, where the model has the switch.
    pub msd_mode: Option<bool>,
    /// One entry per input.
    pub inputs: Vec<Settings>,
}

#[cfg(test)]
mod tests;
