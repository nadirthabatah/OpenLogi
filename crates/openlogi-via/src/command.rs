//! VIA request framing and response parsing.
//!
//! Every exchange is the same shape: a 32-byte request whose first byte is the
//! command id, and a 32-byte response that echoes that id back. The echo is
//! load-bearing — it is the only thing distinguishing "the keyboard answered
//! my question" from "the keyboard sent an unrelated report that happened to
//! arrive first" — so every parser here checks it rather than trusting
//! position in the stream.

use thiserror::Error;

use crate::identity::REPORT_LEN;

/// Command ids, from QMK's `via_command_id`.
///
/// Only the ones this crate implements are named. The numbering is a firmware
/// contract rather than something we choose, so these are written as explicit
/// discriminants: an accidental reordering would silently repurpose every
/// command below the change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CommandId {
    /// Report the VIA protocol revision the firmware implements.
    GetProtocolVersion = 0x01,
    /// Read one keycode from the dynamic keymap.
    GetKeycode = 0x04,
    /// Write one keycode into the dynamic keymap.
    SetKeycode = 0x05,
    /// How many layers the dynamic keymap holds.
    GetLayerCount = 0x11,
}

/// Why a VIA exchange could not be completed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ProtocolError {
    /// The device answered with fewer bytes than a VIA report holds.
    #[error("the device answered with {got} bytes; a VIA report is {REPORT_LEN}")]
    ShortReport {
        /// How many bytes arrived.
        got: usize,
    },
    /// The answer echoed a different command than the one asked.
    ///
    /// Not necessarily a broken device: a keyboard can send an unrelated raw
    /// report, and reading the next one is the right response. It is a hard
    /// error here so that the *caller* decides to keep reading, rather than
    /// this layer silently accepting an answer to another question.
    #[error("asked for command {asked:#04x} and the device answered {answered:#04x}")]
    Mismatched {
        /// The command sent.
        asked: u8,
        /// The command the device echoed.
        answered: u8,
    },
    /// A layer index the keyboard does not have.
    #[error("layer {layer} does not exist; this keymap has {count}")]
    LayerOutOfRange {
        /// The layer asked for.
        layer: u8,
        /// How many the keyboard reports.
        count: u8,
    },
    /// A protocol revision this crate has not been written against.
    ///
    /// Refused rather than attempted. VIA's payload layouts have changed
    /// across revisions, and guessing at an unknown one means writing bytes of
    /// unknown meaning into a keyboard's keymap.
    #[error(
        "this keyboard speaks VIA protocol {found}, and this build implements \
         {}. Refusing rather than guessing at a layout that may have changed.",
        SUPPORTED_PROTOCOL
    )]
    UnsupportedProtocol {
        /// What the keyboard reported.
        found: u16,
    },
}

/// The VIA protocol revision this crate implements.
///
/// Revision 9 is what current QMK ships and what the reference implementation
/// targets. A device reporting anything else is refused rather than addressed
/// on the assumption its layouts match.
pub const SUPPORTED_PROTOCOL: u16 = 9;

/// One request to a VIA device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Ask which protocol revision the firmware speaks.
    GetProtocolVersion,
    /// Ask how many layers the dynamic keymap holds.
    GetLayerCount,
    /// Read the keycode at one position.
    GetKeycode {
        /// Keymap layer.
        layer: u8,
        /// Matrix row.
        row: u8,
        /// Matrix column.
        column: u8,
    },
    /// Write a keycode to one position.
    SetKeycode {
        /// Keymap layer.
        layer: u8,
        /// Matrix row.
        row: u8,
        /// Matrix column.
        column: u8,
        /// The QMK keycode to store.
        keycode: u16,
    },
}

impl Command {
    /// The command id this request carries.
    #[must_use]
    pub const fn id(self) -> CommandId {
        match self {
            Self::GetProtocolVersion => CommandId::GetProtocolVersion,
            Self::GetLayerCount => CommandId::GetLayerCount,
            Self::GetKeycode { .. } => CommandId::GetKeycode,
            Self::SetKeycode { .. } => CommandId::SetKeycode,
        }
    }

