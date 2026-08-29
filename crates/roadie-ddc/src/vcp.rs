//! VCP features: the numbered knobs behind a monitor's on-screen menu.
//!
//! MCCS gives every adjustment a one-byte code, and the useful ones are
//! remarkably consistent across brands — `0x10` is brightness on a Dell, an
//! LG, and a Gigabyte alike. That consistency is the whole reason a
//! vendor-neutral monitor control is possible at all.
//!
//! It stops at `0xE0`. Everything from there up is vendor-defined, and two
//! monitors can use the same code for unrelated things. [`Feature::Other`]
//! carries those through by number without naming them, because a wrong name
//! is worse than no name.
//!
//! # Codes are a claim, not a guarantee
//!
//! A monitor is free to implement a feature badly, report a maximum it does
//! not honour, or list a feature in its capability string and then refuse to
//! set it. The capability string ([`crate::capabilities`]) is the better
//! source for what a specific panel can do; this module is what the numbers
//! *mean* once you have them.

/// A VCP feature code.
///
/// The named variants are the ones worth a menu entry and consistent enough
/// across vendors to rely on. Everything else round-trips through
/// [`Self::Other`] unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Feature {
    /// Brightness, called luminance in the specification. Continuous, and the
    /// single most-wanted control on the list.
    Brightness,
    /// Contrast. Continuous.
    Contrast,
    /// Colour preset — warm, cool, sRGB, and so on. The values are discrete
    /// and vary by monitor, so read the capability string before offering them.
    ColorPreset,
    /// Red channel gain. Continuous.
    RedGain,
    /// Green channel gain. Continuous.
    GreenGain,
    /// Blue channel gain. Continuous.
    BlueGain,
    /// Which physical input the monitor displays. See [`InputSource`].
    InputSource,
    /// Speaker volume, on monitors that have speakers. Continuous.
    Volume,
    /// Audio mute. `0x01` mutes, `0x02` unmutes.
    Mute,
    /// On-screen-display language.
    OsdLanguage,
    /// Power state. See [`PowerMode`], and read its warning before writing it.
    PowerMode,
    /// The MCCS version the monitor claims. Read-only, and a good first probe:
    /// a monitor that answers this at all is a monitor that speaks DDC.
    McssVersion,
    /// Anything this crate does not name, including the whole vendor-defined
    /// range from `0xE0` up.
    Other(u8),
}

impl Feature {
    /// The one-byte code that goes on the wire.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Brightness => 0x10,
            Self::Contrast => 0x12,
            Self::ColorPreset => 0x14,
            Self::RedGain => 0x16,
            Self::GreenGain => 0x18,
            Self::BlueGain => 0x1A,
            Self::InputSource => 0x60,
            Self::Volume => 0x62,
            Self::Mute => 0x8D,
            Self::OsdLanguage => 0xCC,
            Self::PowerMode => 0xD6,
            Self::McssVersion => 0xDF,
            Self::Other(code) => code,
        }
    }

    /// The feature a code names.
    #[must_use]
    pub const fn from_code(code: u8) -> Self {
        match code {
            0x10 => Self::Brightness,
            0x12 => Self::Contrast,
            0x14 => Self::ColorPreset,
            0x16 => Self::RedGain,
            0x18 => Self::GreenGain,
            0x1A => Self::BlueGain,
            0x60 => Self::InputSource,
            0x62 => Self::Volume,
            0x8D => Self::Mute,
            0xCC => Self::OsdLanguage,
            0xD6 => Self::PowerMode,
            0xDF => Self::McssVersion,
            other => Self::Other(other),
        }
    }

    /// The name to speak or print.
    ///
    /// Unnamed codes get `None` rather than a placeholder string, so a caller
    /// can decide how to say "feature 0xE2" in its own voice instead of
    /// inheriting one from here.
    #[must_use]
    pub const fn name(self) -> Option<&'static str> {
        Some(match self {
            Self::Brightness => "brightness",
            Self::Contrast => "contrast",
            Self::ColorPreset => "colour preset",
            Self::RedGain => "red gain",
            Self::GreenGain => "green gain",
            Self::BlueGain => "blue gain",
            Self::InputSource => "input source",
            Self::Volume => "volume",
            Self::Mute => "mute",
            Self::OsdLanguage => "on-screen display language",
            Self::PowerMode => "power mode",
            Self::McssVersion => "MCCS version",
            Self::Other(_) => return None,
        })
    }

    /// Whether writing this feature can leave the monitor unreachable, or
    /// change something the user cannot undo from the same interface.
    ///
    /// Only power qualifies today, and it qualifies for a specific reason:
    /// [`PowerMode::Off`] can stop a monitor answering DDC at all, and the way
    /// back is a button on the bezel.
    #[must_use]
    pub const fn is_risky(self) -> bool {
        matches!(self, Self::PowerMode)
    }

    /// Every feature this crate names, in the order a menu should offer them:
    /// the things people reach for daily first.
    pub const NAMED: [Self; 12] = [
        Self::Brightness,
        Self::Contrast,
        Self::Volume,
        Self::Mute,
        Self::InputSource,
        Self::ColorPreset,
        Self::RedGain,
        Self::GreenGain,
        Self::BlueGain,
        Self::PowerMode,
        Self::OsdLanguage,
        Self::McssVersion,
    ];
}

