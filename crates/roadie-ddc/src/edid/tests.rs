//! EDID tests.
//!
//! The fixtures are built to the EDID 1.4 layout rather than captured from a
//! monitor, since this project has none. Two things make that less circular
//! than it sounds: the packed manufacturer codes below are `0x10AC` for Dell
//! and `0x1E6D` for LG, which are the published PNP IDs and were not derived
//! from this code; and the blocks carry real checksums, so a fixture with a
//! wrong one had to be broken deliberately.

use super::*;

/// A Dell U2723QE: brand code `DEL`, a name descriptor, and a text serial.
const DELL: [u8; 128] = [
    0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x10, 0xAC, 0x81, 0x41, 0x78, 0x56, 0x34, 0x12,
    0x0A, 0x20, 0x01, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x1F, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFC, 0x00, 0x55, 0x32, 0x37,
    0x32, 0x33, 0x51, 0x45, 0x0A, 0x20, 0x20, 0x20, 0x20, 0x20, 0x00, 0x00, 0x00, 0xFF, 0x00, 0x41,
    0x42, 0x43, 0x31, 0x32, 0x33, 0x0A, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x00, 0x00, 0x00, 0x10,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x52,
];

/// An LG whose name descriptor already carries the brand.
const LG: [u8; 128] = [
    0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x1E, 0x6D, 0x11, 0x5B, 0x00, 0x00, 0x00, 0x00,
    0x01, 0x1F, 0x01, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFC, 0x00, 0x4C, 0x47, 0x20, 0x55, 0x4C,
    0x54, 0x52, 0x41, 0x46, 0x49, 0x4E, 0x45, 0x0A, 0x40, 0x1F, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x08,
];

/// A display with no name descriptor at all, from a vendor with no entry in the table.
const NAMELESS: [u8; 128] = [
    0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x6B, 0x5A, 0xAB, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x01, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x1F, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x1F, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xB3,
];

/// The Dell block with one byte of its checksum wrong, as displays really ship.
const BAD_CHECKSUM: [u8; 128] = [
    0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x10, 0xAC, 0x81, 0x41, 0x78, 0x56, 0x34, 0x12,
    0x0A, 0x20, 0x01, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x1F, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFC, 0x00, 0x55, 0x32, 0x37,
    0x32, 0x33, 0x51, 0x45, 0x0A, 0x20, 0x20, 0x20, 0x20, 0x20, 0x00, 0x00, 0x00, 0xFF, 0x00, 0x41,
    0x42, 0x43, 0x31, 0x32, 0x33, 0x0A, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20, 0x00, 0x00, 0x00, 0x10,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x53,
];

/// Apply edits to a block and repair its checksum, so a test can change one
/// field without hand-computing the last byte — and so a test that *wants* a
/// bad checksum has to say so.
fn patched(base: [u8; 128], edits: &[(usize, u8)]) -> [u8; 128] {
    let mut block = base;
    for (index, byte) in edits {
        block[*index] = *byte;
    }
    block[127] = 0;
    block[127] = 0_u8.wrapping_sub(checksum(&block));
    block
}

#[test]
fn a_dell_block_parses_to_the_display_on_the_label() {
    let edid = Edid::parse(&DELL).unwrap();

    assert_eq!(&edid.manufacturer, b"DEL");
    assert_eq!(edid.vendor(), Some("Dell"));
    assert_eq!(edid.product_code, 0x4181);
    assert_eq!(edid.serial_number, 0x1234_5678);
    assert_eq!(edid.name.as_deref(), Some("U2723QE"));
    assert_eq!(edid.serial_text.as_deref(), Some("ABC123"));
    assert_eq!(edid.version, (1, 4));
    assert_eq!(edid.manufacture_week, Some(10));
    assert_eq!(edid.manufacture_year, Some(2022));
    assert_eq!(edid.warnings, Vec::<String>::new());
}

#[test]
fn a_description_puts_the_brand_in_front_of_the_model() {
    assert_eq!(Edid::parse(&DELL).unwrap().describe(), "Dell U2723QE");
}

