//! Create a reproducible engineering comparison for screenshots and startup tests.
use dc_app::{persistence::save_session, SessionState};
use dc_core::{LayerColor, RasterBuffer};
use std::path::{Path, PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut session = SessionState::new();
    for (filename, name) in [
        ("reference.png", "Level 01 · Original"),
        ("revision.png", "Level 01 · Revision B"),
    ] {
        let path = root.join("docs/demo").join(filename);
        let image = image::open(&path)?.into_rgba8();
        let id = session.add_layer(name.into(), path, RasterBuffer::new(image, 200))?;
        session.get_layer_mut(id).unwrap().blend_color = if session.layers.len() == 1 {
            LayerColor::new(232, 92, 82)
        } else {
            LayerColor::new(78, 145, 218)
        };
    }
    session.diff_config.reference_color = LayerColor::new(232, 92, 82);
    session.viewport.center_x = 900.0;
    session.viewport.center_y = 600.0;
    session.viewport.zoom = 0.54;
    let output = std::env::args()
        .nth(1)
        .ok_or("Provide an output session path")?;
    save_session(&session, Path::new(&output))?;
    println!("Saved {output}");
    Ok(())
}
