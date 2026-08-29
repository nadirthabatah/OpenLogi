//! Which interface this is, and which table it uses.
//!
//! A Scarlett is identified by its USB product id under Focusrite's vendor id.
//! That is enough to name the model and to say how many inputs it has of each
//! kind — but not always enough to choose its table, because the newer
//! families changed their address layout in a firmware update. So the lookup
//! takes the firmware version too, and a host that does not read one gets the
//! oldest layout, which is the safe way to be wrong: an address that has moved
//! reads as a setting the interface does not have, rather than as a different
//! setting written by mistake.

use crate::config::ConfigSet;
use crate::tables;

/// Focusrite's USB vendor id.
pub const VENDOR_ID: u16 = 0x1235;

/// One table and the firmware version it starts applying at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FirmwareTable {
    /// The first firmware version this table describes.
    pub from_firmware: u16,
    /// The table itself.
    pub table: ConfigSet,
}

/// One interface Focusrite makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Model {
    /// Its USB product id, under [`VENDOR_ID`].
    pub product_id: u16,
    /// What it is called on the box.
    pub name: &'static str,
    /// The family it belongs to.
    pub series: &'static str,
    /// How many input pairs have switchable phantom power.
    ///
    /// Zero means the model has none that software can reach — true of the
    /// whole 2nd generation and the Claretts, where the switch is physical.
    pub phantom_pairs: u8,
    /// The first input pair with phantom power, counted from zero.
    pub phantom_first: u8,
    /// How many inputs switch between line and instrument level.
    pub level_inputs: u8,
    /// The first of those, counted from zero.
    pub level_first: u8,
    /// How many inputs have a switchable pad.
    pub pad_inputs: u8,
    /// How many inputs have the "air" voicing.
    pub air_inputs: u8,
    /// How many inputs have software gain control.
    pub gain_inputs: u8,
    /// Its tables, oldest firmware first.
    pub tables: &'static [FirmwareTable],
}

impl Model {
    /// The table describing this model at `firmware`.
    ///
    /// Picks the newest table whose threshold `firmware` has reached. A
    /// version below every threshold gets the oldest table rather than
    /// nothing, because a device reporting an unexpectedly low version is
    /// likelier to be one this crate has not been taught about than one with
    /// no layout at all.
    #[must_use]
    pub fn table_for(&self, firmware: u16) -> Option<ConfigSet> {
        let mut chosen: Option<&FirmwareTable> = None;
        for candidate in self.tables {
            if firmware >= candidate.from_firmware
                && chosen.is_none_or(|best| candidate.from_firmware >= best.from_firmware)
            {
                chosen = Some(candidate);
            }
        }
        chosen
            .or_else(|| self.tables.first())
            .map(|entry| entry.table)
    }

    /// Whether software can switch phantom power on this model at all.
    #[must_use]
    pub const fn has_phantom_power(&self) -> bool {
        self.phantom_pairs > 0
    }
}

