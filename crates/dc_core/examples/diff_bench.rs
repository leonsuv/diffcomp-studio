//! Repeatable drawing-diff benchmark: cargo run --release -p dc_core --example diff_bench
use image::{Rgba, RgbaImage};
use std::time::Instant;
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (reference, target) = if args.len() >= 2 {
        (
            image::open(&args[0]).expect("reference image").to_rgba8(),
            image::open(&args[1]).expect("target image").to_rgba8(),
        )
    } else {
        let reference = RgbaImage::from_fn(4096, 4096, |x, y| {
            if x % 101 == 0 || y % 83 == 0 {
                Rgba([0, 0, 0, 255])
            } else {
                Rgba([255; 4])
            }
        });
        let mut target = reference.clone();
        for y in 100..400 {
            for x in 100..400 {
                target.put_pixel(x, y, Rgba([0, 0, 0, 255]));
            }
        }
        (reference, target)
    };
    let mut times = Vec::new();
    for _ in 0..6 {
        let start = Instant::now();
        let result =
            dc_core::compute_morphological_diff(&reference, &target, [255, 0, 0], [0, 100, 255])
                .unwrap();
        std::hint::black_box(&result);
        times.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    times.remove(0); // Warm caches and initialize the Rayon pool before measuring.
    times.sort_by(f64::total_cmp);
    println!(
        "{}x{}: median {:.1} ms, min {:.1} ms",
        reference.width(),
        reference.height(),
        times[2],
        times[0]
    );
}
