//! The one write here that can damage equipment.
//!
//! Phantom power sends 48 volts down the microphone cable. A condenser
//! microphone needs it; a **ribbon microphone can be damaged by it**, and so
//! can some vintage and passive designs. Nothing in the protocol says what is
//! plugged in, and nothing can — so the software cannot decide this, and the
//! person holding the microphone has to.
//!
//! That is the whole argument for the gate. It is the same shape as the one
//! guarding a monitor's power in `roadie-display`, and deliberately just as
//! narrow: **only switching phantom power on** carries a risk. Switching it
//! off does not, and neither does anything else on the interface. Treating
//! more of them as dangerous would train somebody to pass the confirmation
//! without reading it, which is how a real confirmation stops working.

/// A write that could damage something, and what it would cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Risk {
    /// Switching 48 V phantom power **on** for one input pair.
    PhantomPower {
        /// Which pair, counted the way the interface labels its inputs — from
        /// one, not from zero, because the number is going to be read aloud
        /// and compared against what is printed on the box.
        pair: u16,
    },
}

impl Risk {
    /// The risk in setting phantom power on `pair` to `enabled`, if any.
    ///
    /// `None` for switching it off: that is how somebody makes the interface
    /// safe again, and asking them to confirm it would be an obstacle in front
    /// of the safe direction.
    #[must_use]
    pub const fn of_phantom(pair: u16, enabled: bool) -> Option<Self> {
        if enabled {
            Some(Self::PhantomPower { pair })
        } else {
            None
        }
    }

    /// What this costs, written to be read aloud.
    ///
    /// One sentence for what happens, one for what it can damage, one for the
    /// way out. No symbols and no abbreviations that a screen reader would
    /// spell letter by letter.
    #[must_use]
    pub fn spoken(&self) -> String {
        match *self {
            Self::PhantomPower { pair } => format!(
                "This switches 48 volt phantom power on for input pair {pair}. A condenser \
                 microphone needs it, but a ribbon microphone can be damaged by it, and so can \
                 some older passive microphones. Unplug anything you are not sure about before \
                 switching this on."
            ),
        }
    }
}

impl std::fmt::Display for Risk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.spoken())
    }
}

/// Proof that whoever asked for a risky write was told what it costs.
///
/// No `Default`, no `From<bool>`, and no way to build one from a flag alone:
/// [`Acknowledged::of`] takes the [`Risk`] itself, so the call site has to
/// have the risk in hand — which means it has had the sentence in hand. A
/// `--yes` flag several functions away cannot conjure one.
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
    /// acknowledgement of one thing cannot be spent on another: agreeing to
    /// switch phantom power on for one pair does not authorise it on the next.
    #[must_use]
    pub const fn risk(self) -> Risk {
        self.0
    }
}

/// Whether `acknowledged` authorises `risk`.
///
/// Its own function so both front ends check it the same way, and so the
/// pair-by-pair rule has somewhere to be tested.
#[must_use]
pub fn authorises(acknowledged: Option<Acknowledged>, risk: Risk) -> bool {
    acknowledged.is_some_and(|given| given.risk() == risk)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switching_phantom_power_on_is_a_risk_and_switching_it_off_is_not() {
        // Off is how somebody makes the interface safe again. Putting a
        // confirmation in front of the safe direction would be an obstacle
        // exactly where there should not be one.
        assert_eq!(
            Risk::of_phantom(1, true),
            Some(Risk::PhantomPower { pair: 1 })
        );
        assert_eq!(Risk::of_phantom(1, false), None);
    }

    #[test]
    fn an_acknowledgement_for_one_pair_does_not_cover_another() {
        // The mistake this stops is a host that asks once and then applies the
        // answer to every input, switching phantom power on over a ribbon
        // microphone the person never agreed to.
        let first = Acknowledged::of(Risk::PhantomPower { pair: 1 });
        assert!(authorises(Some(first), Risk::PhantomPower { pair: 1 }));
        assert!(!authorises(Some(first), Risk::PhantomPower { pair: 2 }));
    }

    #[test]
    fn nothing_is_authorised_without_an_acknowledgement() {
        assert!(!authorises(None, Risk::PhantomPower { pair: 1 }));
    }

    #[test]
    fn the_warning_names_what_it_can_damage_and_how_to_avoid_it() {
        // A warning that only says "are you sure" teaches people to say yes.
        let said = Risk::PhantomPower { pair: 2 }.spoken();
        assert!(said.contains("ribbon"), "{said}");
        assert!(said.contains("48 volt"), "{said}");
        assert!(said.contains("Unplug"), "{said}");
        assert!(said.contains("pair 2"), "{said}");
    }

    #[test]
    fn the_warning_is_written_to_be_heard() {
        // No symbols, and nothing a screen reader would spell out letter by
        // letter. "48V" is read as "forty-eight vee"; "48 volt" is not.
        let said = Risk::PhantomPower { pair: 1 }.spoken();
        assert!(!said.contains("48V"), "{said}");
        assert!(
            !said.contains('/') && !said.contains('*') && !said.contains('('),
            "{said}"
        );
    }
}
