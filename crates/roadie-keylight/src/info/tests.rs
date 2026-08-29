use super::AccessoryInfo;

/// The answer a Key Light Air actually sends, trimmed to the fields read here.
const REAL: &str = r#"{
  "productName": "Elgato Key Light Air",
  "hardwareBoardType": 200,
  "firmwareBuildNumber": 218,
  "firmwareVersion": "1.0.3",
  "serialNumber": "CW31J1A00183",
  "displayName": "Key Light Left",
  "features": ["lights"],
  "wifi-info": { "ssid": "somewhere", "frequencyMHz": 2400, "rssi": -47 }
}"#;

#[test]
fn the_documented_answer_parses_including_fields_this_build_ignores() {
    // The unread `wifi-info` and `hardwareBoardType` must not make the whole
    // answer unparseable: the firmware adds fields between versions, and a
    // parser that rejected the ones it did not know would break on an update.
    let info: AccessoryInfo = serde_json::from_str(REAL).expect("the documented shape");
    assert_eq!(info.display_name.as_deref(), Some("Key Light Left"));
    assert_eq!(info.product_name.as_deref(), Some("Elgato Key Light Air"));
    assert_eq!(info.serial_number.as_deref(), Some("CW31J1A00183"));
    assert_eq!(info.firmware_version.as_deref(), Some("1.0.3"));
}

#[test]
fn an_older_unit_that_sends_almost_nothing_still_parses() {
    // Every field is optional because the answer has grown over the years.
    let info: AccessoryInfo = serde_json::from_str("{}").expect("an empty object is valid");
    assert_eq!(info, AccessoryInfo::default());
}

#[test]
fn the_name_someone_gave_the_light_wins_over_the_model() {
    // A desk with two of them has them called "key left" and "key right", and
    // those are the words that person will use.
    let info: AccessoryInfo = serde_json::from_str(REAL).expect("the documented shape");
    assert_eq!(info.describe(), "Key Light Left");
}

#[test]
fn a_light_with_no_given_name_falls_back_to_its_model() {
    let info = AccessoryInfo {
        product_name: Some("Elgato Key Light Air".to_owned()),
        ..AccessoryInfo::default()
    };
    assert_eq!(info.describe(), "Elgato Key Light Air");
}

#[test]
fn a_blank_name_is_treated_as_no_name_rather_than_printed() {
    // A light named with spaces would otherwise reach a screen reader as a
    // pause where a name should be.
    let info = AccessoryInfo {
        display_name: Some("   ".to_owned()),
        product_name: Some("Elgato Key Light".to_owned()),
        ..AccessoryInfo::default()
    };
    assert_eq!(info.describe(), "Elgato Key Light");
}

#[test]
fn a_light_that_names_no_name_at_all_says_so_rather_than_inventing_one() {
    assert_eq!(
        AccessoryInfo::default().describe(),
        "an unnamed Elgato light"
    );
}

#[test]
fn a_device_on_the_same_service_that_is_not_a_light_is_left_alone() {
    let not_a_light = AccessoryInfo {
        product_name: Some("Elgato Something Else".to_owned()),
        features: vec!["camera".to_owned()],
        ..AccessoryInfo::default()
    };
    assert!(!not_a_light.controls_lights());
}

#[test]
fn an_older_unit_listing_no_features_is_assumed_to_be_a_light() {
    // The field arrived after the first models, and refusing those would drop
    // working hardware to satisfy a newer field.
    assert!(AccessoryInfo::default().controls_lights());
}
