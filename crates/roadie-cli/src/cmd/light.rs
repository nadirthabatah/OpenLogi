//! `roadie light` — discovery and manual control for standalone lights.
//!
//! The CLI intentionally uses the same raw-HID driver as the agent. It is a
//! small hardware-facing surface for validating discovery and report encoding
//! before exercising the GPUI panel.
//!
//! Two families, reached two ways. Logitech's Litra lights are on USB and
//! speak raw HID; Elgato's Key Lights are on Wi-Fi and speak HTTP. They are
//! one command rather than two because the question a person asks is "what
//! lights do I have", and an answer covering only the ones on USB would be
//! worse for being confidently incomplete.

mod network;

use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use clap::{Args, Subcommand};
use roadie_core::device::{LightValueUnit, StandaloneDevice};
use roadie_hid::{DeviceRoute, LightCommand, find_litra};

use crate::spoken::counted;

#[derive(Debug, Subcommand)]
pub enum LightCmd {
    /// List every light this build can drive, on USB and on the network.
    List(DeviceArgs),
    /// Turn a light on.
    On(DeviceArgs),
    /// Turn a light off.
    Off(DeviceArgs),
    /// Set normalized brightness or native lumens.
    Brightness(BrightnessArgs),
    /// Set colour temperature in Kelvin.
    Temperature(TemperatureArgs),
}

#[derive(Debug, Args)]
pub struct DeviceArgs {
    /// Case-insensitive substring of the light name or identity.
    #[arg(long)]
    device: Option<String>,
    /// Do not look for lights on the network.
    ///
    /// Looking costs a few seconds every time, so this is here for anyone
    /// whose lights are all on USB and who runs this often.
    #[arg(long)]
    no_network: bool,
    /// Seconds to spend looking for lights on the network.
    #[arg(long, default_value_t = 3, value_parser = clap::value_parser!(u64).range(1..=30))]
    wait: u64,
}

impl DeviceArgs {
    /// Every Key Light on the network, or none when the look was declined.
    fn network(&self) -> Vec<network::Found> {
        if self.no_network {
            return Vec::new();
        }
        network::find(Duration::from_secs(self.wait))
    }
}

#[derive(Debug, Args)]
pub struct BrightnessArgs {
    #[command(flatten)]
    device: DeviceArgs,
    /// Normalized brightness from 0 to 100 percent.
    #[arg(long, conflicts_with = "lumens", value_parser = clap::value_parser!(u8).range(0..=100))]
    percent: Option<u8>,
    /// Native brightness in lumens.
    #[arg(long, conflicts_with = "percent")]
    lumens: Option<u16>,
}

#[derive(Debug, Args)]
pub struct TemperatureArgs {
    #[command(flatten)]
    device: DeviceArgs,
    /// Colour temperature in Kelvin.
    #[arg(long)]
    kelvin: u16,
}

impl LightCmd {
    /// Dispatch the subcommand.
    ///
    /// # Errors
    ///
    /// Fails when the HID stack cannot be enumerated, when no light matches
    /// the selection, or when a light refuses the change. A network with no
    /// Key Lights on it is not a failure.
    pub async fn run(self) -> Result<()> {
        match self {
            Self::List(args) => list(&args).await,
            Self::On(args) => set_power(&args, true).await,
            Self::Off(args) => set_power(&args, false).await,
            Self::Brightness(args) => set_brightness(args).await,
            Self::Temperature(args) => set_temperature(args).await,
        }
    }
}

/// One light on the desk, whichever way it is reached.
///
/// A borrow rather than an owned value because selection picks one out of two
/// lists that the caller already holds, and copying a device to name it would
/// be work for nothing.
#[derive(Debug)]
enum Lamp<'a> {
    /// A Logitech Litra, over USB.
    Litra(&'a StandaloneDevice),
    /// An Elgato Key Light, over the network.
    Network(&'a network::Found),
}

impl Lamp<'_> {
    /// The name to print, speak, or match a query against.
    fn name(&self) -> &str {
        match self {
            Self::Litra(device) => &device.display_name,
            Self::Network(found) => found.name(),
        }
    }
}

