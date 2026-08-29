//! What this build can do with a device, once it knows what the device is.

use roadie_device_registry::LOGITECH_VENDOR_ID;
use roadie_device_registry::litra::find_litra;
use roadie_device_registry::receiver::{ReceiverBrand, find_receiver};
use roadie_streamdeck::model::identify as identify_deck;
use roadie_via::identity::is_via_collection;

use crate::hidpp::is_long_collection;
use crate::identity::Identity;

/// A driver compiled into this build.
///
/// Deliberately a closed enum rather than a string: a device survey that can
/// name a driver which does not exist would be worse than one that admits it
/// knows nothing, because it sends someone looking for a command that is not
/// there. Adding a driver here is a compile-time change, so the list cannot
/// promise support the binary does not carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Driver {
    /// Logitech's HID++ protocol: mice, keyboards, and their controls.
    HidPlusPlus,
    /// Logitech's Litra lights.
    Litra,
    /// Elgato Stream Decks and the Stream Deck Pedal.
    StreamDeck,
    /// Any UVC webcam, whoever made it.
    Uvc,
    /// Any QMK keyboard or macro pad with VIA enabled.
    Via,
}

impl Driver {
    /// Stable identifier, for scripts and for `--json` output.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::HidPlusPlus => "hidpp",
            Self::Litra => "litra",
            Self::StreamDeck => "streamdeck",
            Self::Uvc => "uvc",
            Self::Via => "via",
        }
    }

    /// What this driver actually lets you change, in one line.
    ///
    /// Written to be read aloud: the point of the survey is that someone who
    /// cannot see the screen learns what each device offers without opening
    /// anything.
    #[must_use]
    pub const fn what_it_configures(self) -> &'static str {
        match self {
            Self::HidPlusPlus => "buttons, pointer speed, scroll wheel, and backlight",
            Self::Litra => "power, brightness, and colour temperature",
            Self::StreamDeck => "key images, labels, brightness, and key actions",
            Self::Uvc => "brightness, contrast, exposure, focus, and zoom",
            Self::Via => "what each key sends, across every keymap layer",
        }
    }

    /// The command that configures a device this driver handles.
    #[must_use]
    pub const fn command(self) -> &'static str {
        match self {
            Self::HidPlusPlus => "roadie list",
            Self::Litra => "roadie light",
            Self::StreamDeck => "roadie streamdeck",
            Self::Uvc => "roadie camera",
            Self::Via => "roadie via",
        }
    }
}

/// What this build can do with one device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Support {
    /// A driver in this build configures it.
    Driver {
        /// The driver that handles it.
        driver: Driver,
        /// The model, when a catalog entry names one.
        model: Option<&'static str>,
    },
    /// Probably drivable, but only a check against the device can say.
    ///
    /// Some devices are identified by the HID collection they expose rather
    /// than by a catalog entry, and a usage page is a convention rather than
    /// a claim the device makes — other firmware uses the same vendor page. A
    /// survey that enumerates without opening anything can honestly report a
    /// candidate and no more.
    ///
    /// Reporting these as supported would be the worse error of the two: it
    /// promises a device works and leaves someone to discover otherwise.
    Candidate {
        /// The driver that would handle it, if the check passes.
        driver: Driver,
        /// What has to be true, in words fit to read aloud.
        needs: &'static str,
    },
    /// A Logitech receiver: a way in, not a peripheral.
    ///
    /// Reported distinctly because listing it as an unsupported device would
    /// be actively wrong — it is fully supported, and the things it supports
    /// are the mice and keyboards paired to it, which appear in their own
    /// right. Someone reading "Unifying receiver: unsupported" would
    /// reasonably conclude their mouse was not going to work.
    Receiver(ReceiverBrand),
    /// Detected, and nothing in this build drives it.
    ///
    /// Never omitted from a listing. A device you own that the hub cannot
    /// configure is exactly what you need told, and it is also how the list of
    /// what to support next gets written.
    Unsupported,
}

impl Support {
    /// Whether this build can configure the device.
    #[must_use]
    pub const fn is_configurable(&self) -> bool {
        matches!(self, Self::Driver { .. })
    }
}

/// One thing plugged in, and what can be done with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Peripheral {
    /// Who and what the OS says it is.
    pub identity: Identity,
    /// What this build can do with it.
    pub support: Support,
}