    /// The bytes to write to the raw endpoint.
    ///
    /// Always [`REPORT_LEN`] bytes: QMK reads a full report and a short write
    /// leaves the tail of the previous one in place, which is how a stray
    /// keycode ends up somewhere nobody asked for.
    #[must_use]
    pub const fn encode(self) -> [u8; REPORT_LEN] {
        let mut report = [0_u8; REPORT_LEN];
        report[0] = self.id() as u8;
        match self {
            Self::GetProtocolVersion | Self::GetLayerCount => {}
            Self::GetKeycode { layer, row, column } => {
                report[1] = layer;
                report[2] = row;
                report[3] = column;
            }
            Self::SetKeycode {
                layer,
                row,
                column,
                keycode,
            } => {
                report[1] = layer;
                report[2] = row;
                report[3] = column;
                // Big-endian, as every multi-byte VIA field is.
                report[4] = (keycode >> 8) as u8;
                report[5] = (keycode & 0xff) as u8;
            }
        }
        report
    }
}

/// What a device answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Response {
    /// The protocol revision the firmware speaks.
    ProtocolVersion(u16),
    /// How many layers the dynamic keymap holds.
    LayerCount(u8),
    /// The keycode read from, or confirmed at, one position.
    Keycode(u16),
}

impl Response {
    /// Parse a device's answer to `command`.
    ///
    /// # Errors
    ///
    /// [`ProtocolError::ShortReport`] when fewer than [`REPORT_LEN`] bytes
    /// arrived, and [`ProtocolError::Mismatched`] when the answer echoes a
    /// different command — which is a signal to read again, not to give up.
    pub fn parse(command: Command, report: &[u8]) -> Result<Self, ProtocolError> {
        if report.len() < REPORT_LEN {
            return Err(ProtocolError::ShortReport { got: report.len() });
        }
        let asked = command.id() as u8;
        if report[0] != asked {
            return Err(ProtocolError::Mismatched {
                asked,
                answered: report[0],
            });
        }
        Ok(match command {
            Command::GetProtocolVersion => {
                Self::ProtocolVersion(u16::from_be_bytes([report[1], report[2]]))
            }
            Command::GetLayerCount => Self::LayerCount(report[1]),
            // A read answers with the keycode after the three coordinates it
            // echoes; a write answers with the same shape, which is what lets
            // a caller confirm what actually landed rather than assume.
            Command::GetKeycode { .. } | Command::SetKeycode { .. } => {
                Self::Keycode(u16::from_be_bytes([report[4], report[5]]))
            }
        })
    }
}

/// Check a reported protocol revision before addressing a device.
///
/// # Errors
///
/// [`ProtocolError::UnsupportedProtocol`] for any revision this crate has not
/// been written against.
pub const fn check_protocol(found: u16) -> Result<(), ProtocolError> {
    if found == SUPPORTED_PROTOCOL {
        Ok(())
    } else {
        Err(ProtocolError::UnsupportedProtocol { found })
    }
}

#[cfg(test)]
mod tests {
    use super::{Command, CommandId, ProtocolError, Response, SUPPORTED_PROTOCOL, check_protocol};
    use crate::identity::REPORT_LEN;

    /// A device's answer, built the way firmware would build it.
    fn answer(id: CommandId, payload: &[(usize, u8)]) -> [u8; REPORT_LEN] {
        let mut report = [0_u8; REPORT_LEN];
        report[0] = id as u8;
        for &(index, value) in payload {
            report[index] = value;
        }
        report
    }

    #[test]
    fn a_request_is_always_a_full_report() {
        assert_eq!(Command::GetProtocolVersion.encode().len(), REPORT_LEN);
        assert_eq!(
            Command::SetKeycode {
                layer: 0,
                row: 1,
                column: 2,
                keycode: 0x0004,
            }
            .encode()
            .len(),
            REPORT_LEN
        );
    }

    #[test]
    fn a_read_carries_its_coordinates_in_order() {
        let report = Command::GetKeycode {
            layer: 1,
            row: 2,
            column: 3,
        }
        .encode();
        assert_eq!(report[0], 0x04);
        assert_eq!(&report[1..4], &[1, 2, 3]);
        assert!(
            report[4..].iter().all(|byte| *byte == 0),
            "tail must be clear"
        );
    }