/// The one light to act on, out of everything found.
///
/// Ambiguity is an error rather than a guess. Picking the first of several
/// would change a different light than the one meant, and someone who cannot
/// see which one lit up has no way to notice.
fn select_lamp<'a>(
    litra: &'a [StandaloneDevice],
    network: &'a [network::Found],
    query: Option<&str>,
) -> Result<Lamp<'a>> {
    let all: Vec<Lamp<'a>> = litra
        .iter()
        .map(Lamp::Litra)
        .chain(network.iter().map(Lamp::Network))
        .collect();

    let Some(query) = query else {
        return match all.len() {
            0 => Err(anyhow!(
                "No lights found. Run roadie light list, which says what was looked for."
            )),
            1 => Ok(all.into_iter().next().unwrap_or_else(|| unreachable!())),
            many => Err(anyhow!(
                "{} found, so which one has to be said: add --device and part of a name. \
                 Run roadie light list to see them.",
                counted(many, "light is", "lights are")
            )),
        };
    };

    let wanted = query.to_lowercase();
    let matching = |lamp: &Lamp<'a>| match lamp {
        Lamp::Litra(device) => {
            device.display_name.to_lowercase().contains(&wanted)
                || device.address.identity.to_lowercase().contains(&wanted)
        }
        Lamp::Network(found) => {
            found.name().to_lowercase().contains(&wanted)
                || found.light.address().to_string().contains(&wanted)
        }
    };
    let mut matched: Vec<Lamp<'a>> = all.into_iter().filter(matching).collect();
    match matched.len() {
        0 => Err(anyhow!(
            "No light's name contains {query:?}. Run roadie light list to see them."
        )),
        1 => Ok(matched.remove(0)),
        // Naming the candidates rather than only counting them: the next thing
        // someone has to do is choose between them, and being told to "use
        // more of the name" without being told the names is an instruction
        // they cannot follow.
        _ => Err(anyhow!(
            "{query:?} matches more than one light: {}. Use more of a name.",
            matched
                .iter()
                .map(|lamp| lamp.name().to_owned())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

async fn standalone() -> Result<Vec<StandaloneDevice>> {
    roadie_hid::enumerate_standalone()
        .await
        .context("failed to enumerate standalone HID devices")
}

async fn list(args: &DeviceArgs) -> Result<()> {
    let devices = standalone().await?;
    let network = args.network();
    if devices.is_empty() && network.is_empty() {
        print!("{}", nothing_found(args.no_network));
        return Ok(());
    }
    for found in &network {
        print!("{}", network::describe(found));
    }
    if !network.is_empty() {
        print!("{}", network::ranges());
    }
    for device in devices {
        let address = &device.address;
        println!(
            "{} — {} ({:04x}:{:04x} usage {:04x}:{:04x})",
            device.display_name,
            address.identity,
            address.vendor_id,
            address.product_id,
            address.usage_page,
            address.usage_id,
        );
        if let Some(caps) = device.light_capabilities {
            if let Some(range) = caps.brightness {
                println!(
                    "  brightness: {}–{} {:?}",
                    range.min(),
                    range.max(),
                    range.unit()
                );
            }
            if let Some(range) = caps.temperature {
                println!(
                    "  temperature: {}–{} K step {}",
                    range.min(),
                    range.max(),
                    range.step()
                );
            }
            println!("  power: {}", if caps.power { "yes" } else { "no" });
        }
    }
    Ok(())
}

/// What to say when nothing was found at all.
///
/// Names what was looked for, including what was deliberately not looked for,
/// because "no lights found" after `--no-network` would otherwise read as a
/// statement about the network too.
fn nothing_found(no_network: bool) -> String {
    if no_network {
        return "No lights found on USB. The network was not searched, because \
                --no-network was given.\n"
            .to_owned();
    }
    "No lights found, on USB or on the network. A Litra has to be plugged in and \
     readable; an Elgato light has to be on the same network as this computer and \
     answering.\n"
        .to_owned()
}

async fn set_power(args: &DeviceArgs, enabled: bool) -> Result<()> {
    let devices = standalone().await?;
    let network = args.network();
    match select_lamp(&devices, &network, args.device.as_deref())? {
        Lamp::Litra(device) => apply(device, LightCommand::Power(enabled)).await,
        Lamp::Network(found) => write_network(found, |light| light.set_on(enabled)),
    }
}

/// Apply a change to a network light and say what it did.
///
/// The light answers a write with its resulting state, so this reports what
/// actually happened rather than what was asked for — the light clamps values
/// of its own accord, and a report of the request would be a claim.
fn write_network(
    found: &network::Found,
    change: impl FnOnce(roadie_keylight::Light) -> roadie_keylight::Light,
) -> Result<()> {
    let current = found
        .state
        .as_ref()
        .map_err(|why| anyhow!("{} did not answer: {why}", found.name()))?;
    let after = found
        .light
        .write(change(*current))
        .with_context(|| format!("failed to change {}", found.name()))?;
    print!(
        "{}",
        network::describe(&network::Found {
            light: found.light.clone(),
            state: Ok(after),
        })
    );
    Ok(())
}

async fn set_brightness(args: BrightnessArgs) -> Result<()> {
    let devices = standalone().await?;
    let network = args.device.network();
    let device = match select_lamp(&devices, &network, args.device.device.as_deref())? {
        Lamp::Litra(device) => device,
        Lamp::Network(found) => {
            let Some(percent) = args.percent else {
                return Err(anyhow!(
                    "{} takes a brightness as a percentage, so pass --percent. Lumens are \
                     a Litra thing; an Elgato light does not report them.",
                    found.name()
                ));
            };
            return write_network(found, |light| light.set_brightness(u16::from(percent)));
        }
    };
    let caps = device
        .light_capabilities
        .ok_or_else(|| anyhow!("selected light did not advertise capabilities"))?;
    let range = caps
        .brightness
        .ok_or_else(|| anyhow!("selected light does not support brightness"))?;
    let command = match (args.percent, args.lumens) {
        (Some(percent), None) => LightCommand::BrightnessPercent(percent),
        (None, Some(lumens)) => {
            if range.unit() != LightValueUnit::Lumens || !range.contains(lumens) {
                return Err(anyhow!(
                    "lumens must be in the supported range {}..={} with step {}",
                    range.min(),
                    range.max(),
                    range.step()
                ));
            }
            LightCommand::BrightnessNative(lumens)
        }
        (None, None) => return Err(anyhow!("pass either --percent or --lumens")),
        (Some(_), Some(_)) => unreachable!("clap enforces the argument conflict"),
    };
    apply(device, command).await
}

async fn set_temperature(args: TemperatureArgs) -> Result<()> {
    let devices = standalone().await?;
    let network = args.device.network();
    match select_lamp(&devices, &network, args.device.device.as_deref())? {
        Lamp::Litra(device) => apply(device, LightCommand::TemperatureKelvin(args.kelvin)).await,
        Lamp::Network(found) => write_network(found, |light| light.set_kelvin(args.kelvin)),
    }
}

async fn apply(device: &StandaloneDevice, command: LightCommand) -> Result<()> {
    let model = find_litra(
        device.address.vendor_id,
        device.address.product_id,
        device.address.usage_page,
        device.address.usage_id,
    )
    .map(|descriptor| descriptor.model)
    .ok_or_else(|| {
        anyhow!(
            "unsupported light product {:04x}",
            device.address.product_id
        )
    })?;
    let route = DeviceRoute::RawHid {
        vendor_id: device.address.vendor_id,
        product_id: device.address.product_id,
        usage_page: device.address.usage_page,
        usage_id: device.address.usage_id,
        identity: device.address.identity.clone(),
    };
    roadie_hid::apply_litra(&route, model, command)
        .await
        .context("failed to write the light command")
}

#[cfg(test)]
mod tests;
