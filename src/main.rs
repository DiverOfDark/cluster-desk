//! A fast desktop Kubernetes UI inspired by the Lens family of IDEs.

use eframe::egui;

mod ansi;
mod app;
mod backend;
mod crd;
mod demo;
mod details;
mod kubectl_checksums;
mod dock;
mod model;
mod prefs;
mod settings;
mod terminal;
mod theme;
mod update;
mod util;
mod views;

use std::path::PathBuf;

fn main() -> eframe::Result<()> {
    let mut screenshot: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--screenshot" => screenshot = args.next().map(PathBuf::from),
            s if s.starts_with("--screenshot=") => screenshot = Some(PathBuf::from(&s["--screenshot=".len()..])),
            "--help" | "-h" => {
                println!("kubelens [--screenshot <file.png>]");
                return Ok(());
            }
            _ => {}
        }
    }
    let size = [1440.0, 900.0];
    if let Some(path) = screenshot {
        // Draw ~120+ frames off-screen on the GPU and save the picture. This needs no window,
        // so it also works on a locked or headless desktop.
        return headless_screenshot(path, size);
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Cluster Desk")
            .with_app_id("cluster-desk")
            .with_inner_size(size)
            .with_min_inner_size([900.0, 560.0])
            .with_icon(eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")).unwrap_or_default()),
        wgpu_options: low_power_wgpu(),
        ..Default::default()
    };
    eframe::run_native("Cluster Desk", options, Box::new(move |cc| Ok(Box::new(app::App::new(cc, screenshot)))))
}

/// eframe asks wgpu for the high-performance adapter by default, which on a laptop or a
/// desktop with two GPUs wakes the discrete card (and makes the compositor copy every frame
/// across). A list-and-text UI has no use for it, so prefer the integrated GPU.
/// `WGPU_POWER_PREF=high` still overrides this.
fn low_power_wgpu() -> eframe::egui_wgpu::WgpuConfiguration {
    let mut cfg = eframe::egui_wgpu::WgpuConfiguration::default();
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut cfg.wgpu_setup {
        setup.power_preference =
            eframe::egui_wgpu::wgpu::PowerPreference::from_env().unwrap_or(eframe::egui_wgpu::wgpu::PowerPreference::LowPower);
    }
    cfg
}

fn headless_screenshot(path: PathBuf, size: [f32; 2]) -> eframe::Result<()> {
    let scale: f32 = std::env::var("SHOT_SCALE").ok().and_then(|v| v.parse().ok()).unwrap_or(1.7);
    let p2 = path.clone();
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(size[0], size[1]))
        .with_pixels_per_point(scale)
        .allow_missing_glyphs()
        .wgpu()
        .build_eframe(move |cc| {
            // Same system-font fallback a normal window gets (symbols, emoji, CJK).
            cc.egui_ctx.add_font_provider(std::sync::Arc::new(egui_system_font_provider::SystemFontProvider::new()));
            let mut a = app::App::new(cc, Some(p2));
            a.headless = true;
            a
        });
    let start = std::time::Instant::now();
    while !harness.state().shot_ready && start.elapsed() < std::time::Duration::from_secs(80) {
        harness.step();
        std::thread::sleep(std::time::Duration::from_millis(4));
    }
    // A couple more frames so everything settles before rendering.
    harness.step();
    harness.step();
    match harness.render() {
        Ok(img) => {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            match img.save(&path) {
                Ok(()) => {
                    eprintln!("saved screenshot to {}", path.display());
                    std::process::exit(0);
                }
                Err(e) => {
                    eprintln!("could not save screenshot: {e}");
                    std::process::exit(1);
                }
            }
        }
        Err(e) => {
            eprintln!("could not render screenshot: {e}");
            std::process::exit(1);
        }
    }
}
