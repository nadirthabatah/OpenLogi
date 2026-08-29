//! Turning "set phantom power on pair two" into the exchanges that do it.
//!
//! A host holding a USB handle needs three things from this module: the bytes
//! of a read, the bytes of a write, and — because a write is rarely one
//! exchange — the *order* of the exchanges that make up one change.
//!
//! # The three shapes a write takes
//!
//! Which one applies is decided by the model's [`Descriptor`], not by the
//! caller, and getting it wrong is quiet rather than loud.
//!
//! [`Plan::Direct`] is the ordinary case: put the bytes at the address, then
//! activate.
//!
//! [`Plan::ModifyBit`] is for a setting narrower than a byte. Phantom power is
//! the one that matters — one bit per input pair, several pairs sharing a
//! byte — so the byte has to be read, one bit changed, and the byte written
//! back. Writing it outright would switch phantom power **off** on every other
//! pair, silently, while the panel showed only the pair that was asked for.
//!
//! [`Plan::Buffered`] is how the fourth generation and the Vocaster write. The
//! value and its index go into a scratch area and the activation does the rest;
//! the setting's own address is never written at all.
//!
//! # The two encodings that look right and are not
//!
//! A write's value field is **as wide as the setting**, not four bytes. An
//! eight-bit setting sends nine bytes — four of address, four of length, one of
//! value — and padding it to twelve makes the device reject the request rather
//! than complain about it.
//!
//! And a write that needs activation and does not get it has changed the stored
//! value without changing the hardware. Nothing reports an error; the interface
//! keeps behaving as it did, while a panel that read the value back sees the
//! number it just wrote.

use crate::config::{DATA_CMD, Descriptor, GET_DATA, SET_DATA};

/// The payload of a read.
///
/// Four bytes of address and four of length, little-endian. The answer is
/// `len` bytes with no header of its own beyond the packet's.
#[must_use]
pub fn read_payload(offset: u16, len: u32) -> Vec<u8> {
    let mut payload = Vec::with_capacity(8);
    payload.extend_from_slice(&u32::from(offset).to_le_bytes());
    payload.extend_from_slice(&len.to_le_bytes());
    payload
}

/// The command a read is sent under.
pub const READ_COMMAND: u32 = GET_DATA;

/// The command a write is sent under.
pub const WRITE_COMMAND: u32 = SET_DATA;

/// The command an activation is sent under.
pub const ACTIVATE_COMMAND: u32 = DATA_CMD;

/// The payload of a write.
///
/// The value is sent **as wide as the setting**, which is why this takes bytes
/// rather than a number: a caller with a `u32` in hand would have to decide how
/// much of it to send, and the answer is not four.
#[must_use]
pub fn write_payload(offset: u16, value: &[u8]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(8 + value.len());
    payload.extend_from_slice(&u32::from(offset).to_le_bytes());
    payload.extend_from_slice(&u32::try_from(value.len()).unwrap_or(u32::MAX).to_le_bytes());
    payload.extend_from_slice(value);
    payload
}

/// The payload of an activation.
#[must_use]
pub fn activate_payload(code: u8) -> Vec<u8> {
    u32::from(code).to_le_bytes().to_vec()
}

/// What a host must do to carry out one change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    /// Write bytes at an address, then activate if there is a code.
    Direct {
        /// Where.
        offset: u16,
        /// What, already the width of the setting.
        value: Vec<u8>,
        /// The activation to follow with, if any.
        activate: Option<u8>,
    },
    /// Read one byte, change one bit of it, write it back, then activate.
    ///
    /// The read is not optional and cannot be skipped by assuming the byte's
    /// other bits: they belong to the neighbouring inputs, and their current
    /// state is not knowable from anything the host has already asked for.
    ModifyBit {
        /// The byte holding the bit.
        offset: u16,
        /// Which bit, counted from the least significant.
        bit: u8,
        /// What to set it to.
        enabled: bool,
        /// The activation to follow with, if any.
        activate: Option<u8>,
    },
    /// Put the index and the value in the scratch area, then activate.
    ///
    /// The index goes one byte above the value, and both are written before
    /// the activation. The setting's own address is never touched.
    Buffered {
        /// The scratch address the value goes to.
        value_offset: u16,
        /// The value.
        value: u8,
        /// The scratch address the index goes to, one above the value's.
        index_offset: u16,
        /// Which instance of the setting this is.
        index: u8,
        /// The activation that applies it.
        activate: u8,
    },
}