impl Peripheral {
    /// Classify a HID node.
    ///
    /// A single peripheral exposes several HID collections and the verdict can
    /// differ between them — only one collection of a Logitech mouse carries
    /// HID++ — so this answers for the collection it is given, and merging the
    /// collections of one device is the caller's job.
    #[must_use]
    pub fn from_hid(identity: Identity, usage_page: u16, usage_id: u16) -> Self {
        let support = classify_hid(&identity, usage_page, usage_id);
        Self { identity, support }
    }

    /// Classify a camera.
    ///
    /// Every camera the OS reports as a capture device is a UVC camera, and
    /// UVC is a class standard rather than a per-vendor protocol: the same
    /// brightness, exposure, focus and zoom registers answer on an Elgato
    /// Facecam, an Obsbot, and a laptop's built-in camera. So there is nothing
    /// to look up — a camera is supported because of what it is, not because
    /// of who made it.
    #[must_use]
    pub fn from_camera(identity: Identity) -> Self {
        Self {
            identity,
            support: Support::Driver {
                driver: Driver::Uvc,
                model: None,
            },
        }
    }

    /// Keep whichever verdict says more, when one device was seen twice.
    ///
    /// Merging collections of one physical device must not lose the supported
    /// verdict just because the unsupported collection came second: a Logitech
    /// mouse enumerates a plain mouse collection alongside its HID++ one, and
    /// which arrives first is not something we control.
    ///
    /// When the verdicts say the same amount, the *name* still has to be
    /// chosen, and the collections of one device do not always agree on it —
    /// "Stream Deck" and "Stream Deck (Consumer Control)" are the same device
    /// introducing itself twice. Keeping whichever arrived first would make
    /// the listed name depend on enumeration order, so a desk could read
    /// differently between two runs on the same machine. The survey promises
    /// the opposite, and a promise kept only while the OS happens to be
    /// consistent is not one.
    ///
    /// So a tie is broken on the name itself: the one that sorts first. That
    /// is arbitrary, and it is arbitrary the same way every time, which is
    /// the property being bought.
    #[must_use]
    pub fn merge(self, other: Self) -> Self {
        match rank(&other.support).cmp(&rank(&self.support)) {
            std::cmp::Ordering::Greater => other,
            std::cmp::Ordering::Less => self,
            std::cmp::Ordering::Equal => {
                if other.identity.describe() < self.identity.describe() {
                    other
                } else {
                    self
                }
            }
        }
    }
}

/// How much a verdict says, for choosing between two of them.
///
/// Merging the collections of one physical device must keep the one that says
/// the most: a Logitech mouse enumerates a plain mouse collection alongside
/// its HID++ one, and which arrives first is not something we control.
const fn rank(support: &Support) -> u8 {
    match support {
        Support::Unsupported => 0,
        Support::Candidate { .. } => 1,
        Support::Receiver(_) => 2,
        Support::Driver { .. } => 3,
    }
}

/// The verdict for one HID collection.
fn classify_hid(identity: &Identity, usage_page: u16, usage_id: u16) -> Support {
    let (vendor, product) = (identity.vendor_id, identity.product_id);

    if let Some(deck) = identify_deck(vendor, product) {
        return Support::Driver {
            driver: Driver::StreamDeck,
            model: Some(deck.name),
        };
    }

    // Checked before HID++, not after: a Litra deliberately uses the same
    // vendor collection as Logitech's HID++ peripherals, so the more specific
    // match has to win or every Litra would be reported as a mouse.
    if let Some(litra) = find_litra(vendor, product, usage_page, usage_id) {
        return Support::Driver {
            driver: Driver::Litra,
            model: Some(litra.registry_model_id),
        };
    }

    if let Some(receiver) = find_receiver(vendor, product) {
        return Support::Receiver(receiver.brand);
    }

    // A candidate rather than a verdict: the vendor usage page VIA uses is
    // shared with other firmware, so only a protocol handshake proves one.
    if is_via_collection(usage_page, usage_id) {
        return Support::Candidate {
            driver: Driver::Via,
            needs: "a VIA protocol check, which `roadie via list` performs",
        };
    }

    if vendor == LOGITECH_VENDOR_ID && is_long_collection(usage_page, usage_id) {
        return Support::Driver {
            driver: Driver::HidPlusPlus,
            model: None,
        };
    }

    Support::Unsupported
}

#[cfg(test)]
mod tests {
    use roadie_device_registry::LOGITECH_VENDOR_ID;
    use roadie_device_registry::litra::{LITRA_GLOW_PRODUCT_ID, LITRA_USAGE_ID, LITRA_USAGE_PAGE};
    use roadie_device_registry::receiver::ReceiverBrand;
    use roadie_streamdeck::model::ELGATO_VENDOR_ID;

