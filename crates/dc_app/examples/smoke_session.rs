//! Generate a small deterministic session for manual comparison-mode testing.
use dc_app::{persistence::save_session, SessionState};
use dc_core::{LayerColor, RasterBuffer};
use image::{Rgba, RgbaImage};
fn main() {
    let mut session = SessionState::new();
    let reference = RgbaImage::from_fn(1200, 800, |x, y| {
        if (100..1100).contains(&x) && (y == 100 || y == 700)
            || (100..700).contains(&y) && (x == 100 || x == 1100)
            || (200..700).contains(&x) && (y == 250 || y == 500)
        {
            Rgba([0, 0, 0, 255])
        } else {
            Rgba([255; 4])
        }
    });
    session
        .add_layer(
            "Reference".into(),
            "smoke-reference.png".into(),
            RasterBuffer::new(reference.clone(), 300),
        )
        .unwrap();
    for (index, color) in [
        (1, LayerColor::new(0, 100, 255)),
        (2, LayerColor::new(0, 180, 80)),
    ] {
        let mut target = reference.clone();
        for y in 300..400 {
            for x in (200 * index)..(200 * index + 100) {
                target.put_pixel(x, y, Rgba([0, 0, 0, 255]));
            }
        }
        let id = session
            .add_layer(
                format!("Revision {index}"),
                format!("smoke-{index}.png").into(),
                RasterBuffer::new(target, 300),
            )
            .unwrap();
        session.get_layer_mut(id).unwrap().blend_color = color;
    }
    session.viewport.center_x = 600.0;
    session.viewport.center_y = 400.0;
    session.viewport.zoom = 0.75;
    let path = std::env::args().nth(1).expect("output session path");
    save_session(&session, std::path::Path::new(&path)).unwrap();
    println!("Saved {path}");
}