/// Errors in planning a write, all of them the caller asking for something the
/// model cannot do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PlanError {
    /// The setting is written through a scratch area the model does not have.
    #[error("this setting is written through a scratch area that this model does not have")]
    NoParamBuffer,

    /// The setting is written through the scratch area and has no activation.
    ///
    /// A scratch write with nothing to apply it changes nothing at all, so it
    /// is refused rather than sent and quietly ignored.
    #[error(
        "this setting is written through a scratch area but has no activation, so a write would do nothing"
    )]
    NoActivation,

    /// The value does not fit the setting.
    #[error("this setting holds {capacity} bytes and the value given is {given}")]
    ValueTooWide {
        /// How many bytes the setting holds.
        capacity: usize,
        /// How many were given.
        given: usize,
    },
}

/// How to write `value` to instance `index` of the setting `descriptor`
/// describes.
///
/// `param_buf_addr` is the model's scratch address, zero where it has none.
pub fn plan_write(
    descriptor: Descriptor,
    param_buf_addr: u16,
    index: u16,
    value: &[u8],
) -> Result<Plan, PlanError> {
    let activate = (descriptor.activate != 0).then_some(descriptor.activate);

    if descriptor.via_param_buf {
        if param_buf_addr == 0 {
            return Err(PlanError::NoParamBuffer);
        }
        let activate = activate.ok_or(PlanError::NoActivation)?;
        let [single] = value else {
            return Err(PlanError::ValueTooWide {
                capacity: 1,
                given: value.len(),
            });
        };
        return Ok(Plan::Buffered {
            value_offset: param_buf_addr,
            value: *single,
            index_offset: param_buf_addr.wrapping_add(1),
            index: u8::try_from(index).unwrap_or(u8::MAX),
            activate,
        });
    }

    if descriptor.is_whole_bytes() {
        let capacity = descriptor.byte_len();
        if value.len() != capacity {
            return Err(PlanError::ValueTooWide {
                capacity,
                given: value.len(),
            });
        }
        return Ok(Plan::Direct {
            offset: descriptor.address(index),
            value: value.to_vec(),
            activate,
        });
    }

    // Narrower than a byte, so the index selects the bit rather than the
    // address, and the byte's other bits belong to other inputs.
    let [single] = value else {
        return Err(PlanError::ValueTooWide {
            capacity: 1,
            given: value.len(),
        });
    };
    Ok(Plan::ModifyBit {
        offset: descriptor.offset,
        bit: u8::try_from(index).unwrap_or(u8::MAX),
        enabled: *single != 0,
        activate,
    })
}