    use super::{Driver, Identity, Peripheral, Support};

    fn identity(vendor_id: u16, product_id: u16) -> Identity {
        Identity {
            vendor_id,
            product_id,
            ..Identity::default()
        }
    }

    #[test]
    fn a_stream_deck_is_named_by_its_catalog_entry() {
        let found = Peripheral::from_hid(identity(ELGATO_VENDOR_ID, 0x0080), 0xff00, 0x0001);
        assert_eq!(
            found.support,
            Support::Driver {
                driver: Driver::StreamDeck,
                model: Some("Stream Deck MK.2"),
            }
        );
    }

    /// The one ordering in this function that is not arbitrary. A Litra
    /// answers on the same vendor collection HID++ uses, so checking HID++
    /// first would report every Litra as a mouse and offer the wrong commands.
    #[test]
    fn a_litra_is_a_light_not_a_mouse() {
        let found = Peripheral::from_hid(
            identity(LOGITECH_VENDOR_ID, LITRA_GLOW_PRODUCT_ID),
            LITRA_USAGE_PAGE,
            LITRA_USAGE_ID,
        );
        assert_eq!(
            found.support,
            Support::Driver {
                driver: Driver::Litra,
                model: Some("8c900"),
            }
        );
    }

    #[test]
    fn a_logitech_hidpp_collection_is_configurable() {
        let found = Peripheral::from_hid(identity(LOGITECH_VENDOR_ID, 0x4082), 0xff00, 0x0002);
        assert_eq!(
            found.support,
            Support::Driver {
                driver: Driver::HidPlusPlus,
                model: None,
            }
        );
    }

    /// The plain mouse collection of a Logitech mouse. Not a bug — the device
    /// is still supported through its other collection, which is exactly why
    /// `merge` exists.
    #[test]
    fn a_logitech_non_hidpp_collection_is_not_configurable_on_its_own() {
        let found = Peripheral::from_hid(identity(LOGITECH_VENDOR_ID, 0x4082), 0x0001, 0x0002);
        assert_eq!(found.support, Support::Unsupported);
    }

    #[test]
    fn a_receiver_is_reported_as_a_receiver_not_as_unsupported() {
        let found = Peripheral::from_hid(identity(LOGITECH_VENDOR_ID, 0xc52b), 0xff00, 0x0002);
        assert_eq!(found.support, Support::Receiver(ReceiverBrand::Unifying));
    }

    #[test]
    fn an_unknown_device_is_reported_rather_than_dropped() {
        let found = Peripheral::from_hid(identity(0x1234, 0x5678), 0x000c, 0x0001);
        assert_eq!(found.support, Support::Unsupported);
        assert!(!found.support.is_configurable());
    }

    #[test]
    fn every_camera_is_supported_whoever_made_it() {
        for vendor in [LOGITECH_VENDOR_ID, ELGATO_VENDOR_ID, 0x1234] {
            let found = Peripheral::from_camera(identity(vendor, 0x0001));
            assert_eq!(
                found.support,
                Support::Driver {
                    driver: Driver::Uvc,
                    model: None,
                },
                "a UVC camera from {vendor:#06x} should be configurable"
            );
        }
    }

    /// The ordering hazard: a mouse's plain collection can be enumerated
    /// before its HID++ one, and merging must not let the later, emptier
    /// verdict win.
    #[test]
    fn merging_keeps_the_verdict_that_says_more_in_either_order() {
        let plain = Peripheral::from_hid(identity(LOGITECH_VENDOR_ID, 0x4082), 0x0001, 0x0002);
        let rich = Peripheral::from_hid(identity(LOGITECH_VENDOR_ID, 0x4082), 0xff00, 0x0002);
        assert!(
            plain.clone().merge(rich.clone()).support.is_configurable(),
            "unsupported first"
        );
        assert!(
            rich.merge(plain).support.is_configurable(),
            "supported first"
        );
    }

    /// The honest answer for a device identified only by the collection it
    /// exposes. Calling it supported would promise a device works and leave
    /// someone to find out otherwise.
    #[test]
    fn a_via_collection_is_a_candidate_not_a_promise() {
        let found = Peripheral::from_hid(identity(0x4653, 0x0001), 0xff60, 0x0061);
        let Support::Candidate { driver, needs } = found.support else {
            panic!("a VIA collection is a candidate: {:?}", found.support);
        };
        assert_eq!(driver, Driver::Via);
        assert!(needs.contains("roadie via"), "{needs}");
        assert!(
            !found.support.is_configurable(),
            "a candidate has not been confirmed"
        );
    }

