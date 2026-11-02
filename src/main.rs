#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod fetcher;
mod parser;
mod ui;

const DEFAULT_GUIDE_URL: &str = "https://xmltvfr.fr/xmltv/xmltv_fr.zip";

fn main() -> eframe::Result {
    let mut args = std::env::args().skip(1);
    let url = match args.next() {
        Some(arg) if arg == "--help" || arg == "-h" => {
            println!("Usage: tvnet [URL-du-guide.xml[.gz|.zip]]\nTV_GUIDE_URL peut fournir l'URL si aucun argument n'est passé.");
            return Ok(());
        }
        Some(url) => url,
        None => std::env::var("TV_GUIDE_URL").unwrap_or_else(|_| DEFAULT_GUIDE_URL.to_owned()),
    };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("TVnet · Films du soir")
            .with_icon(app_icon())
            .with_inner_size([760.0, 760.0])
            .with_min_inner_size([760.0, 520.0])
            .with_max_inner_size([760.0, 4096.0]),
        ..Default::default()
    };
    eframe::run_native(
        "TVnet",
        options,
        Box::new(move |cc| Ok(Box::new(ui::TvnetApp::new(cc, url)))),
    )
}

fn app_icon() -> egui::IconData {
    let image = image::load_from_memory(include_bytes!("../packaging/appimage/tvnet.png"))
        .expect("l’icône TVnet doit être une image valide")
        .into_rgba8();
    let (width, height) = image.dimensions();
    egui::IconData { rgba: image.into_raw(), width, height }
}
