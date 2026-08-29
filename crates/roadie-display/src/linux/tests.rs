use std::fs;

use super::{connected, i2c_node, read_edid};

/// A throwaway directory shaped like one of the kernel's connector
/// directories.
///
/// Real `/sys` cannot be written to and a machine running these tests has no
/// monitor attached anyway, so the layout is reproduced rather than borrowed.
/// What is being tested is this file's reading of that layout, which is
/// exactly the part that would break if a driver published it differently.
struct Connector(std::path::PathBuf);

impl Connector {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("roadie-display-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("a writable temporary directory");
        Self(path)
    }

    fn with_file(self, name: &str, contents: &[u8]) -> Self {
        fs::write(self.0.join(name), contents).expect("a writable temporary file");
        self
    }

    fn with_bus(self, relative: &str, bus: &str) -> Self {
        let directory = self.0.join(relative);
        fs::create_dir_all(&directory).expect("a writable temporary directory");
        fs::create_dir_all(directory.join(bus)).expect("a writable temporary directory");
        self
    }
}

impl Drop for Connector {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn only_a_connected_connector_counts() {
    let plugged = Connector::new("connected").with_file("status", b"connected\n");
    assert!(connected(&plugged.0));

    let empty = Connector::new("disconnected").with_file("status", b"disconnected\n");
    assert!(!connected(&empty.0));

    // A directory under /sys/class/drm with no status file at all is not a
    // connector — the cards themselves look like this.
    let card = Connector::new("card");
    assert!(!connected(&card.0));
}

#[test]
fn a_connected_connector_with_an_empty_edid_has_no_name() {
    // A real state, not a broken one: the kernel reports the link before it
    // has read the block, and some KVM switches never let it. A display with
    // no name is still a display.
    let connector = Connector::new("nameless").with_file("edid", b"");
    assert!(read_edid(&connector.0).is_none());
}

#[test]
fn the_modern_bus_layout_is_found() {
    // What amdgpu and i915 publish today.
    let connector = Connector::new("modern").with_bus("ddc/i2c-dev", "i2c-7");
    assert_eq!(
        i2c_node(&connector.0),
        Some(std::path::PathBuf::from("/dev/i2c-7"))
    );
}

#[test]
fn the_older_bus_layout_is_found_too() {
    // Some drivers put the bus directly under the connector. Guessing one
    // layout would leave those monitors silently uncontrollable.
    let connector = Connector::new("older").with_bus(".", "i2c-3");
    assert_eq!(
        i2c_node(&connector.0),
        Some(std::path::PathBuf::from("/dev/i2c-3"))
    );
}

#[test]
fn a_connector_with_no_bus_at_all_reports_none() {
    let connector = Connector::new("busless").with_file("status", b"connected\n");
    assert_eq!(i2c_node(&connector.0), None);
}

#[test]
fn a_machine_with_no_display_subsystem_reports_none_rather_than_failing() {
    // The build container's own case, and it went the wrong way the first
    // time: with no /sys/class/drm at all this returned an error, so
    // `roadie display list` said "something went wrong" where the true answer
    // was "you have no monitors". A container, a headless server and a kernel
    // built without DRM are all that shape, and none of them is a fault.
    let displays = super::enumerate().expect("no display subsystem is an answer, not a failure");
    for display in &displays {
        assert!(
            !display.describe().is_empty(),
            "every display in the list has something to call it"
        );
    }
}
