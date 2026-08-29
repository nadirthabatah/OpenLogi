//! How a VIA-capable device is recognised among a machine's HID collections.

/// Bytes in every VIA request and every VIA response.
///
/// Fixed by the protocol: QMK's raw HID endpoint is 32 bytes each way, and a
/// request is padded to that length rather than sent short. Encoding a
/// command therefore always produces exactly this many bytes, which is why
/// [`crate::command::Command::encode`] returns an array rather than a `Vec`
/// that a caller could get wrong.
pub const REPORT_LEN: usize = 32;

/// The HID usage page QMK's raw endpoint lives on.
///
/// Vendor-defined, and shared by every VIA board — which is what makes a
/// vendor-neutral driver possible at all. It is not, however, exclusive to
/// VIA: other firmware uses the vendor page too, so matching on it identifies
/// a *candidate*, and only a successful protocol-version exchange confirms
/// one. Treating the usage page as proof would mean writing keycodes at
/// whatever else happened to answer there.
pub const USAGE_PAGE: u16 = 0xff60;

/// The HID usage id of QMK's raw endpoint.
pub const USAGE_ID: u16 = 0x0061;

/// Whether a HID collection could be a VIA endpoint.
///
/// A candidate, not a confirmation — see [`USAGE_PAGE`].
#[must_use]
pub const fn is_via_collection(usage_page: u16, usage_id: u16) -> bool {
    usage_page == USAGE_PAGE && usage_id == USAGE_ID
}

#[cfg(test)]
mod tests {
    use super::{REPORT_LEN, USAGE_ID, USAGE_PAGE, is_via_collection};

    #[test]
    fn the_via_collection_is_recognised() {
        assert!(is_via_collection(USAGE_PAGE, USAGE_ID));
    }

    /// The near-misses matter more than the hit: writing keycodes at the
    /// wrong endpoint is how a keyboard loses a key.
    #[test]
    fn a_neighbouring_collection_is_not_a_via_endpoint() {
        assert!(
            !is_via_collection(USAGE_PAGE, 0x0060),
            "right page, wrong usage"
        );
        assert!(
            !is_via_collection(0xff43, USAGE_ID),
            "wrong page, right usage"
        );
        assert!(!is_via_collection(0x0001, 0x0006), "the keyboard page");
    }

    #[test]
    fn the_report_length_is_the_raw_hid_endpoint_size() {
        assert_eq!(REPORT_LEN, 32);
    }
}
