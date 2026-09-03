//! An interface made of software.
//!
//! The counterpart of `roadie-display`'s scripted panel, and it exists for
//! the same reason: everything above the transport — the handshake, the
//! sequence checking, the read-modify-write, the gate in front of 48 volts —
//! is logic that should be provable without an audio interface to risk. And
//! the one thing that could go badly wrong here really would go badly wrong:
//! a phantom write that cleared its neighbours would be silent on a desk and
//! obvious against this.
//!
//! It answers *addresses*, not scripted replies, so a read after a write
//! returns what the write actually did rather than what a fixture author
//! believed it would do. A test that spells out the answer proves the
//! fixture; this proves the code.

use std::sync::{Arc, Mutex};

use roadie_scarlett::config::{ConfigSet, DATA_CMD, GET_DATA, SET_DATA};
use roadie_scarlett::device::Model;
use roadie_scarlett::packet::HEADER_LEN;
use roadie_scarlett::transaction::apply_bit;
use roadie_scarlett::{INIT_1, INIT_2, INIT_2_RESPONSE_LEN};

use crate::transport::Transport;
use crate::{ControlError, Result};

/// How much address space the mock keeps.
///
/// Comfortably past the highest address any table in `roadie-scarlett`
/// names, so a write to a real offset lands somewhere rather than being
/// rejected by the fixture and mistaken for a protocol failure.
const MEMORY: usize = 0x0400;

/// What the mock has been told and what it holds.
#[derive(Debug)]
pub struct State {
    /// The device's address space.
    pub memory: Vec<u8>,
    /// Every activation code sent, in order.
    ///
    /// Recorded because a write that needs activation and does not get it is
    /// the failure with no symptom: the stored value changes, the hardware
    /// does not, and a read afterwards agrees with the write.
    pub activations: Vec<u8>,
}

/// A scripted Focusrite interface.
pub struct Panel {
    state: Arc<Mutex<State>>,
    firmware: u32,
    table: ConfigSet,
    pending: Option<Vec<u8>>,
    name: String,
}

impl Panel {
    /// A device of `model` reporting `firmware`, with an empty address space.
    ///
    /// It is given the model's table for one reason: to apply a buffered
    /// write the way the firmware does. A write through the scratch area
    /// only reaches the setting when the activation arrives, so a mock that
    /// stored the scratch bytes and stopped would answer every read-back
    /// with the old value — and a test written against that would be
    /// asserting the mock's shortcut rather than the code's correctness.
    #[must_use]
    pub fn new(model: &'static Model, firmware: u32) -> Self {
        let table = model
            .table_for(u16::try_from(firmware).unwrap_or(u16::MAX))
            .unwrap_or(ConfigSet {
                param_buf_addr: 0,
                items: &[],
            });
        Self {
            state: Arc::new(Mutex::new(State {
                memory: vec![0; MEMORY],
                activations: Vec::new(),
            })),
            firmware,
            table,
            pending: None,
            name: format!("a mock {}", model.name),
        }
    }

    /// A handle on what the mock holds, for setting it up and checking it.
    #[must_use]
    pub fn state(&self) -> Arc<Mutex<State>> {
        Arc::clone(&self.state)
    }

    /// The little-endian header a device puts in front of every answer.
    fn header(command: u32, sequence: u16, payload: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(HEADER_LEN + payload.len());
        bytes.extend_from_slice(&command.to_le_bytes());
        bytes.extend_from_slice(
            &u16::try_from(payload.len())
                .unwrap_or(u16::MAX)
                .to_le_bytes(),
        );
        bytes.extend_from_slice(&sequence.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(payload);
        bytes
    }

    /// The four bytes at `at`, as a little-endian `u32`.
    fn word(payload: &[u8], at: usize) -> u32 {
        payload
            .get(at..at + 4)
            .and_then(|slice| slice.try_into().ok())
            .map_or(0, u32::from_le_bytes)
    }
}

impl Transport for Panel {
    fn send(&mut self, bytes: &[u8]) -> Result<()> {
        let command = Self::word(bytes, 0);
        let sequence = u16::from_le_bytes([bytes[6], bytes[7]]);
        let payload = bytes.get(HEADER_LEN..).unwrap_or_default();
        let mut state = self.state.lock().map_err(|_| ControlError::Transfer {
            name: self.name.clone(),
            reason: "the mock's lock was poisoned".to_owned(),
        })?;

        let answer = match command {
            INIT_1 => Self::header(command, sequence, &[]),
            INIT_2 => {
                let mut reply = vec![0_u8; INIT_2_RESPONSE_LEN];
                reply[8..12].copy_from_slice(&self.firmware.to_le_bytes());
                // The documented start-up quirk: the request carrying
                // sequence 1 is answered with sequence 0. Reproduced rather
                // than smoothed over, so the exception the protocol layer
                // makes for it is exercised rather than assumed.
                Self::header(command, sequence.saturating_sub(1), &reply)
            }
            GET_DATA => {
                let offset = Self::word(payload, 0) as usize;
                let len = Self::word(payload, 4) as usize;
                let slice = state
                    .memory
                    .get(offset..offset + len)
                    .unwrap_or_default()
                    .to_vec();
                Self::header(command, sequence, &slice)
            }
            SET_DATA => {
                let offset = Self::word(payload, 0) as usize;
                let len = Self::word(payload, 4) as usize;
                let value = payload.get(8..8 + len).unwrap_or_default().to_vec();
                if let Some(target) = state.memory.get_mut(offset..offset + value.len()) {
                    target.copy_from_slice(&value);
                }
                Self::header(command, sequence, &[])
            }
            DATA_CMD => {
                let code = u8::try_from(Self::word(payload, 0)).unwrap_or(u8::MAX);
                state.activations.push(code);
                // Apply a buffered write the way the firmware does: take the
                // value and index out of the scratch area and put them where
                // the setting actually lives. Settings written straight to
                // their address need nothing here — the write already landed
                // and the activation only commits it.
                if let Some((_, descriptor)) = self
                    .table
                    .items
                    .iter()
                    .find(|(_, item)| item.via_param_buf && item.activate == code)
                {
                    let scratch = self.table.param_buf_addr as usize;
                    let value = state.memory.get(scratch).copied().unwrap_or(0);
                    let index = state.memory.get(scratch + 1).copied().unwrap_or(0);
                    if descriptor.is_whole_bytes() {
                        let at = descriptor.address(u16::from(index)) as usize;
                        if let Some(cell) = state.memory.get_mut(at) {
                            *cell = value;
                        }
                    } else {
                        // Bit-sized, so the index picks the bit rather than
                        // the address — the Vocaster's phantom power is this
                        // shape, and the neighbouring bits are other inputs.
                        let at = descriptor.offset as usize;
                        if let Some(cell) = state.memory.get_mut(at) {
                            *cell = apply_bit(*cell, index, value != 0);
                        }
                    }
                }
                Self::header(command, sequence, &[])
            }
            other => {
                return Err(ControlError::Transfer {
                    name: self.name.clone(),
                    reason: format!("the mock was sent command {other:#x}, which it does not know"),
                });
            }
        };
        self.pending = Some(answer);
        Ok(())
    }

    fn fetch(&mut self, len: usize) -> Result<Vec<u8>> {
        let mut answer = self.pending.take().ok_or_else(|| ControlError::Transfer {
            name: self.name.clone(),
            reason: "the answer was fetched before anything was asked".to_owned(),
        })?;
        // A device sends what it has, up to what was asked for.
        answer.truncate(len);
        Ok(answer)
    }

    fn name(&self) -> &str {
        &self.name
    }
}