/// Every interface this crate knows.
pub const MODELS: &[Model] = &[
    Model {
        product_id: 0x8201,
        name: "Scarlett 18i20 2nd Gen",
        series: "Scarlett Gen 2",
        phantom_pairs: 0,
        phantom_first: 0,
        level_inputs: 0,
        level_first: 0,
        pad_inputs: 0,
        air_inputs: 0,
        gain_inputs: 0,
        tables: &[FirmwareTable {
            from_firmware: 0,
            table: tables::GEN2B,
        }],
    },
    Model {
        product_id: 0x8203,
        name: "Scarlett 6i6 2nd Gen",
        series: "Scarlett Gen 2",
        phantom_pairs: 0,
        phantom_first: 0,
        level_inputs: 2,
        level_first: 0,
        pad_inputs: 2,
        air_inputs: 0,
        gain_inputs: 0,
        tables: &[FirmwareTable {
            from_firmware: 0,
            table: tables::GEN2A,
        }],
    },
    Model {
        product_id: 0x8204,
        name: "Scarlett 18i8 2nd Gen",
        series: "Scarlett Gen 2",
        phantom_pairs: 0,
        phantom_first: 0,
        level_inputs: 2,
        level_first: 0,
        pad_inputs: 4,
        air_inputs: 0,
        gain_inputs: 0,
        tables: &[FirmwareTable {
            from_firmware: 0,
            table: tables::GEN2A,
        }],
    },
    Model {
        product_id: 0x8206,
        name: "Clarett USB 2Pre",
        series: "Clarett USB",
        phantom_pairs: 0,
        phantom_first: 0,
        level_inputs: 2,
        level_first: 0,
        pad_inputs: 0,
        air_inputs: 2,
        gain_inputs: 0,
        tables: &[FirmwareTable {
            from_firmware: 0,
            table: tables::CLARETT,
        }],
    },
    Model {
        product_id: 0x8207,
        name: "Clarett USB 4Pre",
        series: "Clarett USB",
        phantom_pairs: 0,
        phantom_first: 0,
        level_inputs: 2,
        level_first: 0,
        pad_inputs: 0,
        air_inputs: 4,
        gain_inputs: 0,
        tables: &[FirmwareTable {
            from_firmware: 0,
            table: tables::CLARETT,
        }],
    },
    Model {
        product_id: 0x8208,
        name: "Clarett USB 8Pre",
        series: "Clarett USB",
        phantom_pairs: 0,
        phantom_first: 0,
        level_inputs: 2,
        level_first: 0,
        pad_inputs: 0,
        air_inputs: 8,
        gain_inputs: 0,
        tables: &[FirmwareTable {
            from_firmware: 0,
            table: tables::CLARETT,
        }],
    },
    Model {
        product_id: 0x820a,
        name: "Clarett+ 2Pre",
        series: "Clarett+",
        phantom_pairs: 0,
        phantom_first: 0,
        level_inputs: 2,
        level_first: 0,
        pad_inputs: 0,
        air_inputs: 2,
        gain_inputs: 0,
        tables: &[FirmwareTable {
            from_firmware: 0,
            table: tables::CLARETT,
        }],
    },
    Model {
        product_id: 0x820b,
        name: "Clarett+ 4Pre",
        series: "Clarett+",
        phantom_pairs: 0,
        phantom_first: 0,
        level_inputs: 2,
        level_first: 0,
        pad_inputs: 0,
        air_inputs: 4,
        gain_inputs: 0,
        tables: &[FirmwareTable {
            from_firmware: 0,
            table: tables::CLARETT,
        }],
    },
    Model {
        product_id: 0x820c,
        name: "Clarett+ 8Pre",
        series: "Clarett+",
        phantom_pairs: 0,
        phantom_first: 0,
        level_inputs: 2,
        level_first: 0,
        pad_inputs: 0,
        air_inputs: 8,
        gain_inputs: 0,
        tables: &[FirmwareTable {
            from_firmware: 0,
            table: tables::CLARETT,
        }],
    },
    Model {
        product_id: 0x8210,
        name: "Scarlett 2i2 3rd Gen",
        series: "Scarlett Gen 3",
        phantom_pairs: 1,
        phantom_first: 0,
        level_inputs: 2,
        level_first: 0,
        pad_inputs: 0,
        air_inputs: 2,
        gain_inputs: 0,
        tables: &[FirmwareTable {
            from_firmware: 0,
            table: tables::GEN3A,
        }],
    },
    Model {
        product_id: 0x8211,
        name: "Scarlett Solo 3rd Gen",
        series: "Scarlett Gen 3",
        phantom_pairs: 1,
        phantom_first: 0,
        level_inputs: 1,
        level_first: 1,
        pad_inputs: 0,
        air_inputs: 1,
        gain_inputs: 0,
        tables: &[FirmwareTable {
            from_firmware: 0,
            table: tables::GEN3A,
        }],
    },
    Model {
        product_id: 0x8212,
        name: "Scarlett 4i4 3rd Gen",
        series: "Scarlett Gen 3",
        phantom_pairs: 1,
        phantom_first: 0,
        level_inputs: 2,
        level_first: 0,
        pad_inputs: 2,
        air_inputs: 2,
        gain_inputs: 0,
        tables: &[FirmwareTable {
            from_firmware: 0,
            table: tables::GEN3B,
        }],
    },
    Model {
        product_id: 0x8213,
        name: "Scarlett 8i6 3rd Gen",
        series: "Scarlett Gen 3",
        phantom_pairs: 1,
        phantom_first: 0,
        level_inputs: 2,
        level_first: 0,
        pad_inputs: 2,
        air_inputs: 2,
        gain_inputs: 0,
        tables: &[FirmwareTable {
            from_firmware: 0,
            table: tables::GEN3B,
        }],
    },
    Model {
        product_id: 0x8214,
        name: "Scarlett 18i8 3rd Gen",
        series: "Scarlett Gen 3",
        phantom_pairs: 2,
        phantom_first: 0,
        level_inputs: 2,
        level_first: 0,
        pad_inputs: 4,
        air_inputs: 4,
        gain_inputs: 0,
        tables: &[FirmwareTable {
            from_firmware: 0,
            table: tables::GEN3C,
        }],
    },
    Model {
        product_id: 0x8215,
        name: "Scarlett 18i20 3rd Gen",
        series: "Scarlett Gen 3",
        phantom_pairs: 2,
        phantom_first: 0,
        level_inputs: 2,
        level_first: 0,
        pad_inputs: 8,
        air_inputs: 8,
        gain_inputs: 0,
        tables: &[FirmwareTable {
            from_firmware: 0,
            table: tables::GEN3C,
        }],
    },
    Model {
        product_id: 0x8216,
        name: "Vocaster One",
        series: "Vocaster",
        phantom_pairs: 1,
        phantom_first: 0,
        level_inputs: 0,
        level_first: 0,
        pad_inputs: 0,
        air_inputs: 0,
        gain_inputs: 1,
        tables: &[FirmwareTable {
            from_firmware: 1769,
            table: tables::VOCASTER,
        }],
    },
    Model {
        product_id: 0x8217,
        name: "Vocaster Two",
        series: "Vocaster",
        phantom_pairs: 2,
        phantom_first: 0,
        level_inputs: 0,
        level_first: 0,
        pad_inputs: 0,
        air_inputs: 0,
        gain_inputs: 2,
        tables: &[FirmwareTable {
            from_firmware: 1769,
            table: tables::VOCASTER,
        }],
    },
    Model {
        product_id: 0x8218,
        name: "Scarlett Solo 4th Gen",
        series: "Scarlett Gen 4",
        phantom_pairs: 1,
        phantom_first: 1,
        level_inputs: 1,
        level_first: 0,
        pad_inputs: 0,
        air_inputs: 1,
        gain_inputs: 0,
        tables: &[
            FirmwareTable {
                from_firmware: 2115,
                table: tables::GEN4_SOLO,
            },
            FirmwareTable {
                from_firmware: 2417,
                table: tables::GEN4_SOLO_2417,
            },
        ],
    },
    Model {
        product_id: 0x8219,
        name: "Scarlett 2i2 4th Gen",
        series: "Scarlett Gen 4",
        phantom_pairs: 1,
        phantom_first: 0,
        level_inputs: 2,
        level_first: 0,
        pad_inputs: 0,
        air_inputs: 2,
        gain_inputs: 2,
        tables: &[
            FirmwareTable {
                from_firmware: 2115,
                table: tables::GEN4_2I2,
            },
            FirmwareTable {
                from_firmware: 2417,
                table: tables::GEN4_2I2_2417,
            },
        ],
    },
    Model {
        product_id: 0x821a,
        name: "Scarlett 4i4 4th Gen",
        series: "Scarlett Gen 4",
        phantom_pairs: 2,
        phantom_first: 0,
        level_inputs: 2,
        level_first: 0,
        pad_inputs: 0,
        air_inputs: 2,
        gain_inputs: 2,
        tables: &[
            FirmwareTable {
                from_firmware: 2089,
                table: tables::GEN4_4I4,
            },
            FirmwareTable {
                from_firmware: 2417,
                table: tables::GEN4_4I4_2417,
            },
        ],
    },
];

