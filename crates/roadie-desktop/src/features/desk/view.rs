//! The monitors-and-lights panel.
//!
//! A thin drawing of [`DeskModel`]. Everything that decides what appears here
//! lives in that module, where it can be tested; this file is layout, the
//! slider entities, and the calls to the agent.
//!
//! Two controls the command line offers are deliberately missing.
//!
//! **Power** and **saving to the monitor's own memory** are absent for the
//! reason the wire types give: a monitor powered off over DDC may stop
//! answering DDC, and that memory wears out. Both belong behind a prompt that
//! says so.
//!
//! **Switching input** is absent for a reason of its own, which applies only
//! here. On the command line it is a deliberate typed sentence; in a panel it
//! is one mis-click, and the cost of the mistake is that the screen goes dark
//! and the way back is the same panel you can no longer see. So the current
//! input is shown and not offered — reading it is the useful half, and it is
//! the half that cannot go wrong.

use std::collections::BTreeMap;

use gpui::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement, Render, Styled,
    Subscription, Task, Window, div, prelude::FluentBuilder as _,
};
use gpui_component::{
    Disableable as _, Selectable as _, h_flex,
    slider::{Slider, SliderEvent, SliderState},
    v_flex,
};
use roadie_ipc::desk::{
    DisplayControl, DisplayReading, DisplaySummary, NetworkLightChange, NetworkLightSummary,
};

use super::model::{DeskModel, describe_value};
use crate::services::ipc::Command;
use crate::state::AppState;
use crate::ui::components::{Toggle, control_button};
use crate::ui::theme::{self, Palette, Typography as _};
use crate::windows::AuxWindow;

/// A light's two continuous controls, for keying its sliders apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum LightKnob {
    Brightness,
    Kelvin,
}

/// What a Key Light accepts, which is the light's own range rather than ours.
const LIGHT_BRIGHTNESS: (f32, f32) = (3.0, 100.0);
const LIGHT_KELVIN: (f32, f32) = (2900.0, 7000.0);

/// A slider, and the value it is currently showing.
///
/// The second half is the point. A slider is dragged to where a person let go
/// of it, and the device then answers with what it actually took — which is
/// often somewhere else, because a monitor clamps to its own maximum and a Key
/// Light has a brightness floor. Without remembering what the slider was last
/// told, there is nothing to compare the device's answer against, and the thumb
/// stays where the drag ended while the number beside it says something else.
struct Slot {
    state: Entity<SliderState>,
    shown: u16,
}

pub struct DeskPanel {
    /// Held so the window keeps following the OS light/dark setting; dropping
    /// it strands the window on whichever theme it opened in.
    appearance: Option<Subscription>,
    model: DeskModel,
    /// One slider per monitor control, keyed by the monitor's handle so two
    /// monitors cannot share a slider's state.
    display_sliders: BTreeMap<(String, DisplayControl), Slot>,
    /// The same for lights.
    light_sliders: BTreeMap<(String, LightKnob), Slot>,
    /// Held for their lifetime, not detached: a slider release that outlives
    /// this panel has nothing left to update.
    subscriptions: Vec<Subscription>,
    /// The two halves of the current scan. Replaced wholesale on a refresh,
    /// which cancels whatever the previous one was still waiting for — the
    /// answers would be dropped by the generation fence anyway, so waiting for
    /// them is only a slower way to ignore them.
    scans: Vec<Task<()>>,
    /// One in-flight write per thing that can be written.
    ///
    /// Keyed rather than accumulated for two reasons. It bounds the map by
    /// what is on screen instead of growing for the life of the window. And
    /// starting a second write to the same control cancels the first, which is
    /// what dragging a slider twice should mean: the newer value wins, and the
    /// older reply is not left to arrive afterwards and put the older number
    /// back.
    writes: BTreeMap<WriteKey, Task<()>>,
}

