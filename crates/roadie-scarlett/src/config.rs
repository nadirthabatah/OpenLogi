//! What a setting is, where it lives, and how it is written.
//!
//! A Scarlett does not expose named controls. It exposes a flat address space
//! that the host reads and writes with [`GET_DATA`] and [`SET_DATA`], and a
//! table per model saying which byte holds what. This module is that table's
//! shape; the tables module holds the numbers.
//!
//! Two things about a write are easy to get wrong and are modelled explicitly.
//!
//! **A setting can be one bit rather than one byte.** [`Descriptor::size_bits`]
//! is in bits, and a value below eight means the setting shares a byte with
//! its neighbours — phantom power is the common case, one bit per input pair.
//! Writing a whole byte there would set every neighbour to zero, which for
//! phantom power means silently switching it off on the other inputs.
//!
//! **A write may not take effect until it is activated.** Most settings are
//! followed by a separate activation command, [`Descriptor::activate`] naming
//! which. A host that writes and stops has changed the stored value and not
//! the hardware, and the interface will keep behaving as it did — with the
//! panel showing the new number.

/// Read from the device's address space.
pub const GET_DATA: u32 = 0x0080_0000;

/// Write to it.
pub const SET_DATA: u32 = 0x0080_0001;

/// Make a write take effect.
pub const DATA_CMD: u32 = 0x0080_0002;

/// Every setting either family exposes, named the way a person would.
///
/// Not every model has every one; [`ConfigSet::descriptor`] answers that
/// per model, and a `None` there means this device genuinely does not have
/// the setting rather than that the lookup failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum ConfigItem {
    /// `ag hot target`.
    AgHotTarget,
    /// `ag mean target`.
    AgMeanTarget,
    /// `ag peak target`.
    AgPeakTarget,
    /// Focusrite's \"air\" voicing on an input.
    AirSwitch,
    /// How that routine ended.
    AutogainStatus,
    /// Start the automatic gain routine.
    AutogainSwitch,
    /// `bluetooth volume`.
    BluetoothVolume,
    /// `compressor params`.
    CompressorParams,
    /// The dim and mute buttons together.
    DimMute,
    /// Monitoring the inputs directly, bypassing the computer.
    DirectMonitor,
    /// How loud that direct monitoring is.
    DirectMonitorGain,
    /// `dsp switch`.
    DspSwitch,
    /// `fp brightness`.
    FpBrightness,
    /// `fp sleep time`.
    FpSleepTime,
    /// `headphone volume`.
    HeadphoneVolume,
    /// Preamp gain, on the models with software gain control.
    InputGain,
    /// `input link switch`.
    InputLinkSwitch,
    /// `input mute switch`.
    InputMuteSwitch,
    /// `input select switch`.
    InputSelectSwitch,
    /// Line or instrument level on an input.
    LevelSwitch,
    /// One output's level.
    LineOutVolume,
    /// The monitor knob's level.
    MasterVolume,
    /// `monitor other enable`.
    MonitorOtherEnable,
    /// `monitor other switch`.
    MonitorOtherSwitch,
    /// Mass Storage Device mode, which the interface ships in.
    MsdSwitch,
    /// Mute on one output.
    MuteSwitch,
    /// The 10 dB pad on an input.
    PadSwitch,
    /// `pcm input switch`.
    PcmInputSwitch,
    /// `peq flt params`.
    PeqFltParams,
    /// `peq flt switch`.
    PeqFltSwitch,
    /// Whether phantom power survives a power cycle.
    PhantomPersistence,
    /// 48 V phantom power on a pair of inputs. The one setting here that can damage equipment — see [`crate::risk`].
    PhantomSwitch,
    /// `power ext`.
    PowerExt,
    /// `power low`.
    PowerLow,
    /// `precomp flt params`.
    PrecompFltParams,
    /// `precomp flt switch`.
    PrecompFltSwitch,
    /// Clip-safe, which backs the gain off when the input approaches clipping.
    SafeSwitch,
    /// Which S/PDIF carrier the interface uses.
    SpdifMode,
    /// `sp hp mute`.
    SpHpMute,
    /// Keep working with no computer attached.
    StandaloneSwitch,
    /// `sw hw switch`.
    SwHwSwitch,
    /// `talkback map`.
    TalkbackMap,
}
/// Where one setting lives on one model, and how to write it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Descriptor {
    /// The address of the byte the setting starts at.
    pub offset: u16,
    /// How wide the setting is, **in bits**.
    ///
    /// Eight or more means whole bytes, and an index selects among them. Fewer
    /// means the setting is one bit inside the byte at [`Self::offset`], and
    /// the index selects the bit — so writing it means reading the byte,
    /// changing that bit, and writing it back. Writing the byte outright would
    /// clear every neighbouring input's setting along with it.
    pub size_bits: u8,
    /// The activation command that makes a write take effect, or zero for a
    /// setting that needs none.
    pub activate: u8,
    /// Whether the write goes through the parameter buffer rather than
    /// straight to the address.
    ///
    /// The newer families write by putting the value and its index into a
    /// scratch area — [`ConfigSet::param_buf_addr`] — and then activating,
    /// rather than by writing the target address at all.
    pub via_param_buf: bool,
}

impl Descriptor {
    /// Whether this setting occupies whole bytes rather than a single bit.
    #[must_use]
    pub const fn is_whole_bytes(&self) -> bool {
        self.size_bits >= 8
    }

    /// How many bytes one value of this setting occupies.
    ///
    /// One for a bit-sized setting: it still lives inside a byte, and that
    /// byte is what a host reads and writes back.
    #[must_use]
    pub const fn byte_len(&self) -> usize {
        if self.is_whole_bytes() {
            self.size_bits as usize / 8
        } else {
            1
        }
    }