    /// A device introduces itself differently on different collections, and
    /// which one the OS hands over first is not ours to control. If the tie
    /// went to whoever arrived first, the same desk would read differently
    /// between two runs — which is the exact promise the survey makes.
    #[test]
    fn two_names_for_one_device_merge_to_the_same_name_either_way_round() {
        let plain = Peripheral {
            identity: Identity {
                product: Some("Stream Deck".to_owned()),
                ..identity(ELGATO_VENDOR_ID, 0x0080)
            },
            support: Support::Driver {
                driver: Driver::StreamDeck,
                model: Some("Stream Deck MK.2"),
            },
        };
        let qualified = Peripheral {
            identity: Identity {
                product: Some("Stream Deck (Consumer Control)".to_owned()),
                ..identity(ELGATO_VENDOR_ID, 0x0080)
            },
            support: Support::Driver {
                driver: Driver::StreamDeck,
                model: Some("Stream Deck MK.2"),
            },
        };

        let forwards = plain.clone().merge(qualified.clone());
        let backwards = qualified.merge(plain);
        assert_eq!(
            forwards.identity.describe(),
            backwards.identity.describe(),
            "the name a device is listed under must not depend on enumeration order"
        );
    }

    /// The whole ordering, every pair, both ways round.
    ///
    /// `merge` exists so that which collection the OS enumerated first cannot
    /// decide what a device is. That guarantee is the ordering being *total* —
    /// any two verdicts having a definite winner — and the three cases a real
    /// desk happens to produce do not pin that. A tie introduced later would
    /// make merge order-dependent again for the pair that tied, silently, on
    /// the one property this function exists to provide.
    #[test]
    fn the_ordering_of_verdicts_is_total_and_has_no_ties() {
        // Weakest first. Each says strictly more than the one before it.
        let ladder = [
            Support::Unsupported,
            Support::Candidate {
                driver: Driver::Via,
                needs: "a check",
            },
            Support::Receiver(ReceiverBrand::Unifying),
            Support::Driver {
                driver: Driver::HidPlusPlus,
                model: None,
            },
        ];

        for (weaker_at, weaker) in ladder.iter().enumerate() {
            for (stronger_at, stronger) in ladder.iter().enumerate() {
                let left = Peripheral {
                    identity: identity(0x046d, 0x4082),
                    support: weaker.clone(),
                };
                let right = Peripheral {
                    identity: identity(0x046d, 0x4082),
                    support: stronger.clone(),
                };
                let expected = if stronger_at > weaker_at {
                    stronger
                } else {
                    weaker
                };
                // Both orders must reach the same verdict; that is the point.
                assert_eq!(
                    &left.clone().merge(right.clone()).support,
                    expected,
                    "merging {weaker:?} then {stronger:?} chose wrongly"
                );
                assert_eq!(
                    &right.merge(left).support,
                    expected,
                    "merging {stronger:?} then {weaker:?} chose wrongly"
                );
            }
        }
    }

    /// A candidate says more than nothing and less than a driver. Merging has
    /// to respect that in both directions, or the collection that arrived
    /// second decides what a device is.
    #[test]
    fn a_candidate_outranks_nothing_and_is_outranked_by_a_driver() {
        let candidate = Peripheral::from_hid(identity(0x4653, 0x0001), 0xff60, 0x0061);
        let nothing = Peripheral::from_hid(identity(0x4653, 0x0001), 0x0001, 0x0006);
        let driver = Peripheral::from_hid(identity(ELGATO_VENDOR_ID, 0x0080), 0xff00, 0x0001);

        assert!(matches!(
            candidate.clone().merge(nothing.clone()).support,
            Support::Candidate { .. }
        ));
        assert!(matches!(
            nothing.merge(candidate.clone()).support,
            Support::Candidate { .. }
        ));
        assert!(
            candidate
                .clone()
                .merge(driver.clone())
                .support
                .is_configurable()
        );
        assert!(driver.merge(candidate).support.is_configurable());
    }

    #[test]
    fn every_driver_has_a_distinct_id_and_a_command() {
        let drivers = [
            Driver::HidPlusPlus,
            Driver::Litra,
            Driver::StreamDeck,
            Driver::Uvc,
            Driver::Via,
        ];
        for (index, driver) in drivers.iter().enumerate() {
            assert!(!driver.what_it_configures().is_empty());
            assert!(driver.command().starts_with("roadie "));
            assert!(
                drivers[..index]
                    .iter()
                    .all(|other| other.id() != driver.id()),
                "{} is used twice",
                driver.id()
            );
        }
    }
}
