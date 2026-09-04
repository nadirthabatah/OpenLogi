//! Drawing a monitor.

use gpui::{
    AppContext as _, Context, Entity, IntoElement, ParentElement, Styled, Window, div,
    prelude::FluentBuilder as _,
};
use gpui_component::{
    h_flex,
    slider::{Slider, SliderEvent, SliderState},
    v_flex,
};
use roadie_ipc::desk::{DisplayControl, DisplayReading, DisplaySummary};

use super::{DeskPanel, Slot, WriteKey};
use crate::features::desk::model::describe_value;
use crate::services::ipc::{Command, DeskCommand};
use crate::state::AppState;
use crate::ui::theme::{Palette, Typography as _};

impl DeskPanel {
    /// Write one monitor control and take back what the monitor then reads.
    fn set_display(
        &mut self,
        id: String,
        control: DisplayControl,
        value: u16,
        cx: &mut Context<Self>,
    ) {
        let Some(sender) = AppState::try_read(cx).map(AppState::ipc_sender) else {
            return;
        };
        let key = WriteKey::Display(id.clone(), control);
        let task = cx.spawn(async move |panel, cx| {
            let (reply, answer) = tokio::sync::oneshot::channel();
            if sender
                .send(Command::Desk(DeskCommand::SetDisplay(
                    id.clone(),
                    control,
                    value,
                    reply,
                )))
                .is_err()
            {
                return;
            }
            if let Ok(Ok(reading)) = answer.await {
                let _ = panel.update(cx, |panel, cx| {
                    panel.model.apply_reading(&id, reading);
                    cx.notify();
                });
            }
        });
        // Inserting replaces, and dropping the old task cancels it.
        self.writes.insert(key, task);
    }

    /// The slider for one monitor control, made on first sight of it and moved
    /// to match the device whenever the device disagrees with it.
    fn display_slider(
        &mut self,
        id: &str,
        reading: DisplayReading,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<SliderState> {
        let key = (id.to_owned(), reading.control);
        if let Some(existing) = self.display_sliders.get_mut(&key) {
            if existing.shown != reading.current {
                // The monitor took something other than what it was handed —
                // clamped to its own maximum, or rounded. The thumb follows the
                // monitor, because the monitor is what is true.
                existing.shown = reading.current;
                let state = existing.state.clone();
                state.update(cx, |slider, cx| {
                    slider.set_value(f32::from(reading.current), window, cx);
                });
                return state;
            }
            return existing.state.clone();
        }
        let maximum = f32::from(reading.maximum.max(1));
        let slider = cx.new(|_| {
            SliderState::new()
                .min(0.0)
                .max(maximum)
                .step(1.0)
                .default_value(f32::from(reading.current))
        });
        let handle = id.to_owned();
        let control = reading.control;
        let subscription = cx.subscribe(&slider, move |panel, _slider, event: &SliderEvent, cx| {
            // Release, not change: a monitor answers a DDC write in tens of
            // milliseconds, and writing on every pixel of a drag would queue
            // hundreds of exchanges behind a person's thumb.
            if let SliderEvent::Release(value) = event {
                let asked = value.start().round().clamp(0.0, f32::from(u16::MAX));
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "clamped to u16's range on the line above"
                )]
                let asked = asked as u16;
                panel.set_display(handle.clone(), control, asked, cx);
            }
        });
        self.subscriptions.push(subscription);
        self.display_sliders.insert(
            key,
            Slot {
                state: slider.clone(),
                shown: reading.current,
            },
        );
        slider
    }

    /// One monitor: its name, then its controls or the reason it has none.
    pub(super) fn display_card(
        &mut self,
        display: &DisplaySummary,
        pal: Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let readings: Vec<DisplayReading> = self.model.readings(&display.id).to_vec();
        let reason = display.unreachable_reason.clone();
        let rows: Vec<_> = readings
            .iter()
            .map(|reading| {
                let label = match reading.control {
                    DisplayControl::Brightness => tr!("Brightness"),
                    DisplayControl::Contrast => tr!("Contrast"),
                    DisplayControl::Volume => tr!("Volume"),
                    DisplayControl::Input => tr!("Input"),
                };
                let value = describe_value(*reading);
                // Input is read, never written — see this module's header.
                let slider = (reading.control != DisplayControl::Input)
                    .then(|| self.display_slider(&display.id, *reading, window, cx));
                v_flex()
                    .gap_1()
                    .child(
                        h_flex()
                            .justify_between()
                            .child(div().text_caption().child(label))
                            .child(div().text_caption().text_color(pal.text_muted).child(value)),
                    )
                    .when_some(slider, |row, state| {
                        row.child(Slider::new(&state).horizontal())
                    })
            })
            .collect();

        Self::card(pal)
            .child(div().text_body().child(display.name.clone()))
            .when_some(reason, |card, why| {
                card.child(div().text_caption().text_color(pal.text_muted).child(why))
            })
            .children(rows)
    }
}
