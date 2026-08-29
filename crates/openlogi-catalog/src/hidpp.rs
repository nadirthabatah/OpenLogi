//! Which HID collections carry Logitech's HID++ traffic.
//!
//! A Logitech peripheral exposes several HID collections and only some of them
//! speak HID++. Knowing which is catalog data — a fact about hardware, not
//! about any host — so it lives here, where the host transport and the device
//! survey can both read it instead of keeping two lists that could disagree
//! about what a device is.

/// HID++ long-report vendor collections, as `(usage_page, usage_id, long_only)`.
///
/// Logitech exposes its HID++ long-report (report id `0x11`) under a
/// vendor-defined HID collection, but the page differs by transport:
///
/// - `0xFF00 / 0x0002` — USB, Logi Bolt / Unifying receivers, and
///   Bluetooth-*classic* devices (MX Master over BT).
/// - `0xFF43 / 0x0202` — Bluetooth-*Low-Energy* directly-paired devices
///   (e.g. the Logitech Lift / Signature mice). Same HID++ protocol, just a
///   different vendor page on the BLE HID report descriptor.
/// - `0xFF43 / 0x0602` — wired G-series gaming keyboards (e.g. the G513): a
///   distinct vendor collection on the same `0xFF43` page. Carries both report
///   widths, so it is not long-only.
///
/// `long_only` marks a transport that exposes *only* the long report — no
/// short-report (`0x10`) collection — so short HID++ requests must be
/// up-converted to long (handled by the `hidpp` channel). BLE-direct devices on
/// macOS are long-only; USB / receiver / wired-keyboard devices carry both.
/// Keeping the flag in this table means a new long-only transport is a
/// single-line addition here, with no second site to update.
///
/// Filtering on these pairs gives us one HID node per physical HID++ device on
/// every supported OS, without reading report descriptors (`async-hid 0.4`
/// only exposes those on Linux).
pub const LONG_COLLECTIONS: [(u16, u16, bool); 3] = [
    (0xff00, 0x0002, false),
    (0xff43, 0x0202, true),
    (0xff43, 0x0602, false),
];

/// Whether `(usage_page, usage_id)` is one of the HID++ long-report collections.
#[must_use]
pub fn is_long_collection(usage_page: u16, usage_id: u16) -> bool {
    LONG_COLLECTIONS
        .iter()
        .any(|&(page, usage, _)| (page, usage) == (usage_page, usage_id))
}

/// Whether a matched HID++ collection exposes only the long report.
///
/// `false` for any pair not in [`LONG_COLLECTIONS`], which is the safe answer:
/// a collection we do not recognise is not one we should be re-framing traffic
/// for.
#[must_use]
pub fn is_long_only(usage_page: u16, usage_id: u16) -> bool {
    LONG_COLLECTIONS
        .iter()
        .any(|&(page, usage, long_only)| (page, usage) == (usage_page, usage_id) && long_only)
}

#[cfg(test)]
mod tests {
    use super::{LONG_COLLECTIONS, is_long_collection, is_long_only};

    #[test]
    fn the_catalogued_collections_are_recognised() {
        for (page, usage, _) in LONG_COLLECTIONS {
            assert!(
                is_long_collection(page, usage),
                "{page:#06x}/{usage:#06x} is in the table but not recognised"
            );
        }
    }

    #[test]
    fn a_collection_outside_the_table_is_not_hidpp() {
        assert!(!is_long_collection(0x0001, 0x0006), "the keyboard page");
        assert!(
            !is_long_collection(0xff00, 0x0001),
            "right page, wrong usage"
        );
        assert!(
            !is_long_collection(0xff43, 0x0203),
            "right page, wrong usage"
        );
    }

    #[test]
    fn only_the_flagged_collection_is_long_only() {
        assert!(is_long_only(0xff43, 0x0202));
        assert!(!is_long_only(0xff00, 0x0002));
        assert!(!is_long_only(0xff43, 0x0602));
    }

    /// A pair we do not know is not long-only. Answering `true` here would
    /// have the transport re-frame traffic for a collection it has never seen.
    #[test]
    fn an_unknown_collection_is_not_long_only() {
        assert!(!is_long_only(0x1234, 0x5678));
    }
}