/// A feature's reading: where it is now, and how far it goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Value {
    /// The current setting.
    pub current: u16,
    /// The largest value the monitor says it accepts.
    ///
    /// Not always 100, and not always honest. Panels that report 100 and then
    /// clamp at 80 exist, which is why a host should read a feature back after
    /// writing it rather than assuming the write landed where it aimed.
    pub maximum: u16,
}

impl Value {
    /// The current setting as a percentage of the maximum, rounded to nearest.
    ///
    /// `None` when the monitor reports a maximum of zero — a broken answer, but
    /// one that turns into a divide-by-zero if taken at face value.
    #[must_use]
    pub fn percent(self) -> Option<u8> {
        if self.maximum == 0 {
            return None;
        }
        let maximum = u32::from(self.maximum);
        // Doubling both sides is how this rounds to nearest without floats:
        // adding half a step before dividing.
        let scaled = (u32::from(self.current) * 200 + maximum) / (maximum * 2);
        Some(u8::try_from(scaled.min(100)).unwrap_or(100))
    }

    /// The raw value a percentage asks for, rounded to nearest and clamped to
    /// the monitor's own maximum.
    #[must_use]
    pub fn from_percent(percent: u8, maximum: u16) -> u16 {
        let percent = u32::from(percent.min(100));
        let scaled = (percent * u32::from(maximum) + 50) / 100;
        u16::try_from(scaled).unwrap_or(maximum)
    }
}

/// Which input a monitor is showing, feature `0x60`.
///
/// The named values are MCCS 2.2a's. Anything above them is vendor territory,
/// and USB-C is the one everybody wants and nobody standardised: `0x1B` and
/// `0x1C` both appear in the wild, on different panels, meaning different
/// ports. So USB-C is deliberately *not* named here. Read the capability
/// string, try the numbers it lists, and let the user name the one that works.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum InputSource {
    /// Analogue VGA, first connector.
    Vga1,
    /// Analogue VGA, second connector.
    Vga2,
    /// DVI, first connector.
    Dvi1,
    /// DVI, second connector.
    Dvi2,
    /// Composite video, first connector.
    Composite1,
    /// Composite video, second connector.
    Composite2,
    /// S-Video, first connector.
    SVideo1,
    /// S-Video, second connector.
    SVideo2,
    /// Tuner, first.
    Tuner1,
    /// Tuner, second.
    Tuner2,
    /// Tuner, third.
    Tuner3,
    /// Component video, first connector.
    Component1,
    /// Component video, second connector.
    Component2,
    /// Component video, third connector.
    Component3,
    /// DisplayPort, first connector.
    DisplayPort1,
    /// DisplayPort, second connector.
    DisplayPort2,
    /// HDMI, first connector.
    Hdmi1,
    /// HDMI, second connector.
    Hdmi2,
    /// A value outside the standard table — vendor-defined, and only
    /// meaningful for the monitor that listed it.
    Other(u8),
}