/// The model with this product id, if it is one of Focusrite's.
///
/// Takes the vendor id too rather than trusting the product id alone: product
/// ids are only unique within a vendor, and 0x8210 belongs to somebody else
/// entirely under a different one.
#[must_use]
pub fn find(vendor_id: u16, product_id: u16) -> Option<&'static Model> {
    if vendor_id != VENDOR_ID {
        return None;
    }
    MODELS.iter().find(|model| model.product_id == product_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ConfigItem;

    #[test]
    fn a_known_interface_is_found_by_its_pair_of_ids() {
        let found = find(VENDOR_ID, 0x8214).expect("the 18i8 3rd Gen");
        assert_eq!(found.name, "Scarlett 18i8 3rd Gen");
        assert_eq!(found.phantom_pairs, 2);
    }

    #[test]
    fn another_vendor_sharing_a_product_id_is_not_a_scarlett() {
        // Product ids are unique per vendor, not globally.
        assert!(find(0x046D, 0x8214).is_none());
    }

    #[test]
    fn the_firmware_version_chooses_between_two_layouts() {
        // The 4i4 4th Gen moved its addresses at firmware 2417. Reading the
        // old layout on new firmware would write real settings to the wrong
        // places, so this is the lookup that has to be right.
        let model = find(VENDOR_ID, 0x821A).expect("the 4i4 4th Gen");
        assert!(model.tables.len() > 1, "this model has two layouts");
        let old = model
            .table_for(2089)
            .expect("a table for the older firmware");
        let new = model
            .table_for(2417)
            .expect("a table for the newer firmware");
        assert_ne!(old, new, "the two firmware versions do not share a layout");
        assert_eq!(
            model.table_for(2500).expect("newer still"),
            new,
            "a version above every threshold takes the newest layout"
        );
    }

    #[test]
    fn a_firmware_version_below_every_threshold_still_gets_a_layout() {
        // Being wrong in the safe direction: an address that has moved reads
        // as a setting the interface does not have, rather than as a different
        // setting written by mistake.
        let model = find(VENDOR_ID, 0x821A).expect("the 4i4 4th Gen");
        assert!(model.table_for(0).is_some());
    }

    #[test]
    fn a_model_claiming_phantom_power_has_somewhere_to_write_it() {
        // The two halves come from different tables and could disagree. If
        // they did, a host would offer a control it could not carry out.
        for model in MODELS {
            let table = model.table_for(u16::MAX).expect("every model has a table");
            assert_eq!(
                model.has_phantom_power(),
                table.has(ConfigItem::PhantomSwitch),
                "{} disagrees with its own table about phantom power",
                model.name
            );
        }
    }

    #[test]
    fn the_second_generation_has_no_software_phantom_power() {
        // Its switch is physical. Offering one in software would be inventing
        // a control that does not exist.
        for pid in [0x8201, 0x8203, 0x8204] {
            let model = find(VENDOR_ID, pid).expect("a 2nd Gen interface");
            assert!(!model.has_phantom_power(), "{}", model.name);
        }
    }

    #[test]
    fn no_two_models_share_a_product_id() {
        // `find` takes the first match, so a duplicate would shadow whichever
        // came second and there would be nothing in the answer to see it by.
        let mut ids: Vec<u16> = MODELS.iter().map(|model| model.product_id).collect();
        let before = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), before, "two models share a product id");
    }

    #[test]
    fn the_two_clarett_families_are_named_apart() {
        // Same internals, different boxes, different product ids. Someone
        // reading the name aloud needs to hear which one they have.
        assert_eq!(
            find(VENDOR_ID, 0x8206).expect("Clarett USB 2Pre").name,
            "Clarett USB 2Pre"
        );
        assert_eq!(
            find(VENDOR_ID, 0x820A).expect("Clarett+ 2Pre").name,
            "Clarett+ 2Pre"
        );
    }
}