/// What a write is about, so a newer one can replace it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum WriteKey {
    /// One control on one monitor.
    Display(String, DisplayControl),
    /// One knob on one light.
    Light(String, LightKnob),
    /// A light's power, which is not a knob.
    LightPower(String),
}

impl DeskPanel {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut panel = Self {
            appearance: None,
            model: DeskModel::default(),
            display_sliders: BTreeMap::new(),
            light_sliders: BTreeMap::new(),
            subscriptions: Vec::new(),
            scans: Vec::new(),
            writes: BTreeMap::new(),
        };
        panel.refresh(cx);
        panel
    }

    /// Ask the agent what is on the desk.
    ///
    /// Both halves are asked for at once and answer independently, so a house
    /// with monitors and no lights does not wait the full multicast window
    /// before showing its monitors.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        let Some(sender) = AppState::try_read(cx).map(AppState::ipc_sender) else {
            return;
        };
        let generation = self.model.begin_scan();
        // Sliders belong to the handles of the scan that made them.
        self.display_sliders.clear();
        self.light_sliders.clear();
        self.subscriptions.clear();

        // Replacing the vector cancels the previous scan's tasks.
        self.scans = Vec::with_capacity(2);
        let displays = sender.clone();
        self.scans.push(cx.spawn(async move |panel, cx| {
            let (reply, answer) = tokio::sync::oneshot::channel();
            if displays.send(Command::ListDisplays(reply)).is_err() {
                return;
            }
            let Ok(found) = answer.await else { return };
            let ids: Vec<String> = found
                .iter()
                .filter(|display| display.reachable)
                .map(|display| display.id.clone())
                .collect();
            let accepted = panel
                .update(cx, |panel, cx| {
                    let taken = panel.model.accept_displays(generation, found);
                    cx.notify();
                    taken
                })
                .unwrap_or(false);
            if !accepted {
                return;
            }
            // Only monitors that answered the probe are asked for readings;
            // the rest would spend a DDC timeout each to say so twice.
            for id in ids {
                let (reply, answer) = tokio::sync::oneshot::channel();
                if displays.send(Command::ReadDisplay(id, reply)).is_err() {
                    return;
                }
                if let Ok(Ok(settings)) = answer.await {
                    let _ = panel.update(cx, |panel, cx| {
                        panel.model.accept_settings(generation, settings);
                        cx.notify();
                    });
                }
            }
            let _ = panel.update(cx, |panel, cx| {
                panel.model.finish_scan(generation);
                cx.notify();
            });
        }));

        self.scans.push(cx.spawn(async move |panel, cx| {
            let (reply, answer) = tokio::sync::oneshot::channel();
            if sender.send(Command::ListNetworkLights(reply)).is_err() {
                return;
            }
            let Ok(found) = answer.await else { return };
            let _ = panel.update(cx, |panel, cx| {
                panel.model.accept_lights(generation, found);
                cx.notify();
            });
        }));
    }

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
                .send(Command::SetDisplay(id.clone(), control, value, reply))
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

    /// Write one light and take back what it then reads.
    fn set_light(
        &mut self,
        id: String,
        change: NetworkLightChange,
        key: WriteKey,
        cx: &mut Context<Self>,
    ) {
        let Some(sender) = AppState::try_read(cx).map(AppState::ipc_sender) else {
            return;
        };
        let task = cx.spawn(async move |panel, cx| {
            let (reply, answer) = tokio::sync::oneshot::channel();
            if sender
                .send(Command::SetNetworkLight(id, change, reply))
                .is_err()
            {
                return;
            }
            if let Ok(Ok(updated)) = answer.await {
                let _ = panel.update(cx, |panel, cx| {
                    panel.model.apply_light(updated);
                    cx.notify();
                });
            }
        });
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

    /// The slider for one light control, kept in step with the light the same
    /// way.
    fn light_slider(
        &mut self,
        light: &NetworkLightSummary,
        knob: LightKnob,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<SliderState> {
        let key = (light.id.clone(), knob);
        let value = match knob {
            LightKnob::Brightness => light.brightness,
            LightKnob::Kelvin => light.kelvin,
        };
        if let Some(existing) = self.light_sliders.get_mut(&key) {
            if existing.shown != value {
                existing.shown = value;
                let state = existing.state.clone();
                state.update(cx, |slider, cx| {
                    slider.set_value(f32::from(value), window, cx);
                });
                return state;
            }
            return existing.state.clone();
        }
        let (low, high) = match knob {
            LightKnob::Brightness => LIGHT_BRIGHTNESS,
            LightKnob::Kelvin => LIGHT_KELVIN,
        };
        let current = f32::from(value);
        let step = match knob {
            LightKnob::Brightness => 1.0,
            // The light counts in mireds, so neighbouring Kelvin values map to
            // the same one. Stepping in fifties keeps every stop distinct.
            LightKnob::Kelvin => 50.0,
        };
        let slider = cx.new(|_| {
            SliderState::new()
                .min(low)
                .max(high)
                .step(step)
                .default_value(current.clamp(low, high))
        });
        let id = light.id.clone();
        let subscription = cx.subscribe(&slider, move |panel, _slider, event: &SliderEvent, cx| {
            if let SliderEvent::Release(value) = event {
                let asked = value.start().round().clamp(0.0, f32::from(u16::MAX));
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "clamped to u16's range on the line above"
                )]
                let asked = asked as u16;
                let change = match knob {
                    LightKnob::Brightness => NetworkLightChange {
                        brightness_percent: Some(asked),
                        ..NetworkLightChange::default()
                    },
                    LightKnob::Kelvin => NetworkLightChange {
                        kelvin: Some(asked),
                        ..NetworkLightChange::default()
                    },
                };
                panel.set_light(id.clone(), change, WriteKey::Light(id.clone(), knob), cx);
            }
        });
        self.subscriptions.push(subscription);
        self.light_sliders.insert(
            key,
            Slot {
                state: slider.clone(),
                shown: value,
            },
        );
        slider
    }

    /// One monitor: its name, then its controls or the reason it has none.
    fn display_card(
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

        v_flex()
            .gap_2()
            .p_3()
            .rounded_md()
            .bg(pal.panel)
            .child(div().text_body().child(display.name.clone()))
            .when_some(reason, |card, why| {
                card.child(div().text_caption().text_color(pal.text_muted).child(why))
            })
            .children(rows)
    }

    /// One light: on/off, brightness, colour temperature.
    fn light_card(
        &mut self,
        light: &NetworkLightSummary,
        light_index: u64,
        pal: Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        // A light that would not say what it is doing gets its name and the
        // reason, and no controls at all: every number on it would be a zero
        // standing in for "not known", and a slider sitting at zero reads as a
        // light turned all the way down rather than one that is not answering.
        if !light.reachable {
            return v_flex()
                .gap_2()
                .p_3()
                .rounded_md()
                .bg(pal.panel)
                .child(div().text_body().child(light.name.clone()))
                .when_some(light.unreachable_reason.clone(), |card, why| {
                    card.child(div().text_caption().text_color(pal.text_muted).child(why))
                })
                .into_any_element();
        }
        let brightness = self.light_slider(light, LightKnob::Brightness, window, cx);
        let kelvin = self.light_slider(light, LightKnob::Kelvin, window, cx);
        let id = light.id.clone();
        let on = light.on;
        v_flex()
            .gap_2()
            .p_3()
            .rounded_md()
            .bg(pal.panel)
            .child(
                h_flex()
                    .justify_between()
                    .child(div().text_body().child(light.name.clone()))
                    .child(
                        Toggle::new(gpui::ElementId::NamedInteger(
                            "desk-light".into(),
                            light_index,
                        ))
                        .label(if on { tr!("On") } else { tr!("Off") })
                        .selected(on)
                        .on_change({
                            // `Toggle::on_change` hands back an `App`, not
                            // this panel's context, so the panel is
                            // reached weakly: if it has gone, the write
                            // has nothing left to report to and is
                            // dropped rather than resurrecting it.
                            let panel = cx.entity().downgrade();
                            move |_checked: &bool, _window: &mut Window, cx: &mut App| {
                                let id = id.clone();
                                let _ = panel.update(cx, |panel, cx| {
                                    panel.set_light(
                                        id.clone(),
                                        NetworkLightChange {
                                            power: Some(!on),
                                            ..NetworkLightChange::default()
                                        },
                                        WriteKey::LightPower(id),
                                        cx,
                                    );
                                });
                            }
                        }),
                    ),
            )
            .child(
                h_flex()
                    .justify_between()
                    .child(div().text_caption().child(tr!("Brightness")))
                    .child(
                        div()
                            .text_caption()
                            .text_color(pal.text_muted)
                            .child(format!("{}%", light.brightness)),
                    ),
            )
            .child(Slider::new(&brightness).horizontal())
            .child(
                h_flex()
                    .justify_between()
                    .child(div().text_caption().child(tr!("Colour temperature")))
                    .child(
                        div()
                            .text_caption()
                            .text_color(pal.text_muted)
                            .child(format!("{} K", light.kelvin)),
                    ),
            )
            .child(Slider::new(&kelvin).horizontal())
            .into_any_element()
    }
}

