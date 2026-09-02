//! The one-byte event encoding.
//!
//! A TourBox sends one byte per event and nothing else: no framing, no
//! length, no checksum, no sequence number. The low six bits name the
//! control and the high two bits say what happened to it.
//!
//! The two halves are not independent, and that is what the types here
//! encode. A button is pressed or released and can never turn; a wheel turns
//! one way or the other and is never released. Splitting them means the
//! impossible combinations cannot be built, so the only place they have to
//! be rejected is [`decode`], where the bytes come in.

use crate::ProtocolError;

/// The low six bits: which control the byte is about.
pub const CONTROL_MASK: u8 = 0x3f;

/// The high two bits: what happened to that control.
pub const ACTION_MASK: u8 = 0xc0;

/// A button went down.
const PRESSED: u8 = 0x00;
/// A button came back up.
const RELEASED: u8 = 0x80;
/// A wheel turned towards the user, or left, or down.
const COUNTER_CLOCKWISE: u8 = 0x00;
/// A wheel turned away from the user, or right, or up.
const CLOCKWISE: u8 = 0x40;
/// A wheel came to rest. The same bit that means "released" on a button,
/// which is consistent rather than coincidental: both mark the end of
/// something that was being held.
const TURN_ENDED: u8 = 0x80;

/// A button on a TourBox.
///
/// Named for where the hand finds it rather than for what it does, because
/// what it does is whatever it has been mapped to. [`Button::Knob`],
/// [`Button::Scroll`] and [`Button::Dial`] are the wheels pressed inward,
/// which is a different control from the same wheel turned and carries its
/// own code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Button {
    /// The tall button on the top face.
    Tall,
    /// The button on the near side, under the thumb.
    Side,
    /// The button on the top edge.
    Top,
    /// The short button on the top face.
    Short,
    /// The scroll wheel pressed inward.
    Scroll,
    /// The up arrow of the four-way pad.
    Up,
    /// The down arrow of the four-way pad.
    Down,
    /// The left arrow of the four-way pad.
    Left,
    /// The right arrow of the four-way pad.
    Right,
    /// The first custom button.
    C1,
    /// The second custom button.
    C2,
    /// The centre button, marked with the TourBox logo.
    Tour,
    /// The large knob pressed inward.
    Knob,
    /// The flat dial pressed inward.
    Dial,
}

impl Button {
    /// The six-bit control code this button reports.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Tall => 0x00,
            Self::Side => 0x01,
            Self::Top => 0x02,
            Self::Short => 0x03,
            Self::Scroll => 0x0a,
            Self::Up => 0x10,
            Self::Down => 0x11,
            Self::Left => 0x12,
            Self::Right => 0x13,
            Self::C1 => 0x22,
            Self::C2 => 0x23,
            Self::Tour => 0x2a,
            Self::Knob => 0x37,
            Self::Dial => 0x38,
        }
    }

    /// What to call this button out loud.
    ///
    /// The noun only, with no verb in it. [`Event::describe`] supplies the
    /// verb, and a name that already contained one would produce "scroll
    /// wheel press pressed" — which is what this said before anybody read it
    /// aloud. The three wheels are named here exactly as they are in
    /// [`Wheel::name`] for the same reason: "knob pressed" and "knob turned
    /// clockwise" are already distinct, so spelling the difference into the
    /// noun as well would say it twice.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Tall => "tall button",
            Self::Side => "side button",
            Self::Top => "top button",
            Self::Short => "short button",
            Self::Scroll => "scroll wheel",
            Self::Up => "up button",
            Self::Down => "down button",
            Self::Left => "left button",
            Self::Right => "right button",
            Self::C1 => "C1 button",
            Self::C2 => "C2 button",
            Self::Tour => "tour button",
            Self::Knob => "knob",
            Self::Dial => "dial",
        }
    }

    /// This control named as an action, for an error message.
    ///
    /// "Pressing the knob" rather than "the knob", because the knob is also
    /// a wheel: saying a byte "names the knob but claims a turn" would be
    /// nonsense, since turning the knob is a real thing. What cannot happen
    /// is *pressing* it and turning it in one byte.
    #[must_use]
    pub const fn pressing(self) -> &'static str {
        match self {
            Self::Tall => "pressing the tall button",
            Self::Side => "pressing the side button",
            Self::Top => "pressing the top button",
            Self::Short => "pressing the short button",
            Self::Scroll => "pressing the scroll wheel",
            Self::Up => "pressing the up button",
            Self::Down => "pressing the down button",
            Self::Left => "pressing the left button",
            Self::Right => "pressing the right button",
            Self::C1 => "pressing the C1 button",
            Self::C2 => "pressing the C2 button",
            Self::Tour => "pressing the tour button",
            Self::Knob => "pressing the knob",
            Self::Dial => "pressing the dial",
        }
    }

    /// The button a six-bit control code names, if it names one.
    #[must_use]
    pub fn from_code(code: u8) -> Option<Self> {
        const ALL: &[Button] = &[
            Button::Tall,
            Button::Side,
            Button::Top,
            Button::Short,
            Button::Scroll,
            Button::Up,
            Button::Down,
            Button::Left,
            Button::Right,
            Button::C1,
            Button::C2,
            Button::Tour,
            Button::Knob,
            Button::Dial,
        ];
        ALL.iter().copied().find(|button| button.code() == code)
    }
}