#[test]
fn a_brand_already_in_the_name_is_not_said_twice() {
    // "LG LG ULTRAFINE" is what this sounds like when nobody checks, and it
    // sounds worse than it reads.
    let edid = Edid::parse(&LG).unwrap();

    assert_eq!(edid.vendor(), Some("LG"));
    assert_eq!(edid.name.as_deref(), Some("LG ULTRAFINE"));
    assert_eq!(edid.describe(), "LG ULTRAFINE");
}

#[test]
fn a_brand_that_merely_prefixes_the_name_is_still_said() {
    // "LGX" starts with "LG" and is not it. Matching on characters rather
    // than on words would swallow the brand here.
    // LG's name descriptor is the first of the four, so its text starts at
    // 54 + 5.
    let block = patched(LG, &[(59, b'L'), (60, b'G'), (61, b'X'), (62, 0x0A)]);

    let edid = Edid::parse(&block).unwrap();

    assert_eq!(edid.name.as_deref(), Some("LGX"));
    assert_eq!(edid.describe(), "LG LGX");
}

#[test]
fn an_unknown_vendor_falls_back_to_its_three_letter_code() {
    let edid = Edid::parse(&NAMELESS).unwrap();

    assert_eq!(edid.vendor(), None);
    assert_eq!(edid.manufacturer_code(), "ZZZ");
}

#[test]
fn a_display_with_no_name_is_still_described_by_something_sayable() {
    // "monitor 2" is not an answer when there are three of them.
    let edid = Edid::parse(&NAMELESS).unwrap();

    assert_eq!(edid.name, None);
    assert_eq!(edid.describe(), "ZZZ 0x00ab");
}

#[test]
fn a_bad_checksum_is_a_warning_rather_than_a_refusal() {
    // Displays ship this wrong. Refusing to name one over it helps nobody.
    let edid = Edid::parse(&BAD_CHECKSUM).unwrap();

    assert_eq!(edid.name.as_deref(), Some("U2723QE"));
    assert_eq!(edid.vendor(), Some("Dell"));
    assert!(
        edid.warnings.iter().any(|w| w.contains("checksum")),
        "{:?}",
        edid.warnings
    );
}

#[test]
fn a_good_checksum_produces_no_warning() {
    assert_eq!(checksum(&DELL), 0);
    assert_eq!(Edid::parse(&DELL).unwrap().warnings, Vec::<String>::new());
}

#[test]
fn a_manufacturer_code_that_is_not_three_letters_is_flagged_and_not_invented() {
    // All-zero packed bits: no letter is zero, so nothing here is a code.
    let block = patched(DELL, &[(8, 0x00), (9, 0x00)]);

    let edid = Edid::parse(&block).unwrap();

    assert_eq!(edid.manufacturer_code(), "???");
    assert_eq!(edid.vendor(), None);
    assert!(
        edid.warnings.iter().any(|w| w.contains("three letters")),
        "{:?}",
        edid.warnings
    );
}

#[test]
fn bytes_that_are_not_an_edid_are_rejected_outright() {
    // The header is eight bytes that do not occur by accident, so failing it
    // means "not an EDID", not "a damaged EDID".
    let block = patched(DELL, &[(1, 0xFE)]);

    assert_eq!(Edid::parse(&block).unwrap_err(), EdidError::NotEdid);
    assert_eq!(Edid::parse(&[0_u8; 128]).unwrap_err(), EdidError::NotEdid);
}

#[test]
fn a_short_read_is_reported_as_short_rather_than_as_not_an_edid() {
    // Two different problems with two different fixes: a short read is a
    // transport question, a bad header is a "this is not a display" question.
    let error = Edid::parse(&DELL[..64]).unwrap_err();

    assert_eq!(error, EdidError::TooShort { len: 64 });
}

#[test]
fn extension_blocks_after_the_base_block_are_ignored_rather_than_rejected() {
    // The kernel hands out base and extension blocks in one file.
    let mut long = DELL.to_vec();
    long.extend_from_slice(&[0x02; 128]);

    assert_eq!(Edid::parse(&long).unwrap(), Edid::parse(&DELL).unwrap());
}