    /// Byte order is the classic silent corruption here: a keycode written
    /// little-endian lands as a different, perfectly valid key.
    #[test]
    fn a_keycode_is_written_big_endian() {
        let report = Command::SetKeycode {
            layer: 0,
            row: 0,
            column: 0,
            keycode: 0x1234,
        }
        .encode();
        assert_eq!(report[4], 0x12);
        assert_eq!(report[5], 0x34);
    }

    #[test]
    fn a_keycode_survives_a_round_trip() {
        for keycode in [0x0000, 0x0004, 0x00e0, 0x1234, 0xffff_u16] {
            let command = Command::SetKeycode {
                layer: 0,
                row: 0,
                column: 0,
                keycode,
            };
            let sent = command.encode();
            let echoed = answer(CommandId::SetKeycode, &[(4, sent[4]), (5, sent[5])]);
            assert_eq!(
                Response::parse(command, &echoed),
                Ok(Response::Keycode(keycode)),
                "{keycode:#06x} did not survive"
            );
        }
    }

    #[test]
    fn the_protocol_version_is_read_big_endian() {
        let report = answer(CommandId::GetProtocolVersion, &[(1, 0x00), (2, 0x09)]);
        assert_eq!(
            Response::parse(Command::GetProtocolVersion, &report),
            Ok(Response::ProtocolVersion(9))
        );
    }

    #[test]
    fn a_layer_count_is_one_byte() {
        let report = answer(CommandId::GetLayerCount, &[(1, 4)]);
        assert_eq!(
            Response::parse(Command::GetLayerCount, &report),
            Ok(Response::LayerCount(4))
        );
    }

    /// The check that makes reads trustworthy. A keyboard can send an
    /// unrelated raw report at any moment; accepting it positionally would
    /// hand back another command's payload as a keycode.
    #[test]
    fn an_answer_to_another_command_is_refused_rather_than_read_as_this_one() {
        let report = answer(CommandId::GetLayerCount, &[(1, 4)]);
        assert_eq!(
            Response::parse(Command::GetProtocolVersion, &report),
            Err(ProtocolError::Mismatched {
                asked: 0x01,
                answered: 0x11,
            })
        );
    }

    #[test]
    fn a_truncated_answer_is_an_error_not_a_zero_keycode() {
        let short = [0x04_u8, 0, 0, 0];
        assert_eq!(
            Response::parse(
                Command::GetKeycode {
                    layer: 0,
                    row: 0,
                    column: 0,
                },
                &short,
            ),
            Err(ProtocolError::ShortReport { got: 4 })
        );
    }

    /// Refusing an unknown revision is the whole safety story for writes: VIA
    /// payload layouts have changed between revisions, and a misread layout
    /// means writing bytes of unknown meaning into someone's keymap.
    #[test]
    fn an_unknown_protocol_revision_is_refused_rather_than_guessed_at() {
        assert_eq!(check_protocol(SUPPORTED_PROTOCOL), Ok(()));
        assert_eq!(
            check_protocol(11),
            Err(ProtocolError::UnsupportedProtocol { found: 11 })
        );
        assert_eq!(
            check_protocol(0),
            Err(ProtocolError::UnsupportedProtocol { found: 0 })
        );
    }

    /// The command numbers are a firmware contract, not ours to renumber.
    #[test]
    fn the_command_ids_match_the_firmware_contract() {
        assert_eq!(CommandId::GetProtocolVersion as u8, 0x01);
        assert_eq!(CommandId::GetKeycode as u8, 0x04);
        assert_eq!(CommandId::SetKeycode as u8, 0x05);
        assert_eq!(CommandId::GetLayerCount as u8, 0x11);
    }

    #[test]
    fn the_error_text_says_what_to_do_about_it() {
        let text = check_protocol(11).expect_err("refused").to_string();
        assert!(text.contains("11"), "{text}");
        assert!(text.contains("Refusing"), "{text}");
    }
}
