//! The two writes that are not free, and the refusal that guards them.
//!
//! Almost everything a monitor accepts is reversible by writing it again:
//! brightness, contrast, colour gains, volume, the input. Two things are not,
//! and both of them are worse for someone who cannot see the screen they have
//! just changed.
//!
//! So the guard lives here rather than in the CLI. A refusal implemented in
//! one front end is a refusal the MCP server does not have, and the MCP server
//! is precisely where a write arrives without a person having typed it.

use roadie_ddc::vcp::PowerMode;
use roadie_ddc::{Feature, Value};

/// A write that cannot be undone from the keyboard.
///
/// Carried by [`crate::DisplayError::Refused`], and the only thing
/// [`crate::Display::set_acknowledging`] will accept as a reason to go ahead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Risk {
    /// Powering the monitor off, feature `0xD6` value `0x05`.
    ///
    /// A monitor in that state may stop answering DDC altogether, so nothing
    /// on the machine can turn it back on. The way back is the button on the
    /// bezel.
    PowerOff,

    /// A power value this crate does not recognise.
    ///
    /// Treated as unrecoverable on purpose. An unknown power state is not a
    /// good thing to be optimistic about, and the standard values are few
    /// enough that a number outside them is vendor territory.
    UnknownPowerState(u8),

    /// Committing the current settings to the monitor's own memory.
    ///
    /// That memory has a finite number of erase cycles. A plain write already
    /// survives until the monitor loses power, so saving is for the moment
    /// someone means "keep this", never something to append to every nudge.
    SaveSettings,
}

impl Risk {
    /// The risk in writing `value` to `feature`, or `None` if there is none.
    ///
    /// Only power carries one. Every other feature in MCCS can be written
    /// back, and treating more of them as dangerous would train a person to
    /// pass the confirmation without reading it — which is how a real
    /// confirmation stops working.
    #[must_use]
    pub fn of(feature: Feature, value: u16) -> Option<Self> {
        if feature != Feature::PowerMode {
            return None;
        }
        // A power value wider than a byte is not a power value at all. It
        // cannot be recognised, so it is treated like any other unrecognised
        // one rather than waved through for being out of range.
        let Ok(code) = u8::try_from(value) else {
            return Some(Self::UnknownPowerState(0));
        };
        match PowerMode::from_code(code) {
            mode if mode.is_recoverable() => None,
            PowerMode::Off => Some(Self::PowerOff),
            _ => Some(Self::UnknownPowerState(code)),
        }
    }

    /// What to tell someone before they decide, written to be read aloud.
    ///
    /// One sentence, no jargon, and it names the physical thing they will have
    /// to go and find — which is the part that actually matters when the
    /// screen has just gone dark.
    #[must_use]
    pub const fn spoken(self) -> &'static str {
        match self {
            Self::PowerOff => {
                "Powering the monitor off may stop it answering the computer entirely. \
                 The only way to turn it back on would be the power button on the monitor itself."
            }
            Self::UnknownPowerState(_) => {
                "That is not a power setting this build recognises, so there is no telling \
                 whether the monitor will still answer the computer afterwards. The way back \
                 may be the power button on the monitor itself."
            }
            Self::SaveSettings => {
                "Saving writes to the monitor's own memory, which can only be rewritten a \
                 limited number of times. Ordinary changes already last until the monitor \
                 loses power, so this is only worth doing to make a setting permanent."
            }
        }
    }
}

impl std::fmt::Display for Risk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.spoken())
    }
}

/// Proof that whoever asked for a risky write was told what it costs.
///
/// There is deliberately no `Default`, no `From<bool>`, and no way to build one
/// from a flag alone: [`Acknowledged::of`] takes the [`Risk`] itself, so the
/// call site has to have the risk in hand, which means it has had the sentence
/// in hand. A `--yes` flag several functions away cannot conjure one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Acknowledged(Risk);

impl Acknowledged {
    /// Record that this specific risk was put to someone and accepted.
    #[must_use]
    pub const fn of(risk: Risk) -> Self {
        Self(risk)
    }

    /// The risk that was accepted.
    ///
    /// Checked against the risk the write actually carries, so an
    /// acknowledgement of one thing cannot be spent on another: confirming a
    /// save does not authorise powering the panel off.
    #[must_use]
    pub const fn risk(self) -> Risk {
        self.0
    }
}

/// What writing `percent` of `value`'s range asks for, in the monitor's units.
///
/// Lives here rather than in the CLI because both front ends need it and both
/// would otherwise round it themselves.
#[must_use]
pub fn scaled(percent: u8, value: Value) -> u16 {
    Value::from_percent(percent, value.maximum)
}

#[cfg(test)]
mod tests;