#[test]
fn descriptor_text_stops_at_the_terminator_and_drops_the_padding() {
    // The field is 13 bytes, newline-terminated and space-padded. Reading all
    // 13 gives a name with five trailing spaces, which a screen reader reads
    // as a pause and a terminal shows as nothing.
    assert_eq!(descriptor_text(b"U2723QE\n     "), "U2723QE");
    assert_eq!(descriptor_text(b"U2723QE      "), "U2723QE");
    assert_eq!(descriptor_text(b"U2723QE\0\0\0\0\0\0"), "U2723QE");
    assert_eq!(descriptor_text(b"ABCDEFGHIJKLM"), "ABCDEFGHIJKLM");
}

#[test]
fn a_name_descriptor_holding_only_padding_is_treated_as_absent() {
    // An empty string is worse than no string: it describes a display as ""
    // rather than falling back to something sayable. Dell's name descriptor is
    // the second of the four, so its text starts at 54 + 18 + 5 — and the
    // terminator goes at the front of the text, not at the front of the
    // descriptor, which would only prove that a corrupted descriptor is
    // skipped.
    let block = patched(DELL, &[(77, 0x0A)]);

    let edid = Edid::parse(&block).unwrap();

    assert_eq!(edid.name, None);
    assert_eq!(edid.describe(), "Dell 0x4181");
}

#[test]
fn a_detailed_timing_descriptor_is_not_read_as_text() {
    // A timing descriptor's first bytes are a pixel clock, not a tag — but its
    // fourth byte is vertical timing, and 0xfc is a perfectly ordinary value
    // for it. Only the leading zeroes distinguish a display descriptor from a
    // timing one, so this puts a timing block that looks like a name
    // descriptor *after* the real name, where reading it would win.
    let mut edits = vec![
        (108, 0x40),
        (109, 0x1F),
        (110, 0x00),
        (111, 0xFC),
        (112, 0x00),
    ];
    edits.extend((113..121).map(|index| (index, b'X')));
    let block = patched(DELL, &edits);

    let edid = Edid::parse(&block).unwrap();

    assert_eq!(edid.name.as_deref(), Some("U2723QE"));
}

#[test]
fn a_timing_descriptor_whose_pixel_clock_starts_with_a_zero_byte_is_still_timing() {
    // The pixel clock is two little-endian bytes, so its low byte is zero
    // whenever the clock is a multiple of 2.56 MHz — 25.6 MHz here. Checking
    // only the first byte would call this a display descriptor, and its fourth
    // byte is horizontal blanking, which is 0xfc as readily as anything else.
    let mut edits = vec![
        (108, 0x00),
        (109, 0x0A),
        (110, 0x00),
        (111, 0xFC),
        (112, 0x00),
    ];
    edits.extend((113..121).map(|index| (index, b'X')));
    let block = patched(DELL, &edits);

    let edid = Edid::parse(&block).unwrap();

    assert_eq!(edid.name.as_deref(), Some("U2723QE"));
}

#[test]
fn a_year_of_zero_means_the_display_did_not_say() {
    let block = patched(DELL, &[(17, 0x00)]);

    assert_eq!(Edid::parse(&block).unwrap().manufacture_year, None);
}

#[test]
fn a_week_outside_the_calendar_is_dropped_rather_than_reported() {
    // 0xff is the specification's "the year byte is a model year" flag, and
    // 0 means unstated. Neither is a week.
    for week in [0x00_u8, 0xFF] {
        let block = patched(DELL, &[(16, week)]);
        assert_eq!(
            Edid::parse(&block).unwrap().manufacture_week,
            None,
            "week {week:#04x}"
        );
    }
}

#[test]
fn the_summary_features_lead_with_what_people_came_for() {
    assert_eq!(SUMMARY_FEATURES[0], Feature::Brightness);
    assert_eq!(SUMMARY_FEATURES[1], Feature::InputSource);
}
