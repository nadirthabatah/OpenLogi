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
    /// The answer echoed a different position than the one asked about.
    ///
    /// Like [`Self::Mismatched`], not necessarily a broken device: a reply to
    /// an earlier request can still be in the pipe. It matters far more than
    /// it looks, because a keymap read walks the matrix in a tight loop and
    /// every reply carries the same command byte — so one stale or duplicated
    /// report would shift every answer after it by one position, and report a
    /// whole keymap that is confidently wrong.
    #[error(
        "asked about layer {asked_layer}, row {asked_row}, column {asked_column} and \
         the device answered about layer {answered_layer}, row {answered_row}, \
         column {answered_column}"
    )]
    WrongPosition {
        /// Layer asked about.
        asked_layer: u8,
        /// Row asked about.
        asked_row: u8,
        /// Column asked about.
        asked_column: u8,
        /// Layer the device answered about.
        answered_layer: u8,
        /// Row the device answered about.
        answered_row: u8,
        /// Column the device answered about.
        answered_column: u8,
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
         9 and 12. Refusing rather than guessing at a layout that may have \
         changed."
    )]
    UnsupportedProtocol {
        /// What the keyboard reported.
        found: u16,
    },
}

/// The VIA protocol revisions this crate implements.
///
/// Revision 9 is what pre-2022 QMK ships; revision 12 is what QMK has shipped
/// since its 0.19 keycode refactor and still ships today. Both are safe for
/// exactly the commands this crate sends: the four ids used here — protocol
/// version, layer count, and the keycode read and write — kept their numbers
/// and payload shapes through every revision between 9 and 12. What changed
/// in between was the lighting commands (replaced by custom-value channels,
/// neither of which this crate speaks) and the numbering of QMK's quantum
/// keycodes — and the keycode table here deliberately names only the basic
/// HID-standard codes, which are identical in both eras, so no name in this
/// build can mean a different key on one revision than the other.
///
/// The transitional revisions 10 and 11 are still refused: they shipped
/// briefly, no board on this project's desks has ever reported one, and
/// accepting a revision nothing can verify is guessing with extra steps.
pub const SUPPORTED_PROTOCOLS: &[u16] = &[9, 12];

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
            //
            // The echoed coordinates are checked, not skipped over. Every
            // keycode reply carries the same command byte, so the command
            // check above cannot tell this position's answer from the previous
            // one's — and a keymap read walks the matrix in a tight loop,
            // where one stale report would shift every answer after it.
            Command::GetKeycode { layer, row, column }
            | Command::SetKeycode {
                layer, row, column, ..
            } => {
                if [report[1], report[2], report[3]] != [layer, row, column] {
                    return Err(ProtocolError::WrongPosition {
                        asked_layer: layer,
                        asked_row: row,
                        asked_column: column,
                        answered_layer: report[1],
                        answered_row: report[2],
                        answered_column: report[3],
                    });
                }
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
    // A `while` rather than `contains`, which is not callable in const fn.
    let mut index = 0;
    while index < SUPPORTED_PROTOCOLS.len() {
        if SUPPORTED_PROTOCOLS[index] == found {
            return Ok(());
        }
        index += 1;
    }
    Err(ProtocolError::UnsupportedProtocol { found })
}

#[cfg(test)]
mod tests {
    use super::{Command, CommandId, ProtocolError, Response, SUPPORTED_PROTOCOLS, check_protocol};
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

    /// The check the command byte alone cannot make.
    ///
    /// Every keycode reply carries command 0x04, so a reply about another
    /// position is indistinguishable from this one's by command id. A keymap
    /// read walks the matrix in a tight loop; one stale report accepted here
    /// shifts every answer after it and reports a keymap that is confidently
    /// wrong.
    #[test]
    fn a_keycode_reply_about_another_position_is_refused() {
        let asked = Command::GetKeycode {
            layer: 1,
            row: 2,
            column: 3,
        };
        // The board answering about the position read just before this one.
        let stale = answer(
            CommandId::GetKeycode,
            &[(1, 1), (2, 2), (3, 2), (4, 0x00), (5, 0x04)],
        );
        assert_eq!(
            Response::parse(asked, &stale),
            Err(ProtocolError::WrongPosition {
                asked_layer: 1,
                asked_row: 2,
                asked_column: 3,
                answered_layer: 1,
                answered_row: 2,
                answered_column: 2,
            })
        );
    }

