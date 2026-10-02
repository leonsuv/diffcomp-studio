// =============================================================================
// dc_app - DiffComp Studio Main Entry Point
// =============================================================================
// Launches the egui-based GUI application.
// =============================================================================

#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use dc_app::DiffCompApp;
#[cfg(not(target_arch = "wasm32"))]
use eframe::NativeOptions;
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;
#[cfg(not(target_arch = "wasm32"))]
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result<()> {
    if std::env::args().any(|arg| arg == "--version" || arg == "-V") {
        println!("DiffComp Studio {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    // Initialize logging
    init_logging();

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        "Starting DiffComp Studio"
    );

    let (startup_session, startup_documents) = parse_startup_args();

    // Configure the native window
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1400.0, 900.0])
        .with_min_inner_size([800.0, 600.0])
        .with_title("DiffComp Studio");

    // Optionally set icon if available
    if let Some(icon) = load_icon() {
        viewport = viewport.with_icon(std::sync::Arc::new(icon));
    }

    let options = NativeOptions {
        #[cfg(target_os = "windows")]
        renderer: eframe::Renderer::Wgpu,
        viewport,
        ..Default::default()
    };

    // Run the application
    eframe::run_native(
        "DiffComp Studio",
        options,
        Box::new(move |cc| {
            let gpu = dc_gpu::GpuDiffEngine::new_blocking().ok();
            Ok(Box::new(DiffCompApp::new_with_startup(
                cc,
                gpu,
                startup_session.clone(),
                startup_documents.clone(),
            )))
        }),
    )
}

/// `--session file.dcs` or `file.dcs` opens a session; other paths are
/// documents to compare, the first one being the reference.
#[cfg(not(target_arch = "wasm32"))]
fn parse_startup_args() -> (Option<PathBuf>, Vec<PathBuf>) {
    let mut session = None;
    let mut documents = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--session" {
            session = args.next().map(PathBuf::from);
        } else if !arg.starts_with("--") {
            let path = PathBuf::from(arg);
            if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("dcs"))
            {
                session = Some(path);
            } else {
                documents.push(path);
            }
        }
    }
    (session, documents)
}

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsCast;

#[cfg(target_arch = "wasm32")]
fn main() {
    // Make sure panics are logged using `console.error`.
    console_error_panic_hook::set_once();

    // Redirect tracing to console.log and friends:
    tracing_wasm::set_as_global_default();

    let web_options = eframe::WebOptions::default();

    wasm_bindgen_futures::spawn_local(async {
        let document = web_sys::window()
            .expect("No window")
            .document()
            .expect("No document");

        let canvas = document
            .get_element_by_id("the_canvas_id")
            .expect("Failed to find canvas")
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("Element is not a canvas");

        let gpu = match dc_gpu::GpuDiffEngine::new_async().await {
            Ok(engine) => Some(engine),
            Err(e) => {
                tracing::error!("GPU Init Failed: {}", e);
                None
            }
        };

        eframe::WebRunner::new()
            .start(
                canvas,
                web_options,
                Box::new(move |cc| Ok(Box::new(DiffCompApp::new(cc, gpu)))),
            )
            .await
            .expect("failed to start eframe");
    });
}

/// Initialize the tracing subscriber for logging.
#[cfg(not(target_arch = "wasm32"))]
fn init_logging() {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,dc_core=debug,dc_app=debug"));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(tracing_subscriber::fmt::layer())
        .init();
}

/// Load the application icon.
#[cfg(not(target_arch = "wasm32"))]
fn load_icon() -> Option<egui::IconData> {
    // In production, you would load an actual icon file here
    // For now, return None to use the default
    None
}
