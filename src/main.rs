//! kSpaceMonger — a fast disk space map for Linux, in the classic SpaceMonger 1.x style.
//!
//! Usage: kSpaceMonger [FOLDER] [--screenshot FILE.png]

mod app;
mod layout;
mod scan;
mod settings;

use std::path::PathBuf;

fn main() -> eframe::Result<()> {
    let mut screenshot: Option<PathBuf> = None;
    let mut folder: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--screenshot" => screenshot = args.next().map(PathBuf::from),
            "-h" | "--help" => {
                println!("Usage: kSpaceMonger [FOLDER] [--screenshot FILE.png]");
                return Ok(());
            }
            _ => folder = Some(PathBuf::from(a)),
        }
    }

    let settings = settings::Settings::load();
    let mut vp = eframe::egui::ViewportBuilder::default()
        .with_title("kSpaceMonger")
        .with_app_id("kspacemonger")
        .with_inner_size([800.0, 600.0])
        .with_min_inner_size([320.0, 240.0]);
    if settings.save_pos && screenshot.is_none() {
        if let Some(w) = settings.window {
            vp = vp.with_position([w[0], w[1]]).with_inner_size([w[2].max(320.0), w[3].max(240.0)]);
        }
    }
    if screenshot.is_some() {
        // Never steal focus from what the person is doing.
        vp = vp.with_active(false);
    }
    let options = eframe::NativeOptions { viewport: vp, ..Default::default() };
    eframe::run_native(
        "kSpaceMonger",
        options,
        Box::new(move |cc| Ok(Box::new(app::SpaceApp::new(cc, settings, folder, screenshot)))),
    )
}