/// A wheel, knob or dial on a TourBox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Wheel {
    /// The large knob.
    Knob,
    /// The scroll wheel.
    Scroll,
    /// The flat dial.
    Dial,
}

impl Wheel {
    /// The six-bit control code this wheel reports when it turns.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Knob => 0x04,
            Self::Scroll => 0x09,
            Self::Dial => 0x0f,
        }
    }

    /// What to call this wheel out loud.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Knob => "knob",
            Self::Scroll => "scroll wheel",
            Self::Dial => "dial",
        }
    }

    /// This control named as an action, for an error message. See
    /// [`Button::pressing`] for why the verb is part of it.
    #[must_use]
    pub const fn turning(self) -> &'static str {
        match self {
            Self::Knob => "turning the knob",
            Self::Scroll => "turning the scroll wheel",
            Self::Dial => "turning the dial",
        }
    }

    /// The wheel a six-bit control code names, if it names one.
    #[must_use]
    pub fn from_code(code: u8) -> Option<Self> {
        const ALL: &[Wheel] = &[Wheel::Knob, Wheel::Scroll, Wheel::Dial];
        ALL.iter().copied().find(|wheel| wheel.code() == code)
    }
}

/// What happened to a button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ButtonAction {
    /// The button went down.
    Pressed,
    /// The button came back up.
    Released,
}

impl ButtonAction {
    /// What to call this out loud.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Pressed => "pressed",
            Self::Released => "released",
        }
    }
}

/// Which way a wheel turned.
///
/// One detent. A TourBox reports every step separately rather than
/// accumulating them, so a fast turn is many of these and not one large one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Turn {
    /// Away from the user: right, or up.
    Clockwise,
    /// Towards the user: left, or down.
    CounterClockwise,
}

impl Turn {
    /// What to call this out loud.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Clockwise => "clockwise",
            Self::CounterClockwise => "counter-clockwise",
        }
    }
}

/// Whether a wheel is still moving or has just come to rest.
///
/// A TourBox marks the end of a turn with its own byte, carrying the same
/// control code and direction as the detents that preceded it and the high
/// bit set — the same bit that means "released" on a button. So a turn is a
/// run of [`TurnPhase::Moving`] events followed by exactly one
/// [`TurnPhase::Ended`].
///
/// This build rejected those end markers as impossible until three
/// independent drivers were compared: one names them `KNOB_LEFT_STOP` and
/// its siblings, and another prints one from live hardware. Without this,
/// every turn of every wheel would have produced a spurious error at the
/// moment the hand stopped.
///
/// Most callers want [`TurnPhase::Moving`] and can ignore the other; it is
/// reported rather than swallowed because it is the only signal that a
/// continuous gesture is over, which is what a caller holding a modifier
/// down for the duration of a turn needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TurnPhase {
    /// One detent of an ongoing turn.
    Moving,
    /// The wheel has stopped. Sent once, after the last detent.
    Ended,
}

/// One thing that happened on a TourBox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Event {
    /// A button was pressed or released.
    Button {
        /// Which button.
        button: Button,
        /// Whether it went down or came up.
        action: ButtonAction,
    },
    /// A wheel moved one detent, or finished moving.
    Turn {
        /// Which wheel.
        wheel: Wheel,
        /// Which way it went.
        direction: Turn,
        /// Whether this is a detent or the end of the turn.
        phase: TurnPhase,
    },
}

impl Event {
    /// The byte a TourBox sends for this event.
    ///
    /// The inverse of [`decode`]. Present so the encoding can be checked
    /// from both ends, and so a test can assert no two events share a byte.
    #[must_use]
    pub const fn byte(self) -> u8 {
        match self {
            Self::Button { button, action } => {
                let action = match action {
                    ButtonAction::Pressed => PRESSED,
                    ButtonAction::Released => RELEASED,
                };
                button.code() | action
            }
            Self::Turn {
                wheel,
                direction,
                phase,
            } => {
                let direction = match direction {
                    Turn::Clockwise => CLOCKWISE,
                    Turn::CounterClockwise => COUNTER_CLOCKWISE,
                };
                let phase = match phase {
                    TurnPhase::Moving => 0x00,
                    TurnPhase::Ended => TURN_ENDED,
                };
                wheel.code() | direction | phase
            }
        }
    }

