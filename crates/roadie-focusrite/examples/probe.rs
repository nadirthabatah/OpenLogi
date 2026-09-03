//! Read every Focusrite interface on this computer, changing nothing.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    for found in roadie_focusrite::attached()? {
        match found {
            Ok(attached) => {
                println!("{}", attached.describe());
                println!("  control interface: {}", attached.control_interface);
                let mut session = roadie_focusrite::Session::open(&attached)?;
                let snapshot = session.snapshot()?;
                println!("  firmware: {}", snapshot.firmware);
                println!("  mass storage mode: {:?}", snapshot.msd_mode);
                for input in &snapshot.inputs {
                    println!(
                        "  input {}: gain={:?} muted={:?} phantom={:?}",
                        input.input, input.gain, input.muted, input.phantom
                    );
                }
            }
            Err(e) => println!("unusable device: {e}"),
        }
    }
    Ok(())
}