impl InputSource {
    /// The value that goes on the wire.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Vga1 => 0x01,
            Self::Vga2 => 0x02,
            Self::Dvi1 => 0x03,
            Self::Dvi2 => 0x04,
            Self::Composite1 => 0x05,
            Self::Composite2 => 0x06,
            Self::SVideo1 => 0x07,
            Self::SVideo2 => 0x08,
            Self::Tuner1 => 0x09,
            Self::Tuner2 => 0x0A,
            Self::Tuner3 => 0x0B,
            Self::Component1 => 0x0C,
            Self::Component2 => 0x0D,
            Self::Component3 => 0x0E,
            Self::DisplayPort1 => 0x0F,
            Self::DisplayPort2 => 0x10,
            Self::Hdmi1 => 0x11,
            Self::Hdmi2 => 0x12,
            Self::Other(code) => code,
        }
    }

    /// The input a value names.
    #[must_use]
    pub const fn from_code(code: u8) -> Self {
        match code {
            0x01 => Self::Vga1,
            0x02 => Self::Vga2,
            0x03 => Self::Dvi1,
            0x04 => Self::Dvi2,
            0x05 => Self::Composite1,
            0x06 => Self::Composite2,
            0x07 => Self::SVideo1,
            0x08 => Self::SVideo2,
            0x09 => Self::Tuner1,
            0x0A => Self::Tuner2,
            0x0B => Self::Tuner3,
            0x0C => Self::Component1,
            0x0D => Self::Component2,
            0x0E => Self::Component3,
            0x0F => Self::DisplayPort1,
            0x10 => Self::DisplayPort2,
            0x11 => Self::Hdmi1,
            0x12 => Self::Hdmi2,
            other => Self::Other(other),
        }
    }

    /// The name to speak or print, or `None` for a vendor-defined value.
    #[must_use]
    pub const fn name(self) -> Option<&'static str> {
        Some(match self {
            Self::Vga1 => "VGA 1",
            Self::Vga2 => "VGA 2",
            Self::Dvi1 => "DVI 1",
            Self::Dvi2 => "DVI 2",
            Self::Composite1 => "composite 1",
            Self::Composite2 => "composite 2",
            Self::SVideo1 => "S-Video 1",
            Self::SVideo2 => "S-Video 2",
            Self::Tuner1 => "tuner 1",
            Self::Tuner2 => "tuner 2",
            Self::Tuner3 => "tuner 3",
            Self::Component1 => "component 1",
            Self::Component2 => "component 2",
            Self::Component3 => "component 3",
            Self::DisplayPort1 => "DisplayPort 1",
            Self::DisplayPort2 => "DisplayPort 2",
            Self::Hdmi1 => "HDMI 1",
            Self::Hdmi2 => "HDMI 2",
            Self::Other(_) => return None,
        })
    }

    /// Parse a name a person typed or said.
    ///
    /// Deliberately forgiving about separators and case, because this is what
    /// sits behind `roadie monitor input hdmi-2`, and someone dictating that
    /// will say "HDMI two" and get whatever their speech engine writes.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        // Sixteen is comfortably past the longest name here, `displayport1`
        // at twelve. That margin is why the overflow below can return `None`
        // without losing anything: no name is sixteen characters, so input
        // long enough to fill this buffer could not have matched one anyway.
        let mut squashed = [0_u8; 16];
        let mut len = 0;
        for byte in text.bytes() {
            if byte.is_ascii_alphanumeric() {
                if len == squashed.len() {
                    return None;
                }
                squashed[len] = byte.to_ascii_lowercase();
                len += 1;
            }
        }
        Some(match &squashed[..len] {
            b"vga" | b"vga1" => Self::Vga1,
            b"vga2" => Self::Vga2,
            b"dvi" | b"dvi1" => Self::Dvi1,
            b"dvi2" => Self::Dvi2,
            b"composite" | b"composite1" => Self::Composite1,
            b"composite2" => Self::Composite2,
            b"svideo" | b"svideo1" => Self::SVideo1,
            b"svideo2" => Self::SVideo2,
            b"tuner" | b"tuner1" => Self::Tuner1,
            b"tuner2" => Self::Tuner2,
            b"tuner3" => Self::Tuner3,
            b"component" | b"component1" => Self::Component1,
            b"component2" => Self::Component2,
            b"component3" => Self::Component3,
            b"dp" | b"dp1" | b"displayport" | b"displayport1" => Self::DisplayPort1,
            b"dp2" | b"displayport2" => Self::DisplayPort2,
            b"hdmi" | b"hdmi1" => Self::Hdmi1,
            b"hdmi2" => Self::Hdmi2,
            _ => return None,
        })
    }
}

/// A monitor's power state, feature `0xD6`.
///
/// # Read [`Self::Off`] before you write it
///
/// A monitor in `Off` may stop answering DDC entirely, which means software
/// cannot turn it back on: the way back is the power button on the bezel. That
/// is an inconvenience for someone who can see the bezel and a genuine problem
/// for someone who cannot, so [`Self::is_recoverable`] exists to be checked,
/// and the layers above are expected to make crossing that line deliberate.
///
/// [`Self::ActiveOff`] is the one to reach for instead. It blanks the panel,
/// saves nearly the same power, and monitors generally keep listening.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PowerMode {
    /// Fully on.
    On,
    /// Standby.
    Standby,
    /// Suspend.
    Suspend,
    /// Active off — the panel is dark, the electronics are awake.
    ActiveOff,
    /// Hard off. May be a one-way trip; see the type's documentation.
    Off,
    /// A value outside the standard table.
    Other(u8),
}

impl PowerMode {
    /// The value that goes on the wire.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::On => 0x01,
            Self::Standby => 0x02,
            Self::Suspend => 0x03,
            Self::ActiveOff => 0x04,
            Self::Off => 0x05,
            Self::Other(code) => code,
        }
    }

    /// The state a value names.
    #[must_use]
    pub const fn from_code(code: u8) -> Self {
        match code {
            0x01 => Self::On,
            0x02 => Self::Standby,
            0x03 => Self::Suspend,
            0x04 => Self::ActiveOff,
            0x05 => Self::Off,
            other => Self::Other(other),
        }
    }

    /// Whether software can expect to bring the monitor back from this state.
    ///
    /// `false` for [`Self::Off`], and for anything unrecognised — an unknown
    /// power value is not a good thing to be optimistic about.
    #[must_use]
    pub const fn is_recoverable(self) -> bool {
        matches!(
            self,
            Self::On | Self::Standby | Self::Suspend | Self::ActiveOff
        )
    }

    /// The name to speak or print, or `None` for a value outside the table.
    #[must_use]
    pub const fn name(self) -> Option<&'static str> {
        Some(match self {
            Self::On => "on",
            Self::Standby => "standby",
            Self::Suspend => "suspend",
            Self::ActiveOff => "screen off",
            Self::Off => "powered off",
            Self::Other(_) => return None,
        })
    }
}

#[cfg(test)]
mod tests;