/// `byte` with `bit` set to `enabled`, leaving every other bit alone.
///
/// The whole point of the read-modify-write, in one function so it can be
/// tested without a device: the neighbouring bits are other inputs' phantom
/// power, and clearing one of them is the failure this exists to prevent.
#[must_use]
pub fn apply_bit(byte: u8, bit: u8, enabled: bool) -> u8 {
    if bit >= u8::BITS.try_into().unwrap_or(u8::MAX) {
        // A bit index past the end of the byte is a caller error, and the
        // safest reading of it is "change nothing" — the alternative is a
        // shift that wraps and lands on somebody else's input.
        return byte;
    }
    let mask = 1u8 << bit;
    if enabled { byte | mask } else { byte & !mask }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor(size_bits: u8, activate: u8, via_param_buf: bool) -> Descriptor {
        Descriptor {
            offset: 0x009C,
            size_bits,
            activate,
            via_param_buf,
        }
    }

    #[test]
    fn a_read_asks_for_an_address_and_a_length() {
        assert_eq!(
            read_payload(0x009C, 1),
            vec![0x9C, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00]
        );
    }

    #[test]
    fn a_write_sends_a_value_as_wide_as_the_setting_and_no_wider() {
        // The encoding that looks right and is not. A one-byte setting sends
        // nine bytes, not twelve; padding it to a full u32 makes the device
        // reject the request rather than explain itself.
        let payload = write_payload(0x009C, &[0x01]);
        assert_eq!(payload.len(), 9);
        assert_eq!(
            payload,
            vec![0x9C, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01]
        );

        let wide = write_payload(0x0034, &[0x34, 0x12]);
        assert_eq!(
            wide.len(),
            10,
            "a sixteen-bit setting sends two value bytes"
        );
        assert_eq!(&wide[4..8], &[0x02, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn an_activation_is_one_little_endian_word() {
        assert_eq!(activate_payload(8), vec![0x08, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn a_whole_byte_setting_is_written_where_its_index_puts_it() {
        let plan = plan_write(descriptor(16, 1, false), 0, 2, &[0x34, 0x12]).expect("planned");
        assert_eq!(
            plan,
            Plan::Direct {
                offset: 0x009C + 4,
                value: vec![0x34, 0x12],
                activate: Some(1),
            }
        );
    }

    #[test]
    fn a_setting_with_no_activation_code_is_planned_without_one() {
        // Master volume is one: it takes effect as it is written.
        let plan = plan_write(descriptor(16, 0, false), 0, 0, &[0, 0]).expect("planned");
        assert!(matches!(plan, Plan::Direct { activate: None, .. }));
    }

    #[test]
    fn a_bit_sized_setting_becomes_a_read_modify_write_at_a_fixed_address() {
        // Phantom power. The index picks the bit, and the address does not
        // move with it — which is exactly why the byte has to be read first.
        let plan = plan_write(descriptor(1, 8, false), 0, 1, &[1]).expect("planned");
        assert_eq!(
            plan,
            Plan::ModifyBit {
                offset: 0x009C,
                bit: 1,
                enabled: true,
                activate: Some(8),
            }
        );
    }

    #[test]
    fn changing_one_bit_leaves_every_other_input_alone() {
        // The failure this whole path exists to prevent: writing the byte
        // outright would switch phantom power off on every other pair, with
        // nothing reported and the panel showing only what was asked for.
        assert_eq!(apply_bit(0b0000_0001, 1, true), 0b0000_0011);
        assert_eq!(apply_bit(0b0000_0011, 0, false), 0b0000_0010);
        assert_eq!(
            apply_bit(0b1111_1111, 3, false),
            0b1111_0111,
            "clearing one bit clears exactly one"
        );
        assert_eq!(
            apply_bit(0b0000_0010, 1, true),
            0b0000_0010,
            "setting a bit that is already set changes nothing"
        );
    }

    #[test]
    fn a_bit_index_past_the_end_of_the_byte_changes_nothing() {
        // The alternative is a shift that wraps and lands on another input's
        // phantom power, which is the worst possible way to be wrong here.
        assert_eq!(apply_bit(0b1010_1010, 8, true), 0b1010_1010);
        assert_eq!(apply_bit(0b1010_1010, 200, false), 0b1010_1010);
    }

    #[test]
    fn the_newer_families_write_the_scratch_area_rather_than_the_setting() {
        let plan = plan_write(descriptor(8, 4, true), 0x0130, 3, &[0x01]).expect("planned");
        assert_eq!(
            plan,
            Plan::Buffered {
                value_offset: 0x0130,
                value: 0x01,
                index_offset: 0x0131,
                index: 3,
                activate: 4,
            }
        );
    }

    #[test]
    fn a_scratch_write_with_nowhere_to_write_is_refused() {
        assert_eq!(
            plan_write(descriptor(8, 4, true), 0, 0, &[1]),
            Err(PlanError::NoParamBuffer)
        );
    }

    #[test]
    fn a_scratch_write_with_nothing_to_apply_it_is_refused() {
        // It would be accepted by the device and change nothing, which is
        // worse than an error: the value reads back as though it worked.
        assert_eq!(
            plan_write(descriptor(8, 0, true), 0x0130, 0, &[1]),
            Err(PlanError::NoActivation)
        );
    }

    #[test]
    fn a_value_that_does_not_fit_the_setting_is_refused() {
        // Truncating would write half a number; padding would overwrite the
        // setting next door. Neither is better than saying no.
        assert_eq!(
            plan_write(descriptor(16, 1, false), 0, 0, &[1]),
            Err(PlanError::ValueTooWide {
                capacity: 2,
                given: 1
            })
        );
        assert_eq!(
            plan_write(descriptor(8, 1, false), 0, 0, &[1, 2]),
            Err(PlanError::ValueTooWide {
                capacity: 1,
                given: 2
            })
        );
    }
}
