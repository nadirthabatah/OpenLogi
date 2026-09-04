//! The eleven desk methods, exercised over the real socket against the mock.
//!
//! Everything else about them is unit-tested: the wire encoding has golden
//! bytes, the agent's conversions have their own tests, and the panel's
//! behaviour has eleven more. What none of that touches is the whole path at
//! once — a client encoding a call, the transport carrying it, tarpc
//! dispatching it by *position* in the service trait, and an answer coming
//! back decoded.
//!
//! That path is where an append-only mistake shows up and nowhere else. Adding
//! a method anywhere but the end shifts every later method's index, and both
//! sides still compile: the client asks for one thing and the server runs
//! another. Bytes on their own cannot catch it, because each method's
//! encoding is individually correct.
//!
//! The mock agent is the server because it is the one that answers without
//! hardware, and because its scripted state is what GUI development runs
//! against — so a break here breaks that too.
//!
//! # On skipping
//!
//! The first version of this test skipped when it could not reach the mock,
//! and reported "ok" for twenty seconds of reaching a socket nobody was
//! listening on — the mock serves the `dev` profile and the client had gone
//! looking for the production one. It was green and it proved nothing, which
//! is the whole failure this project keeps finding in its own tests.
//!
//! So the rule here is narrow: skipping is allowed only when the mock cannot
//! be **started at all**, which is an environment refusing to run a binary and
//! not something this code can be blamed for. Once it has started, failing to
//! reach it is a real failure and fails.

#![expect(
    clippy::tests_outside_test_module,
    reason = "an integration test file is already its own test-only crate"
)]
#![expect(
    clippy::expect_used,
    reason = "the helpers sit outside any #[test] fn, where allow-expect-in-tests cannot see them"
)]

use std::process::{Child, Command};
use std::time::{Duration, Instant};

use roadie_ipc::desk::{AudioInputChange, DisplayControl, NetworkLightChange, StreamDeckChange};
use roadie_ipc::{AgentClient, PROTOCOL_VERSION};
use tarpc::context;

/// Kills the mock on the way out, including when a test panics.
struct Mock(Child);