impl Render for DeskPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pal = theme::palette(cx);
        let displays: Vec<DisplaySummary> = self.model.displays().to_vec();
        let lights: Vec<NetworkLightSummary> = self.model.lights().to_vec();
        let scanning = self.model.is_scanning();
        let empty = self.model.is_empty();

        let display_cards: Vec<_> = displays
            .iter()
            .map(|display| {
                self.display_card(display, pal, window, cx)
                    .into_any_element()
            })
            .collect();
        let light_cards: Vec<_> = lights
            .iter()
            .enumerate()
            .map(|(index, light)| {
                // Indexed rather than keyed by address only because
                // `ElementId` takes an integer here; the address still decides
                // which light a write reaches.
                self.light_card(light, index as u64, pal, window, cx)
                    .into_any_element()
            })
            .collect();

        v_flex()
            .gap_4()
            .p_4()
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .child(div().text_body().child(tr!("Monitors")))
                    .child(
                        control_button("desk-refresh")
                            .label(tr!("Refresh"))
                            .disabled(scanning)
                            .on_click(cx.listener(|panel, _event, _window, cx| {
                                panel.refresh(cx);
                                cx.notify();
                            })),
                    ),
            )
            .when(scanning, |column| {
                column.child(
                    div()
                        .text_caption()
                        .text_color(pal.text_muted)
                        .child(tr!("Looking for monitors and lights…")),
                )
            })
            .when(empty, |column| {
                column.child(
                    div()
                        .text_caption()
                        .text_color(pal.text_muted)
                        .child(tr!("No monitors or lights were found.")),
                )
            })
            .children(display_cards)
            .when(!lights.is_empty(), |column| {
                column.child(div().text_body().child(tr!("Lights")))
            })
            .children(light_cards)
    }
}

impl AuxWindow for DeskPanel {
    fn set_appearance_obs(&mut self, sub: Subscription) {
        self.appearance = Some(sub);
    }
}

/// The window's native title — one definition for opening and for the
/// live-language retitle, so the two cannot drift.
pub(crate) fn window_title() -> gpui::SharedString {
    tr!("Monitors and lights")
}

/// Open the panel, or focus it if it is already up.
pub fn open(cx: &mut App) {
    crate::windows::open_or_focus(
        |registry| &mut registry.desk,
        window_title(),
        gpui::Size::new(gpui::px(460.), gpui::px(620.)),
        DeskPanel::new,
        cx,
    );
}
