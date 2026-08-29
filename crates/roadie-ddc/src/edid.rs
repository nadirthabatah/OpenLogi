//! EDID: who the display is, as opposed to what it can do.
//!
//! Every display carries a 128-byte identity block in an EEPROM on the same
//! cable as DDC/CI, at I²C address `0x50` rather than `0x37`. It is read-only,
//! and it answers even on monitors that have DDC/CI switched off in their menu
//! — which makes it the right thing to read first. A display that has an EDID
//! and refuses DDC is a settings problem the user can fix. A display with
//! neither is a wiring problem. Telling those apart is most of what a
//! diagnostic for monitors has to do.
//!
//! On Linux there is a shortcut worth taking: the kernel already read the EDID
//! and publishes it at `/sys/class/drm/*/edid`, so identifying a monitor needs
//! no I²C access and no permissions at all. Naming displays before asking for
//! bus access is what lets a tool say "the Dell is not answering" instead of
//! "bus 7 is not answering".
//!
//! # Only the base block
//!
//! EDID has extension blocks — CTA-861 for audio and video modes, DisplayID for
//! newer capabilities — and none of them are parsed here. Everything needed to
//! *name* a display is in the first 128 bytes, and parsing timing tables that
//! nothing reads would be surface with no purpose.
//!
//! # A wrong checksum is not a wrong monitor
//!
//! Displays ship with miscomputed EDID checksums. It is common enough that
//! treating it as fatal would refuse to name hardware that works perfectly, so
//! the header — eight bytes that cannot occur by accident — is what this
//! module insists on, and a checksum mismatch lands in [`Edid::warnings`]
//! instead. Same trade as the capability parser, for the same reason.

use crate::vcp::Feature;

/// The eight bytes every EDID begins with.
const HEADER: [u8; 8] = [0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00];

/// The base block's length. Extension blocks follow it and are not read here.
pub const BLOCK_LEN: usize = 128;

/// The I²C address the EDID EEPROM answers on — not the same as
/// [`crate::packet::I2C_ADDRESS`], which is where control lives.
pub const EDID_I2C_ADDRESS: u8 = 0x50;

/// Where the four 18-byte descriptors start.
const DESCRIPTORS: usize = 54;

/// A parsed EDID base block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edid {
    /// The three-letter PNP manufacturer code, such as `DEL` or `GSM`.
    pub manufacturer: [u8; 3],
    /// The vendor's own product code.
    pub product_code: u16,
    /// The serial number as a number. Zero means the display did not set one,
    /// which is common; the text serial in [`Self::serial_text`] is often the
    /// one printed on the label.
    pub serial_number: u32,
    /// Week of manufacture, 1 to 54. `None` when the display used the byte to
    /// mean something else, which the specification allows.
    pub manufacture_week: Option<u8>,
    /// Year of manufacture.
    pub manufacture_year: Option<u16>,
    /// EDID version and revision, such as `(1, 4)`.
    pub version: (u8, u8),
    /// The monitor name descriptor, if the display has one. This is the string
    /// worth saying aloud: `U2723QE`, `LG ULTRAFINE`.
    pub name: Option<String>,
    /// The serial-number descriptor, if the display has one.
    pub serial_text: Option<String>,
    /// What the parser worked around. Empty for a well-formed block.
    pub warnings: Vec<String>,
}

impl Edid {
    /// Parse an EDID base block.
    ///
    /// Extra bytes past the base block are ignored rather than rejected: the
    /// kernel hands out the extension blocks in the same file.
    ///
    /// # Errors
    ///
    /// Returns [`EdidError`] when the input is shorter than a base block or
    /// does not begin with the EDID header.
    pub fn parse(bytes: &[u8]) -> Result<Self, EdidError> {
        let Some(block) = bytes.get(..BLOCK_LEN) else {
            return Err(EdidError::TooShort { len: bytes.len() });
        };
        if block[..8] != HEADER {
            return Err(EdidError::NotEdid);
        }
        let mut warnings = Vec::new();
        if checksum(block) != 0 {
            // Recorded, not fatal: displays ship this wrong, and refusing to
            // name one over it helps nobody.
            warnings.push("the EDID checksum does not match; the block may be damaged".to_owned());
        }

        let (manufacturer, manufacturer_ok) = manufacturer(block);
        if !manufacturer_ok {
            warnings.push("the manufacturer code is not three letters".to_owned());
        }

        // A year byte of zero means the display did not state one.
        let year_offset = block[17];
        let manufacture_year = (year_offset != 0).then(|| 1990 + u16::from(year_offset));
        let week = block[16];
        let manufacture_week = (1..=54).contains(&week).then_some(week);

        let mut name = None;
        let mut serial_text = None;
        for index in 0..4 {
            let start = DESCRIPTORS + index * 18;
            let Some(descriptor) = block.get(start..start + 18) else {
                break;
            };
            // A display descriptor is flagged by zeroes where a detailed
            // timing descriptor would put a nonzero pixel clock.
            if descriptor[0] != 0 || descriptor[1] != 0 || descriptor[2] != 0 {
                continue;
            }
            match descriptor[3] {
                0xFC => name = Some(descriptor_text(&descriptor[5..])),
                0xFF => serial_text = Some(descriptor_text(&descriptor[5..])),
                _ => {}
            }
        }

        Ok(Self {
            manufacturer,
            product_code: u16::from_le_bytes([block[10], block[11]]),
            serial_number: u32::from_le_bytes([block[12], block[13], block[14], block[15]]),
            manufacture_week,
            manufacture_year,
            version: (block[18], block[19]),
            name: name.filter(|text| !text.is_empty()),
            serial_text: serial_text.filter(|text| !text.is_empty()),
            warnings,
        })
    }

