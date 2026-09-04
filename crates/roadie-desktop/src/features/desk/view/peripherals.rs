//! Drawing the two families that are identity and nothing else.
//!
//! Neither a TourBox nor a VIA board has settings this panel can change, and
//! both are drawn anyway. A device that is plugged in and working should
//! appear on a panel called "the desk" — leaving it off would mean somebody
//! checking whether the app can see their controller finds an empty space and
//! concludes it cannot.
//!
//! What each card does instead is say where its settings actually live, so the
//! absence of knobs reads as a signpost rather than as something missing.

use gpui::{IntoElement, ParentElement, Styled, div, prelude::FluentBuilder as _};
use roadie_ipc::desk::{ControllerSummary, MacroPadSummary};

use super::DeskPanel;
use crate::ui::theme::{Palette, Typography as _};

impl DeskPanel {
    /// One TourBox: what it is, and where its buttons are configured.
    pub(super) fn controller_card(
        controller: &ControllerSummary,
        pal: Palette,
    ) -> impl IntoElement {
        Self::card(pal)
            .child(div().text_body().child(controller.name.clone()))
            .child(div().text_caption().text_color(pal.text_muted).child(tr!(
                "%{buttons} buttons and %{wheels} wheels",
                buttons = controller.buttons,
                wheels = controller.wheels
            )))
            .when(controller.haptics, |card| {
                card.child(
                    div()
                        .text_caption()
                        .text_color(pal.text_muted)
                        .child(tr!("It can buzz.")),
                )
            })
            .child(div().text_caption().text_color(pal.text_muted).child(tr!(
                "A controller stores no settings of its own. What each button and wheel \
                         does is set in your configuration file."
            )))
    }

    /// One VIA board: what it is, and what its handshake said.
    pub(super) fn pad_card(pad: &MacroPadSummary, pal: Palette) -> impl IntoElement {
        Self::card(pal)
            .child(div().text_body().child(pad.name.clone()))
            .when(!pad.reachable, |card| {
                card.when_some(pad.unreachable_reason.clone(), |card, why| {
                    card.child(div().text_caption().text_color(pal.text_muted).child(why))
                })
            })
            .when(pad.reachable, |card| {
                card.child(div().text_caption().text_color(pal.text_muted).child(tr!(
                    "Speaks VIA protocol %{protocol}, with %{layers} keymap layers.",
                    protocol = pad.protocol,
                    layers = pad.layers
                )))
                .child(div().text_caption().text_color(pal.text_muted).child(tr!(
                    "Reading and changing its keymap is done from the command line for \
                             now."
                )))
            })
    }
}