    /// A sentence naming what happened, for a screen reader.
    #[must_use]
    pub fn describe(self) -> String {
        match self {
            Self::Button { button, action } => {
                format!("{} {}", button.name(), action.name())
            }
            Self::Turn {
                wheel,
                direction,
                phase,
            } => match phase {
                TurnPhase::Moving => format!("{} turned {}", wheel.name(), direction.name()),
                TurnPhase::Ended => {
                    format!("{} stopped turning {}", wheel.name(), direction.name())
                }
            },
        }
    }
}

/// Turn one byte from a TourBox into the event it reports.
///
/// # Errors
///
/// [`ProtocolError::UnknownControl`] when the low six bits name no control
/// this build knows, and [`ProtocolError::ImpossibleAction`] when they name
/// one that cannot do what the high bits claim — a button that turned, or a
/// wheel that was released. Neither is guessed at: the protocol has no
/// checksum, so a byte that does not make sense is as likely to be corrupt
/// as to be a control from a model this build has not met, and delivering a
/// nearby keystroke would be worse than delivering none.
pub fn decode(byte: u8) -> Result<Event, ProtocolError> {
    let control = byte & CONTROL_MASK;
    let action = byte & ACTION_MASK;

    if let Some(wheel) = Wheel::from_code(control) {
        // All four combinations are real here, unlike for a button. The
        // direction bit and the ended bit are independent, so a wheel has
        // no impossible action to reject.
        let direction = if action & CLOCKWISE == 0 {
            Turn::CounterClockwise
        } else {
            Turn::Clockwise
        };
        let phase = if action & TURN_ENDED == 0 {
            TurnPhase::Moving
        } else {
            TurnPhase::Ended
        };
        return Ok(Event::Turn {
            wheel,
            direction,
            phase,
        });
    }

    if let Some(button) = Button::from_code(control) {
        let action = match action {
            PRESSED => ButtonAction::Pressed,
            RELEASED => ButtonAction::Released,
            _ => {
                return Err(ProtocolError::ImpossibleAction {
                    control: button.pressing(),
                    action: "a turn",
                    byte,
                });
            }
        };
        return Ok(Event::Button { button, action });
    }

    Err(ProtocolError::UnknownControl { control, byte })
}

/// The 8-byte unlock command an Elite requires before it streams anything.
///
/// This is the handshake this crate spent three sessions believing did not
/// exist. The claim that a TourBox "streams whether or not anything has
/// talked to it" was transcribed from NEO drivers, and the Elite disproved
/// it on this project's own hardware: with the port open, the modem lines
/// raised and [`SETUP_MESSAGE`] sent, every button press still produced
/// silence — until this command went first.
///
/// Two independent drivers carry exactly these bytes: jasonrohrer's Elite
/// driver sends them as its libusb init message, and tuxbox sends them as
/// its unlock command, recovered separately from a Windows Bluetooth
/// capture — the same unlock serves both transports. On 2026-09-02 an
/// Elite on this project's desk answered them over USB serial with a
/// 26-byte reply beginning `0x07`, the first bytes that device ever sent
/// this codebase.
pub const UNLOCK_MESSAGE: [u8; 8] = [0x55, 0x00, 0x07, 0x88, 0x94, 0x00, 0x1a, 0xfe];

/// The 94-byte message that configures haptics, with every control set to
/// no feedback.
///
/// Sent once, after [`UNLOCK_MESSAGE`]. The trailing `0xfe` terminates it
/// and the leading `0xb5` identifies it. tuxbox sends these same 94 bytes
/// as five consecutive writes; jasonrohrer's driver sends them as one, and
/// one write is what this crate does.
pub const SETUP_MESSAGE: [u8; 94] = [
    0xb5, 0x00, 0x5d, 0x04, 0x00, 0x05, 0x00, 0x06, 0x00, 0x07, 0x00, 0x08, 0x00, 0x09, 0x00, 0x0b,
    0x00, 0x0c, 0x00, 0x0d, 0x00, 0x0e, 0x00, 0x0f, 0x00, 0x26, 0x00, 0x27, 0x00, 0x28, 0x00, 0x29,
    0x00, 0x3b, 0x00, 0x3c, 0x00, 0x3d, 0x00, 0x3e, 0x00, 0x3f, 0x00, 0x40, 0x00, 0x41, 0x00, 0x42,
    0x00, 0x43, 0x00, 0x44, 0x00, 0x45, 0x00, 0x46, 0x00, 0x47, 0x00, 0x48, 0x00, 0x49, 0x00, 0x4a,
    0x00, 0x4b, 0x00, 0x4c, 0x00, 0x4d, 0x00, 0x4e, 0x00, 0x4f, 0x00, 0x50, 0x00, 0x51, 0x00, 0x52,
    0x00, 0x53, 0x00, 0x54, 0x00, 0xa8, 0x00, 0xa9, 0x00, 0xaa, 0x00, 0xab, 0x00, 0xfe,
];

#[cfg(test)]
mod tests;