    /// The manufacturer code as text, such as `DEL`.
    #[must_use]
    pub fn manufacturer_code(&self) -> &str {
        core::str::from_utf8(&self.manufacturer).unwrap_or("???")
    }

    /// The manufacturer's trading name, when it is one this crate knows.
    ///
    /// The PNP codes are not guessable — LG's is `GSM`, from Goldstar, and
    /// Samsung's is `SAM` only by luck. Read aloud, `GSM` is three letters of
    /// nothing, so the table earns its place: it is the difference between a
    /// monitor list that names brands and one that spells codes.
    #[must_use]
    pub fn vendor(&self) -> Option<&'static str> {
        Some(match &self.manufacturer {
            b"ACI" | b"AUS" => "ASUS",
            b"ACR" => "Acer",
            b"AOC" => "AOC",
            b"APP" => "Apple",
            b"BNQ" => "BenQ",
            b"DEL" => "Dell",
            b"EIZ" | b"ENC" => "EIZO",
            b"GBT" | b"GIG" => "Gigabyte",
            b"GSM" | b"LGD" => "LG",
            b"HPN" | b"HWP" => "HP",
            b"IVM" => "Iiyama",
            b"LEN" => "Lenovo",
            b"MSI" => "MSI",
            b"NEC" => "NEC",
            b"PHL" => "Philips",
            b"SAM" => "Samsung",
            b"SHP" => "Sharp",
            b"SNY" => "Sony",
            b"VSC" => "ViewSonic",
            _ => return None,
        })
    }

    /// A one-line description, meant to be read aloud.
    ///
    /// Falls back through what the display actually gave us rather than
    /// printing blanks: brand and model when both are known, the raw
    /// manufacturer code and a hex product code when they are not. Every
    /// display produces something sayable, because "monitor 2" is not an
    /// answer when there are three of them.
    #[must_use]
    pub fn describe(&self) -> String {
        let brand = self.vendor().unwrap_or_else(|| self.manufacturer_code());
        match &self.name {
            // Some displays put the brand in the name descriptor already, and
            // "LG LG ULTRAFINE" is how that sounds when nobody checks.
            Some(name) if starts_with_word(name, brand) => name.clone(),
            Some(name) => format!("{brand} {name}"),
            None => format!("{brand} {:#06x}", self.product_code),
        }
    }
}

/// Whether `text` begins with `word` at a word boundary.
fn starts_with_word(text: &str, word: &str) -> bool {
    let Some(head) = text.get(..word.len()) else {
        return false;
    };
    head.eq_ignore_ascii_case(word)
        && text[word.len()..]
            .chars()
            .next()
            .is_none_or(|ch| !ch.is_alphanumeric())
}

/// The EDID checksum: every byte of the block sums to zero, modulo 256.
#[must_use]
pub fn checksum(block: &[u8]) -> u8 {
    block.iter().fold(0_u8, |sum, byte| sum.wrapping_add(*byte))
}

/// Decode the three packed five-bit letters of a PNP manufacturer code.
fn manufacturer(block: &[u8]) -> ([u8; 3], bool) {
    let packed = u16::from_be_bytes([block[8], block[9]]);
    let mut out = [b'?'; 3];
    let mut ok = true;
    for (index, shift) in [10_u16, 5, 0].into_iter().enumerate() {
        let value = ((packed >> shift) & 0x1F) as u8;
        // 1 is 'A'; zero and anything past 26 are not letters.
        if (1..=26).contains(&value) {
            out[index] = b'A' + value - 1;
        } else {
            ok = false;
        }
    }
    (out, ok)
}

/// Read a descriptor's 13-byte text field.
///
/// The field is terminated by a newline and padded with spaces, and displays
/// are inconsistent about both, so this trims either — and NUL, which some pad
/// with instead and which `str::trim` does not touch.
fn descriptor_text(bytes: &[u8]) -> String {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0x0A)
        .unwrap_or(bytes.len());
    bytes[..end]
        .iter()
        .map(|byte| char::from(*byte))
        .collect::<String>()
        .trim_matches(|ch: char| ch.is_whitespace() || ch == '\0')
        .to_owned()
}

/// The features worth reading to describe a display's current state, in the
/// order a summary should say them.
///
/// Here rather than in a frontend because the ordering is about displays, not
/// about any one interface: brightness first because it is what people came
/// for, input source next because it is what they came for when it was not
/// brightness.
pub const SUMMARY_FEATURES: [Feature; 4] = [
    Feature::Brightness,
    Feature::InputSource,
    Feature::Contrast,
    Feature::Volume,
];

/// Why an EDID could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EdidError {
    /// Fewer bytes than a base block.
    #[error("an EDID base block is 128 bytes; got {len}")]
    TooShort {
        /// How many bytes arrived.
        len: usize,
    },
    /// The eight-byte header was missing.
    ///
    /// A strong signal, not a marginal one: those eight bytes do not occur by
    /// accident, so this means the bytes are not an EDID at all rather than
    /// that they are a damaged one.
    #[error("these bytes do not begin with the EDID header")]
    NotEdid,
}

#[cfg(test)]
mod tests;