impl Drop for Mock {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Start the mock agent, or say why not.
///
/// `None` means it could not be launched — an environment that will not run a
/// binary, which is not this code's fault. Anything after that is.
fn spawn_mock() -> Option<Mock> {
    Command::new(env!("CARGO_BIN_EXE_roadie-agent-mock"))
        .spawn()
        .ok()
        .map(Mock)
}

/// Connect to the running mock, or panic saying what was tried.
///
/// Polls rather than sleeping a fixed time: a fixed wait is either slower than
/// it needs to be or too short on a loaded machine.
async fn connect_to_mock() -> AgentClient {
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut last: Option<String> = None;
    while Instant::now() < deadline {
        match roadie_ipc::transport::connect().await {
            Ok(stream) => {
                let transport = roadie_ipc::transport::wrap(stream);
                let client = AgentClient::new(tarpc::client::Config::default(), transport).spawn();
                // The handshake doubles as proof the socket belongs to a
                // server speaking this version, not to something left over.
                match client.protocol_version(context::current()).await {
                    Ok(version) if version == PROTOCOL_VERSION => return client,
                    Ok(version) => {
                        last = Some(format!(
                            "something is listening but speaks version {version}, not \
                             {PROTOCOL_VERSION}"
                        ));
                    }
                    Err(error) => last = Some(format!("handshake failed: {error}")),
                }
            }
            Err(error) => last = Some(format!("connect failed: {error}")),
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!(
        "the mock agent started but never answered on the {} profile. Last attempt: {}",
        std::env::var("ROADIE_PROFILE").unwrap_or_else(|_| "default".to_owned()),
        last.unwrap_or_else(|| "no attempt was made".to_owned())
    );
}

#[test]
fn the_desk_methods_answer_over_the_real_socket() {
    // The mock serves the `dev` profile so it cannot occupy an installed
    // agent's socket, and the profile is read from the environment at connect
    // time — so this client has to ask for the same one or it spends the whole
    // deadline knocking on the production socket, which is exactly what the
    // first version of this test did while reporting success.
    //
    #[expect(
        unsafe_code,
        reason = "the profile must be chosen before roadie_core::paths resolves it, and only a process-wide env var selects it — the same reason the mock's own main does this"
    )]
    // SAFETY: `set_var` is unsound only against concurrent env access. This is
    // the first statement of the test: no runtime has been built, no other
    // thread exists, and nothing has read the environment yet.
    unsafe {
        std::env::set_var("ROADIE_PROFILE", "dev");
    }

    let Some(_mock) = spawn_mock() else {
        eprintln!("skipped: this host would not launch the mock agent binary");
        return;
    };

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("a tokio runtime");
    runtime.block_on(exercise_the_desk_methods());
}

async fn exercise_the_desk_methods() {
    let client = connect_to_mock().await;
    exercise_monitors(&client).await;
    exercise_lights(&client).await;
    exercise_stream_decks(&client).await;
    exercise_audio_interfaces(&client).await;
    exercise_controllers_and_pads(&client).await;
}

async fn exercise_monitors(client: &AgentClient) {
    let ctx = context::current;

    // Monitors. The mock scripts one that answers and one that does not,
    // because a panel only ever seen against monitors that answer has never
    // been seen in its commonest real state.
    let displays = client.list_displays(ctx()).await.expect("list_displays");
    assert_eq!(displays.len(), 2, "the mock scripts two monitors");
    let reachable = displays
        .iter()
        .find(|display| display.reachable)
        .expect("one of them answers");
    let silent = displays
        .iter()
        .find(|display| !display.reachable)
        .expect("and one of them does not");
    assert!(
        silent
            .unreachable_reason
            .as_deref()
            .is_some_and(|why| why.contains("i2c")),
        "a monitor that will not answer says why: {silent:?}"
    );

    let settings = client
        .read_display(ctx(), reachable.id.clone())
        .await
        .expect("read_display")
        .expect("the reachable monitor answers");
    assert!(
        settings
            .readings
            .iter()
            .any(|reading| reading.control == DisplayControl::Brightness),
        "brightness is what almost everyone came for"
    );

    // A write answers with what the device took, not with what was sent.
    let taken = client
        .set_display(ctx(), reachable.id.clone(), DisplayControl::Contrast, 500)
        .await
        .expect("set_display")
        .expect("the reachable monitor accepts a write");
    assert_eq!(
        taken.current, taken.maximum,
        "500 is past this monitor's maximum, so it clamps and says so"
    );
    assert!(
        taken.maximum < 500,
        "the mock's contrast maximum is deliberately not 100 either"
    );

    // Reading it back proves the write went somewhere that persists, rather
    // than being echoed by the call that made it.
    let after = client
        .read_display(ctx(), reachable.id.clone())
        .await
        .expect("read_display")
        .expect("still reachable");
    let contrast = after
        .readings
        .iter()
        .find(|reading| reading.control == DisplayControl::Contrast)
        .expect("contrast is still there");
    assert_eq!(contrast.current, taken.current);

    // A monitor that does not answer fails rather than returning nothing.
    client
        .read_display(ctx(), silent.id.clone())
        .await
        .expect("read_display")
        .expect_err("the silent monitor is not readable");
}

async fn exercise_lights(client: &AgentClient) {
    let ctx = context::current;
    let lights = client
        .list_network_lights(ctx())
        .await
        .expect("list_network_lights");
    assert_eq!(lights.len(), 3, "the mock scripts three lights");

    // The one that announced itself and then went quiet is listed rather than
    // dropped, and says why. Dropping it would mean a light silently
    // disappearing from somebody's desk, which is the least useful thing a
    // list can do.
    let quiet = lights
        .iter()
        .find(|light| !light.reachable)
        .expect("one light does not answer");
    assert!(
        quiet
            .unreachable_reason
            .as_deref()
            .is_some_and(|why| why.contains("timed out")),
        "it says why: {quiet:?}"
    );

    // And it will not be told what to do either, since it could not say what
    // it was doing.
    client
        .set_network_light(
            ctx(),
            quiet.id.clone(),
            NetworkLightChange {
                power: Some(true),
                ..NetworkLightChange::default()
            },
        )
        .await
        .expect("set_network_light")
        .expect_err("a light that is not answering cannot be written");

    let first = lights
        .iter()
        .find(|light| light.reachable)
        .expect("one light answers");

    let changed = client
        .set_network_light(
            ctx(),
            first.id.clone(),
            NetworkLightChange {
                power: Some(!first.on),
                brightness_percent: Some(1),
                ..NetworkLightChange::default()
            },
        )
        .await
        .expect("set_network_light")
        .expect("the light accepts a change");
    assert_eq!(changed.on, !first.on);
    assert_eq!(
        changed.brightness, 3,
        "1 percent is below what a Key Light accepts, so it clamps to its floor"
    );

    // A change asking for nothing is refused rather than reported as success.
    client
        .set_network_light(ctx(), first.id.clone(), NetworkLightChange::default())
        .await
        .expect("set_network_light")
        .expect_err("a change with nothing in it");

    // A light that is not there is refused too.
    client
        .set_network_light(
            ctx(),
            "203.0.113.1:9123".to_owned(),
            NetworkLightChange {
                power: Some(true),
                ..NetworkLightChange::default()
            },
        )
        .await
        .expect("set_network_light")
        .expect_err("no light at that address");
}

async fn exercise_stream_decks(client: &AgentClient) {
    let ctx = context::current;
    let decks = client
        .list_stream_decks(ctx())
        .await
        .expect("list_stream_decks");
    assert_eq!(decks.len(), 2, "the mock scripts two decks");

    // Held exclusively by another program is the commonest state a real deck
    // is found in, and the reason it is listed rather than dropped: "quit the
    // Elgato app" is something a person can act on, and a missing row is not.
    let held = decks
        .iter()
        .find(|deck| !deck.reachable)
        .expect("one deck will not open");
    assert!(
        held.unreachable_reason
            .as_deref()
            .is_some_and(|why| why.contains("already has this device open")),
        "it says why: {held:?}"
    );

    let open = decks
        .iter()
        .find(|deck| deck.reachable)
        .expect("one deck opens");
    assert_eq!(open.keys, 32, "an XL has 32 keys");

    client
        .set_stream_deck(
            ctx(),
            open.id.clone(),
            StreamDeckChange {
                brightness_percent: Some(30),
                reset: false,
            },
        )
        .await
        .expect("set_stream_deck")
        .expect("the open deck takes a brightness");

    // Above 100 the hardware's behaviour is undefined, so it never gets there.
    client
        .set_stream_deck(
            ctx(),
            open.id.clone(),
            StreamDeckChange {
                brightness_percent: Some(101),
                reset: false,
            },
        )
        .await
        .expect("set_stream_deck")
        .expect_err("101 percent is not a brightness");

    client
        .set_stream_deck(ctx(), open.id.clone(), StreamDeckChange::default())
        .await
        .expect("set_stream_deck")
        .expect_err("a change with nothing in it");
}

async fn exercise_audio_interfaces(client: &AgentClient) {
    let ctx = context::current;
    let interfaces = client
        .list_audio_interfaces(ctx())
        .await
        .expect("list_audio_interfaces");
    let interface = interfaces.first().expect("the mock scripts one interface");
    assert_eq!(interface.inputs.len(), 2, "a Vocaster Two has two inputs");

    // The second input deliberately has no phantom power, so the panel has to
    // draw an input whose controls are not all present.
    let bare = interface
        .inputs
        .iter()
        .find(|settings| settings.phantom.is_none())
        .expect("one input has no phantom power");
    assert!(bare.gain.is_some(), "it still has gain: {bare:?}");

    // A write answers with the whole interface, because phantom power is
    // switched per pair and changing one input changes what its neighbour says.
    let after = client
        .set_audio_input(
            ctx(),
            interface.id.clone(),
            1,
            AudioInputChange {
                gain: Some(42),
                ..AudioInputChange::default()
            },
        )
        .await
        .expect("set_audio_input")
        .expect("the interface takes a gain");
    let first = after
        .inputs
        .iter()
        .find(|settings| settings.input == 1)
        .expect("input one is still there");
    assert_eq!(first.gain, Some(42), "the write stuck");

    // Switching phantom power on without having been told what it costs is
    // refused, and the refusal carries the sentence to read out. This is the
    // whole safety design, and the wire is where it has to survive.
    let refused = client
        .set_audio_input(
            ctx(),
            interface.id.clone(),
            1,
            AudioInputChange {
                phantom: Some(true),
                ..AudioInputChange::default()
            },
        )
        .await
        .expect("set_audio_input")
        .expect_err("phantom power on without acknowledgement");
    assert!(
        refused.to_string().contains("ribbon"),
        "the refusal says what it can damage: {refused}"
    );

    // Acknowledged, it goes through.
    let lit = client
        .set_audio_input(
            ctx(),
            interface.id.clone(),
            1,
            AudioInputChange {
                phantom: Some(true),
                phantom_acknowledged: true,
                ..AudioInputChange::default()
            },
        )
        .await
        .expect("set_audio_input")
        .expect("acknowledged phantom power");
    assert_eq!(
        lit.inputs
            .iter()
            .find(|settings| settings.input == 1)
            .and_then(|settings| settings.phantom),
        Some(true)
    );

    // Switching it off asks nothing: that is how somebody makes the interface
    // safe again, and a confirmation in front of it would be an obstacle
    // before the safe direction.
    client
        .set_audio_input(
            ctx(),
            interface.id.clone(),
            1,
            AudioInputChange {
                phantom: Some(false),
                ..AudioInputChange::default()
            },
        )
        .await
        .expect("set_audio_input")
        .expect("switching phantom power off needs no acknowledgement");
}

async fn exercise_controllers_and_pads(client: &AgentClient) {
    let ctx = context::current;

    // Identity only, and deliberately so: a TourBox stores no settings, so
    // there is no matching setter to exercise here.
    let controllers = client
        .list_controllers(ctx())
        .await
        .expect("list_controllers");
    let tourbox = controllers
        .first()
        .expect("the mock scripts one controller");
    assert!(tourbox.buttons > 0, "it has buttons: {tourbox:?}");
    assert!(!tourbox.id.is_empty(), "and a port to reach it on");

    let pads = client
        .list_macro_pads(ctx())
        .await
        .expect("list_macro_pads");
    let pad = pads.first().expect("the mock scripts one board");
    assert!(pad.reachable, "the scripted board answers: {pad:?}");
    assert!(
        pad.layers > 0,
        "a board that answered says how many layers it has: {pad:?}"
    );
}