    /// The address of value `index` of this setting.
    ///
    /// Only meaningful for a whole-byte setting: a bit-sized one keeps every
    /// value in the same byte and distinguishes them by bit position, so its
    /// address does not move with the index.
    #[must_use]
    pub fn address(&self, index: u16) -> u16 {
        if self.is_whole_bytes() {
            let width = u16::from(self.size_bits / 8);
            self.offset.wrapping_add(index.wrapping_mul(width))
        } else {
            self.offset
        }
    }
}

/// One model's table of settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfigSet {
    /// The scratch address the newer families write through, or zero where
    /// there is none.
    pub param_buf_addr: u16,
    /// The settings this model has, and where each lives.
    pub items: &'static [(ConfigItem, Descriptor)],
}

impl ConfigSet {
    /// Where `item` lives on this model, or `None` if it has no such setting.
    ///
    /// `None` is an answer, not a failure: a Scarlett Solo has no pad and a
    /// 2nd-generation interface has no phantom control at all, and a host
    /// should say so rather than write to an address that means something
    /// else on that model.
    #[must_use]
    pub fn descriptor(&self, item: ConfigItem) -> Option<Descriptor> {
        self.items
            .iter()
            .find(|(candidate, _)| *candidate == item)
            .map(|(_, descriptor)| *descriptor)
    }

    /// Whether this model has `item` at all.
    #[must_use]
    pub fn has(&self, item: ConfigItem) -> bool {
        self.descriptor(item).is_some()
    }
}

#[cfg(test)]
use crate::tables;

/// Every table, checked as a set.
#[cfg(test)]
const ALL: &[(&str, ConfigSet)] = &[
    ("gen2a", tables::GEN2A),
    ("gen2b", tables::GEN2B),
    ("gen3a", tables::GEN3A),
    ("gen3b", tables::GEN3B),
    ("gen3c", tables::GEN3C),
    ("vocaster", tables::VOCASTER),
    ("gen4_solo", tables::GEN4_SOLO),
    ("gen4_solo_2417", tables::GEN4_SOLO_2417),
    ("gen4_2i2", tables::GEN4_2I2),
    ("gen4_2i2_2417", tables::GEN4_2I2_2417),
    ("gen4_4i4", tables::GEN4_4I4),
    ("gen4_4i4_2417", tables::GEN4_4I4_2417),
    ("clarett", tables::CLARETT),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_table_lists_a_setting_twice() {
        // `descriptor` takes the first match, so a duplicate would silently
        // shadow the second and there would be nothing to see in the answer.
        for (name, set) in ALL {
            let mut seen: Vec<ConfigItem> = set.items.iter().map(|(item, _)| *item).collect();
            let before = seen.len();
            seen.sort_unstable();
            seen.dedup();
            assert_eq!(seen.len(), before, "{name} lists a setting more than once");
        }
    }

    #[test]
    fn a_setting_a_model_lacks_is_absent_rather_than_zero() {
        // The 2nd-generation tables have no phantom control. Answering with a
        // zero offset would send a write to address zero, which on these
        // interfaces is a different setting entirely.
        assert!(!tables::GEN2A.has(ConfigItem::PhantomSwitch));
        assert_eq!(tables::GEN2A.descriptor(ConfigItem::PhantomSwitch), None);
        assert!(tables::GEN3C.has(ConfigItem::PhantomSwitch));
    }

    #[test]
    fn phantom_power_is_a_single_bit_on_the_third_generation() {
        // The detail that makes a naive write dangerous: one bit per input
        // pair inside a shared byte, so writing the byte would switch phantom
        // power off on every other pair.
        let phantom = tables::GEN3C
            .descriptor(ConfigItem::PhantomSwitch)
            .expect("the 18i8 has phantom power");
        assert_eq!(phantom.size_bits, 1);
        assert!(!phantom.is_whole_bytes());
        assert_eq!(phantom.byte_len(), 1);
        assert_eq!(
            phantom.address(0),
            phantom.address(1),
            "every pair shares the byte; the index picks the bit, not the address"
        );
    }

    #[test]
    fn a_whole_byte_setting_advances_with_its_index() {
        // Output volume is sixteen bits, so the second output is two bytes
        // along rather than one.
        let volume = tables::GEN3C
            .descriptor(ConfigItem::LineOutVolume)
            .expect("the 18i8 has output volumes");
        assert_eq!(volume.size_bits, 16);
        assert_eq!(volume.byte_len(), 2);
        assert_eq!(volume.address(1), volume.offset + 2);
        assert_eq!(volume.address(3), volume.offset + 6);
    }

    #[test]
    fn the_newer_families_write_through_a_scratch_address() {
        // Gen 4 and Vocaster put the value somewhere else and activate;
        // everything older writes the target address directly.
        assert_ne!(tables::GEN4_4I4.param_buf_addr, 0);
        assert_ne!(tables::VOCASTER.param_buf_addr, 0);
        assert_eq!(tables::GEN3C.param_buf_addr, 0);
        assert_eq!(tables::CLARETT.param_buf_addr, 0);
    }

    #[test]
    fn a_setting_written_through_the_scratch_area_always_has_somewhere_to_put_it() {
        // A table claiming a parameter-buffer write while naming no scratch
        // address would send the value to address zero.
        for (name, set) in ALL {
            for (item, descriptor) in set.items {
                assert!(
                    !descriptor.via_param_buf || set.param_buf_addr != 0,
                    "{name} writes {item:?} through a scratch area it does not have"
                );
            }
        }
    }
}