    /// The same check on a write, where accepting another position's echo
    /// would confirm a keycode landed somewhere it did not.
    #[test]
    fn a_write_confirmed_by_another_positions_echo_is_refused() {
        let asked = Command::SetKeycode {
            layer: 0,
            row: 4,
            column: 5,
            keycode: 0x0068,
        };
        let elsewhere = answer(
            CommandId::SetKeycode,
            &[(1, 0), (2, 4), (3, 6), (4, 0x00), (5, 0x68)],
        );
        assert!(matches!(
            Response::parse(asked, &elsewhere),
            Err(ProtocolError::WrongPosition { .. })
        ));
    }

    /// And the position it did ask about is still accepted, at coordinates
    /// that are not all zero — which is what the round-trip tests above use,
    /// and why they could not have caught the bug this pins.
    #[test]
    fn the_right_position_is_accepted_at_non_zero_coordinates() {
        let asked = Command::GetKeycode {
            layer: 2,
            row: 3,
            column: 7,
        };
        let reply = answer(
            CommandId::GetKeycode,
            &[(1, 2), (2, 3), (3, 7), (4, 0x00), (5, 0x68)],
        );
        assert_eq!(
            Response::parse(asked, &reply),
            Ok(Response::Keycode(0x0068))
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
    ///
    /// The boundary is exact on purpose: 9 and 12 are implemented, and the
    /// transitional 10 and 11 sit *between* them and are still refused — a
    /// membership check, not a range check, is what this pins.
    #[test]
    fn an_unknown_protocol_revision_is_refused_rather_than_guessed_at() {
        for &supported in SUPPORTED_PROTOCOLS {
            assert_eq!(check_protocol(supported), Ok(()), "protocol {supported}");
        }
        assert_eq!(check_protocol(9), Ok(()));
        assert_eq!(check_protocol(12), Ok(()));
        for refused in [0, 8, 10, 11, 13, u16::MAX] {
            assert_eq!(
                check_protocol(refused),
                Err(ProtocolError::UnsupportedProtocol { found: refused }),
                "protocol {refused} must be refused"
            );
        }
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

    /// Arbitrary bytes from a board must never panic the parser, and must
    /// never be read as an answer to a question they do not answer.
    ///
    /// A QMK board is the one thing this crate cannot get its hands on, so the
    /// next best assurance is to assume it sends anything at all and require
    /// a value or an error — never a crash, and never a keycode taken from a
    /// report that was about something else. A panic here kills the process
    /// mid-keymap-read; a wrong accept writes someone's keyboard from noise.
    ///
    /// Deterministic, so a failure names bytes that reproduce it.
    #[test]
    fn no_report_of_any_shape_panics_or_is_misread() {
        let mut state = 0x853C_49E6_748F_EA9B_u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };

        let commands = [
            Command::GetProtocolVersion,
            Command::GetLayerCount,
            Command::GetKeycode {
                layer: 1,
                row: 2,
                column: 3,
            },
            Command::SetKeycode {
                layer: 1,
                row: 2,
                column: 3,
                keycode: 0x0068,
            },
        ];

        for command in commands {
            for _ in 0..20_000 {
                let len = (next() % 80) as usize;
                let mut report = vec![0_u8; len];
                for byte in &mut report {
                    *byte = (next() & 0xff) as u8;
                }
                match Response::parse(command, &report) {
                    Err(_) => {}
                    Ok(response) => {
                        // An accepted report must have echoed this command...
                        assert_eq!(
                            report[0],
                            command.id() as u8,
                            "accepted a report for another command: {report:02x?}"
                        );
                        // ...and, for the positional commands, this position.
                        if let Command::GetKeycode { layer, row, column }
                        | Command::SetKeycode {
                            layer, row, column, ..
                        } = command
                        {
                            assert_eq!(
                                [report[1], report[2], report[3]],
                                [layer, row, column],
                                "accepted a report about another key: {report:02x?}"
                            );
                            assert!(
                                matches!(response, Response::Keycode(_)),
                                "a keycode command answered as {response:?}"
                            );
                        }
                    }
                }
            }
        }
    }
}
