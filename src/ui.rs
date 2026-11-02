use crate::{fetcher, parser::{self, Film}};
use egui::{Color32, RichText, TextureHandle, TextureOptions, Vec2};
use std::{collections::{BTreeMap, BTreeSet}, fs, path::Path, sync::mpsc::{self, Receiver, Sender}, time::{Duration, Instant, SystemTime, UNIX_EPOCH}};

const IMAGE_CACHE_TTL: Duration = Duration::from_secs(365 * 24 * 60 * 60);

fn apply_dark_theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.panel_fill = Color32::from_rgb(13, 19, 31);
    style.visuals.window_fill = Color32::from_rgb(17, 25, 39);
    style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(29, 40, 59);
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(40, 58, 82);
    style.spacing.item_spacing = Vec2::new(10.0, 9.0);
    ctx.set_style(style);
}

enum UiEvent {
    Films(Result<[Vec<Film>; 2], String>, Option<SystemTime>, Option<bool>),
    Image(String, Option<([usize; 2], Vec<u8>)>, bool),
}

pub struct TvnetApp {
    url: String,
    films_today: Vec<Film>,
    films_tomorrow: Vec<Film>,
    show_tomorrow: bool,
    selected: usize,
    receiver: Receiver<UiEvent>,
    sender: Sender<UiEvent>,
    loading: bool,
    error: Option<String>,
    show_tnt: bool,
    show_bouquets: bool,
    selected_channels: BTreeSet<String>,
    hidden_channels: BTreeSet<String>,
    hide_hidden_channel_logos: bool,
    channel_search: String,
    last_downloaded: Option<SystemTime>,
    notification: Option<(String, Instant)>,
    textures: BTreeMap<String, TextureHandle>,
    requested_images: Vec<String>,
    pending_images: usize,
    dark_theme_applied: bool,
}

impl TvnetApp {
    pub fn new(cc: &eframe::CreationContext<'_>, url: String) -> Self {
        apply_dark_theme(&cc.egui_ctx);

        let (sender, receiver) = mpsc::channel();
        let last_downloaded = fetcher::cached_modified_at(&url);
        start_load(url.clone(), sender.clone(), false);
        Self {
            url, films_today: Vec::new(), films_tomorrow: Vec::new(), show_tomorrow: false,
            selected: 0, receiver, sender, loading: true,
            error: None, show_tnt: true, show_bouquets: true, selected_channels: BTreeSet::new(),
            hidden_channels: fetcher::load_hidden_channels().unwrap_or_default(), hide_hidden_channel_logos: fetcher::load_hide_hidden_channel_logos(),
            channel_search: String::new(),
            last_downloaded,
            notification: None,
            textures: BTreeMap::new(), requested_images: Vec::new(), pending_images: 0,
            dark_theme_applied: false,
        }
    }

    fn reload(&mut self) {
        let (sender, receiver) = mpsc::channel();
        start_load(self.url.clone(), sender.clone(), true);
        self.sender = sender;
        self.receiver = receiver;
        self.loading = true;
        self.error = None;
        self.films_today.clear();
        self.films_tomorrow.clear();
        self.textures.clear();
        self.requested_images.clear();
        self.pending_images = 0;
        self.selected_channels.clear();
        self.notification = None;
    }

    fn set_channel_hidden(&mut self, channel: String, hidden: bool) {
        if hidden {
            self.hidden_channels.insert(channel.clone());
            self.selected_channels.remove(&channel);
        } else {
            self.hidden_channels.remove(&channel);
        }

        if let Err(error) = fetcher::save_hidden_channels(&self.hidden_channels) {
            self.notification = Some((format!("Impossible d’enregistrer les chaînes masquées : {error}"), Instant::now()));
        }
    }

    fn receive_events(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.receiver.try_recv() {
            match event {
                UiEvent::Films(result, updated_at, refresh_status) => {
                    if updated_at.is_some() { self.last_downloaded = updated_at; }
                    if let Some(changed) = refresh_status {
                        let message = if changed {
                            "Le guide a été mis à jour.".to_owned()
                        } else {
                            "Le guide est déjà à jour : mise à jour non nécessaire.".to_owned()
                        };
                        self.notification = Some((message, Instant::now()));
                    }
                    self.loading = false;
                    match result {
                        Ok([today, tomorrow]) => {
                            self.films_today = today;
                            self.films_tomorrow = tomorrow;
                            let films = [&self.films_today, &self.films_tomorrow];
                            let mut urls = BTreeMap::new();
                            for day_films in films {
                                for film in day_films {
                                    if let Some(url) = &film.logo_url { urls.insert(url.clone(), ()); }
                                    if let Some(url) = &film.affiche_url { urls.insert(url.clone(), ()); }
                                }
                            }
                            self.requested_images = urls.keys().cloned().collect();
                            self.pending_images = self.requested_images.len();
                            for url in self.requested_images.clone() {
                                load_image_async(url, self.sender.clone(), ctx.clone());
                            }
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
                UiEvent::Image(url, Some((size, rgba)), completed) => {
                    if completed { self.pending_images = self.pending_images.saturating_sub(1); }
                    let image = egui::ColorImage::from_rgba_unmultiplied(size, &rgba);
                    self.textures.insert(url.clone(), ctx.load_texture(url, image, TextureOptions::LINEAR));
                }
                UiEvent::Image(_, None, completed) => {
                    if completed { self.pending_images = self.pending_images.saturating_sub(1); }
                }
            }
        }
    }
}

fn start_load(url: String, sender: Sender<UiEvent>, force_refresh: bool) {
    std::thread::spawn(move || {
        let (fetched, refresh_status) = if force_refresh {
            match fetcher::refresh(&url) {
                Ok((xml, changed)) => (Ok(xml), Some(changed)),
                Err(error) => (Err(error), None),
            }
        } else {
            (fetcher::fetch(&url), None)
        };
        let updated_at = fetcher::cached_modified_at(&url);
        let result = fetched.and_then(|xml| {
            let today = chrono::Local::now().date_naive();
            let tomorrow = today + chrono::Days::new(1);
            Ok([
                parser::parse_evening_movies(&xml, today)?,
                parser::parse_evening_movies(&xml, tomorrow)?,
            ])
        }).map_err(|error| error.to_string());
        let _ = sender.send(UiEvent::Films(result, updated_at, refresh_status));
    });
}

fn load_image_async(url: String, sender: Sender<UiEvent>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let cache_path = fetcher::image_cache_path(&url).ok();
        if let Some(path) = &cache_path {
            if let Ok(bytes) = fs::read(path) {
                if let Some(pixels) = decode_image(&bytes) {
                    let cache_is_fresh = fs::metadata(path).ok()
                        .and_then(|metadata| metadata.modified().ok())
                        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
                        .is_some_and(|age| age < IMAGE_CACHE_TTL);
                    let _ = sender.send(UiEvent::Image(url.clone(), Some(pixels), cache_is_fresh));
                    ctx.request_repaint();
                    if cache_is_fresh { return; }
                } else {
                    let _ = fs::remove_file(path);
                }
            }
        }

        let downloaded = reqwest::blocking::Client::builder().timeout(Duration::from_secs(8)).build()
            .and_then(|client| client.get(&url).send())
            .and_then(reqwest::blocking::Response::error_for_status)
            .and_then(reqwest::blocking::Response::bytes);
        match downloaded {
            Ok(bytes) => {
                let bytes = bytes.to_vec();
                let pixels = decode_image(&bytes);
                if let (Some(path), Some(_)) = (&cache_path, &pixels) {
                    write_image_cache(path, &bytes);
                }
                let _ = sender.send(UiEvent::Image(url, pixels, true));
            }
            Err(_) => {
                let _ = sender.send(UiEvent::Image(url, None, true));
            }
        }
        ctx.request_repaint();
    });
}

fn decode_image(bytes: &[u8]) -> Option<([usize; 2], Vec<u8>)> {
    let image = image::load_from_memory(bytes).ok()?.thumbnail(900, 1200).to_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    Some((size, image.into_raw()))
}

fn write_image_cache(path: &Path, bytes: &[u8]) {
    let Some(parent) = path.parent() else { return; };
    if fs::create_dir_all(parent).is_err() { return; }
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    let temporary = path.with_extension(format!("tmp-{nonce}"));
    if fs::write(&temporary, bytes).is_err() { return; }
    if path.exists() && fs::remove_file(path).is_err() {
        let _ = fs::remove_file(&temporary);
        return;
    }
    if fs::rename(&temporary, path).is_err() {
        let _ = fs::remove_file(&temporary);
    }
}

impl eframe::App for TvnetApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // eframe may apply the OS theme after `new`; enforce our application
        // palette once the first frame is ready.
        if !self.dark_theme_applied {
            apply_dark_theme(ctx);
            self.dark_theme_applied = true;
        }
        let mut channel_visibility_change: Option<(String, bool)> = None;
        self.receive_events(ctx);
        if self.notification.as_ref().is_some_and(|(_, started)| started.elapsed() >= Duration::from_secs(5)) {
            self.notification = None;
        }
        if self.notification.is_some() {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
        if self.loading || self.pending_images > 0 {
            ctx.request_repaint_after(Duration::from_millis(120));
        }

        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            egui::Frame::none().fill(Color32::from_rgb(17, 27, 44)).inner_margin(egui::Margin::symmetric(22.0, 15.0)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("TV").size(29.0).strong().color(Color32::from_rgb(77, 190, 255)));
                    ui.label(RichText::new("net").size(29.0).strong().color(Color32::WHITE));
                    ui.add_space(12.0);
                    ui.vertical(|ui| {
                        ui.label(RichText::new("LES FILMS DU SOIR").size(16.0).strong());
                        ui.label(RichText::new("TNT & bouquets · sélection par chaîne").size(12.0).color(Color32::from_rgb(159, 177, 202)));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                ui.add(egui::TextEdit::singleline(&mut self.channel_search)
                                    .hint_text("Rechercher une chaîne")
                                    .desired_width(190.0));
                                ui.add_space(8.0);
                                if ui.button("↻  Actualiser le guide").clicked() { self.reload(); }
                            });
                            ui.horizontal(|ui| {
                                ui.checkbox(&mut self.show_tnt, "TNT");
                                ui.checkbox(&mut self.show_bouquets, "Bouquets");
                            });
                        });
                    });
                });
            });
        });

        egui::TopBottomPanel::top("channel-picker").show(ctx, |ui| {
            egui::Frame::none().fill(Color32::from_rgb(15, 23, 37))
                .inner_margin(egui::Margin { left: 18.0, right: 18.0, top: 4.0, bottom: 0.0 }).show(ui, |ui| {
                    let mut channels = BTreeMap::new();
                    let query = normaliser_texte(&self.channel_search);
                    let day_films = if self.show_tomorrow { &self.films_tomorrow } else { &self.films_today };
                    for film in day_films {
                        if !query.is_empty() && !normaliser_texte(&film.chaine).contains(&query) { continue; }
                        channels.entry(film.chaine.clone()).or_insert_with(|| film.logo_url.clone());
                    }
                    let mut channels = channels.into_iter().collect::<Vec<_>>();
                    channels.sort_by(|(a, _), (b, _)| compare_channels(a, b));
                    ui.horizontal_top(|ui| {
                        ui.vertical(|ui| {
                            ui.label(RichText::new("CHAÎNES").size(11.0).strong().color(Color32::from_rgb(159, 177, 202)));
                            ui.label(RichText::new(if self.selected_channels.is_empty() {
                                "Toutes"
                            } else {
                                "Sélection"
                            }).size(10.0).color(Color32::from_rgb(125, 195, 240)));
                            if ui.small_button("Tout afficher").clicked() {
                                self.selected_channels.clear();
                            }
                            if ui.small_button(if self.hide_hidden_channel_logos {
                                "Voir chaînes masquées"
                            } else {
                                "Cacher chaînes masquées"
                            }).clicked() {
                                self.hide_hidden_channel_logos = !self.hide_hidden_channel_logos;
                                if let Err(error) = fetcher::save_hide_hidden_channel_logos(self.hide_hidden_channel_logos) {
                                    self.notification = Some((format!("Impossible d’enregistrer la préférence : {error}"), Instant::now()));
                                }
                            }
                        });
                        ui.add_space(8.0);
                        ui.scope(|ui| {
                            ui.style_mut().spacing.scroll = egui::style::ScrollStyle::solid();
                            egui::ScrollArea::horizontal()
                                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
                                .auto_shrink([true, false])
                                .max_height(112.0)
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        for (channel, logo_url) in channels {
                                        let selected = self.selected_channels.contains(&channel);
                                        let hidden = self.hidden_channels.contains(&channel);
                                        if hidden && self.hide_hidden_channel_logos { continue; }
                                        let frame = egui::Frame::none()
                                            .fill(if hidden { Color32::from_rgb(14, 20, 31) } else if selected { Color32::from_rgb(32, 61, 86) } else { Color32::from_rgb(20, 30, 46) })
                                            .stroke(egui::Stroke::new(if selected { 2.0_f32 } else { 1.0_f32 }, if selected { Color32::from_rgb(77, 190, 255) } else { Color32::from_rgb(39, 53, 75) }))
                                            .rounding(7.0).inner_margin(5.0)
                                            .show(ui, |ui| {
                                                ui.set_max_size(Vec2::new(66.0, 70.0));
                                                ui.set_width(66.0);
                                                ui.set_height(70.0);
                                                ui.vertical_centered(|ui| {
                                                    if let Some(texture) = logo_url.as_ref().and_then(|url| self.textures.get(url)) {
                                                        let image = egui::Image::new((texture.id(), texture.size_vec2())).max_size(Vec2::new(56.0, 37.0));
                                                        ui.add(if hidden { image.tint(Color32::from_white_alpha(75)) } else { image });
                                                    } else {
                                                        ui.label(RichText::new(channel.chars().take(3).collect::<String>().to_uppercase()).size(16.0).strong().color(if hidden { Color32::from_rgb(53, 92, 122) } else { Color32::from_rgb(91, 184, 244) }));
                                                    }
                                                    ui.label(RichText::new(&channel).size(9.0).color(if hidden { Color32::from_rgb(99, 111, 126) } else { Color32::from_rgb(210, 220, 234) }));
                                                });
                                            });
                                        let response = frame.response.interact(egui::Sense::click());
                                        response.context_menu(|ui| {
                                            if hidden {
                                                if ui.button("Afficher dans la liste").clicked() {
                                                    channel_visibility_change = Some((channel.clone(), false));
                                                    ui.close_menu();
                                                }
                                            } else if ui.button("Masquer dans la liste").clicked() {
                                                channel_visibility_change = Some((channel.clone(), true));
                                                ui.close_menu();
                                            }
                                        });
                                        if response.clicked() {
                                            if selected { self.selected_channels.remove(&channel); }
                                            else { self.selected_channels.insert(channel); }
                                        }
                                        }
                                    });
                                    ui.add_space(4.0);
                                });
                        });
                    });
                });
        });

        egui::TopBottomPanel::bottom("guide-update-time").show(ctx, |ui| {
            egui::Frame::none().fill(Color32::from_rgb(17, 27, 44))
                .inner_margin(egui::Margin::symmetric(18.0, 5.0)).show(ui, |ui| {
                    let status = self.last_downloaded.map(date_heure_francais).unwrap_or_else(|| {
                        if self.loading { "Téléchargement du guide en cours…".to_owned() }
                        else { "Date de téléchargement inconnue".to_owned() }
                    });
                    ui.horizontal(|ui| {
                        if let Some((message, _)) = &self.notification {
                            ui.colored_label(Color32::LIGHT_GREEN, message);
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(RichText::new(format!("Dernière mise à jour du guide : {status}"))
                                .size(11.0).color(Color32::from_rgb(159, 177, 202)));
                        });
                    });
                });
        });

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(Color32::from_rgb(13, 19, 31)))
            .show(ctx, |ui| {
            let today = chrono::Local::now().date_naive();
            let tomorrow = today + chrono::Days::new(1);
            ui.horizontal(|ui| {
                ui.label(RichText::new("PROGRAMMES :").size(11.0).strong().color(Color32::from_rgb(159, 177, 202)));
                if ui.selectable_value(&mut self.show_tomorrow, false,
                    format!("Aujourd’hui · {}", date_en_francais(today))).clicked() {
                    self.selected = 0;
                }
                if ui.selectable_value(&mut self.show_tomorrow, true,
                    format!("Demain · {}", date_en_francais(tomorrow))).clicked() {
                    self.selected = 0;
                }
            });
            ui.add_space(5.0);

            let films = if self.show_tomorrow { &self.films_tomorrow } else { &self.films_today };
            if self.loading {
                ui.horizontal(|ui| { ui.spinner(); ui.label("Récupération du programme TV…"); });
            }
            if let Some(error) = &self.error {
                egui::Frame::none().fill(Color32::from_rgb(69, 34, 43)).inner_margin(12.0).rounding(8.0).show(ui, |ui| {
                    ui.colored_label(Color32::LIGHT_RED, format!("Le guide n’a pas pu être chargé : {error}"));
                });
            }

            let visible_indices: Vec<usize> = films.iter().enumerate()
                .filter(|(_, film)| {
                    let type_visible = if is_tnt_channel(&film.chaine) { self.show_tnt } else { self.show_bouquets };
                    let query = normaliser_texte(&self.channel_search);
                    let search_visible = query.is_empty() || normaliser_texte(&film.chaine).contains(&query);
                    type_visible && search_visible && !self.hidden_channels.contains(&film.chaine)
                        && (self.selected_channels.is_empty() || self.selected_channels.contains(&film.chaine))
                })
                .map(|(index, _)| index)
                .collect();
            if !visible_indices.contains(&self.selected) {
                self.selected = visible_indices.first().copied().unwrap_or(0);
            }
            let section_title = if self.show_tomorrow { "FILMS DE DEMAIN SOIR" } else { "FILMS DE CE SOIR" };
            ui.heading(RichText::new(section_title).size(13.0).strong().color(Color32::from_rgb(159, 177, 202)));
            ui.add_space(4.0);
            egui::ScrollArea::vertical().id_salt("channel-film-list").auto_shrink([false, true]).show(ui, |ui| {
                    let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
                    let mut group_order = Vec::new();
                    for idx in visible_indices.iter().copied() {
                        let film = &films[idx];
                        if !groups.contains_key(&film.chaine) { group_order.push(film.chaine.clone()); }
                        groups.entry(film.chaine.clone()).or_default().push(idx);
                    }
                    group_order.sort_by(|a, b| compare_channels(a, b));
                    let mut shown_bouquet = None;
                    for channel in group_order {
                        let bouquet = bouquet_for_channel(&channel);
                        if let Some(name) = bouquet {
                            if shown_bouquet != Some(name) {
                                ui.add_space(5.0);
                                ui.label(RichText::new(format!("BOUQUET  {}", name.to_uppercase()))
                                    .size(11.0).strong().color(Color32::from_rgb(77, 190, 255)));
                                ui.add_space(2.0);
                            }
                        }
                        shown_bouquet = bouquet;
                        let indices = &groups[&channel];
                        let logo = indices.iter().find_map(|&i| films[i].logo_url.as_ref());
                        egui::Frame::none().fill(Color32::from_rgb(20, 30, 46)).stroke(egui::Stroke::new(1.0_f32, Color32::from_rgb(39, 53, 75))).rounding(10.0).inner_margin(10.0).show(ui, |ui| {
                            ui.horizontal_top(|ui| {
                                ui.vertical(|ui| {
                                    ui.set_width(90.0);
                                    ui.label(RichText::new(&channel).size(12.0).strong().color(Color32::from_rgb(210, 220, 234)));
                                    let logo_response = egui::Frame::none().fill(Color32::from_rgb(14, 21, 34)).rounding(8.0).inner_margin(6.0).show(ui, |ui| {
                                        ui.set_width(82.0);
                                        ui.set_height(78.0);
                                        ui.centered_and_justified(|ui| {
                                            if let Some(texture) = logo.and_then(|url| self.textures.get(url)) {
                                                ui.add(egui::Image::new((texture.id(), texture.size_vec2())).max_size(Vec2::new(70.0, 64.0)));
                                            } else {
                                                ui.label(RichText::new(channel.chars().take(3).collect::<String>().to_uppercase()).size(19.0).strong().color(Color32::from_rgb(91, 184, 244)));
                                            }
                                        });
                                    }).response.interact(egui::Sense::click());
                                    logo_response.context_menu(|ui| {
                                        if ui.button("Masquer dans la liste").clicked() {
                                            channel_visibility_change = Some((channel.clone(), true));
                                            ui.close_menu();
                                        }
                                    });
                                });
                                ui.add_space(8.0);
                                ui.vertical(|ui| {
                                    for &film_index in indices {
                                        let film = &films[film_index];
                                        let selected = self.selected == film_index;
                                        let bg = if selected { Color32::from_rgb(39, 75, 105) } else { Color32::TRANSPARENT };
                                        let row = egui::Frame::none().fill(bg).rounding(7.0).inner_margin(egui::Margin::symmetric(7.0, 6.0)).show(ui, |ui| {
                                            ui.horizontal_top(|ui| {
                                                // Colonne d’affiche de largeur fixe (affiche + marge droite) : la
                                                // description prend exactement le reste, sans jamais déborder.
                                                const POSTER_WIDTH: f32 = 190.0;
                                                const POSTER_HEIGHT: f32 = POSTER_WIDTH * 9.0 / 16.0; // cadre 16:9 commun à toutes les affiches
                                                const POSTER_RIGHT_MARGIN: f32 = 16.0;
                                                const POSTER_COLUMN_WIDTH: f32 = POSTER_WIDTH + POSTER_RIGHT_MARGIN;
                                                let has_poster = film.affiche_url.is_some();
                                                let poster = film.affiche_url.as_ref().and_then(|url| self.textures.get(url));
                                                let details_width = if has_poster {
                                                    (ui.available_width() - POSTER_COLUMN_WIDTH - ui.spacing().item_spacing.x).max(0.0).floor()
                                                } else {
                                                    ui.available_width()
                                                };
                                                ui.allocate_ui_with_layout(Vec2::new(details_width, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                                                    ui.set_width(details_width);
                                                    ui.add(egui::Label::new(RichText::new(&film.titre).strong().size(13.0)).wrap());
                                                    ui.label(RichText::new(format!("{} – {}", film.debut.format("%H:%M"), film.fin.format("%H:%M"))).size(11.0).color(Color32::from_rgb(125, 195, 240)));
                                                    let metadata = [
                                                        film.annee.as_deref(),
                                                        film.genres.first().map(String::as_str),
                                                        film.pays.as_deref(),
                                                    ].into_iter().flatten().filter(|value| !value.trim().is_empty()).collect::<Vec<_>>();
                                                    if !metadata.is_empty() {
                                                        ui.label(RichText::new(metadata.join(" · ")).size(10.0).color(Color32::from_rgb(125, 195, 240)));
                                                    }
                                                    let acteurs = film.acteurs.iter()
                                                        .map(|acteur| acteur.trim())
                                                        .filter(|acteur| !acteur.is_empty())
                                                        .collect::<Vec<_>>();
                                                    let distribution = if acteurs.is_empty() {
                                                        "Distribution non renseignée".to_owned()
                                                    } else {
                                                        format!("Avec : {}", acteurs.join(", "))
                                                    };
                                                    ui.add(egui::Label::new(RichText::new(distribution).size(10.0).color(Color32::from_rgb(159, 177, 202))).wrap());
                                                    if let Some(resume) = film.resume.as_deref().filter(|resume| !resume.trim().is_empty()) {
                                                        ui.add(egui::Label::new(RichText::new(resume).size(10.0).color(Color32::from_rgb(191, 202, 218))).wrap());
                                                    }
                                                });
                                                if has_poster {
                                                    ui.allocate_ui_with_layout(
                                                        Vec2::new(POSTER_COLUMN_WIDTH, 0.0),
                                                        egui::Layout::left_to_right(egui::Align::Min),
                                                        |ui| {
                                                            ui.set_width(POSTER_COLUMN_WIDTH);
                                                            if let Some(texture) = poster {
                                                                // Toutes les affiches occupent exactement le même cadre
                                                                // POSTER_WIDTH × POSTER_HEIGHT, sans déformation : l'image est
                                                                // mise à l'échelle pour couvrir le cadre puis recadrée.
                                                                // Trop haute : on garde le haut et on perd le bas.
                                                                // Trop large : on coupe à parts égales à gauche et à droite.
                                                                let source_size = texture.size_vec2();
                                                                let source_ratio = source_size.x.max(1.0) / source_size.y.max(1.0);
                                                                let target_ratio = POSTER_WIDTH / POSTER_HEIGHT;
                                                                let uv = if source_ratio < target_ratio {
                                                                    let visible = source_ratio / target_ratio;
                                                                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, visible))
                                                                } else {
                                                                    let visible = target_ratio / source_ratio;
                                                                    let left = (1.0 - visible) / 2.0;
                                                                    egui::Rect::from_min_max(egui::pos2(left, 0.0), egui::pos2(left + visible, 1.0))
                                                                };
                                                                let frame_size = Vec2::new(POSTER_WIDTH, POSTER_HEIGHT);
                                                                ui.add(
                                                                    egui::Image::new((texture.id(), frame_size))
                                                                        .uv(uv)
                                                                        .maintain_aspect_ratio(false)
                                                                        .fit_to_exact_size(frame_size),
                                                                );
                                                            }
                                                        },
                                                    );
                                                }
                                            });
                                        });
                                        if row.response.interact(egui::Sense::click()).clicked() {
                                            self.selected = film_index;
                                        }
                                    }
                                });
                            });
                        });
                        ui.add_space(9.0);
                    }
                    if !self.loading && self.error.is_none() && visible_indices.is_empty() {
                        let message = if films.is_empty() {
                            "Aucun film trouvé pour cette soirée."
                        } else {
                            "Aucun film ne correspond aux filtres sélectionnés."
                        };
                        ui.label(RichText::new(message).color(Color32::from_rgb(159, 177, 202)));
                    }
                });
        });
        if let Some((channel, hidden)) = channel_visibility_change {
            self.set_channel_hidden(channel, hidden);
        }
        if self.loading || self.pending_images > 0 { ctx.request_repaint_after(Duration::from_millis(120)); }
    }
}

fn bouquet_for_channel(channel: &str) -> Option<&'static str> {
    let name = channel.trim().to_lowercase();
    if name.starts_with("canal+") { Some("Canal+") }
    else if name.starts_with("ciné+") || name.starts_with("cine+") { Some("Ciné+") }
    else if name.starts_with("bein sports") || name.starts_with("be in sports") { Some("beIN Sports") }
    else if name.starts_with("eurosport") { Some("Eurosport") }
    else if name == "rmc" || name.starts_with("rmc ") { Some("RMC") }
    else if name == "bfm" || name.starts_with("bfm ") { Some("BFM") }
    else if name == "tf1" || name.starts_with("tf1 ") { Some("TF1") }
    else if ["france 2", "france 3", "france 4", "france 5", "france info", "france ô"]
        .iter().any(|prefix| name == *prefix || name.starts_with(&format!("{prefix} "))) { Some("France Télévisions") }
    else { None }
}

fn is_tnt_channel(channel: &str) -> bool {
    tnt_position(channel).is_some()
}

fn compare_channels(a: &str, b: &str) -> std::cmp::Ordering {
    match (tnt_position(a), tnt_position(b)) {
        (Some(a_position), Some(b_position)) => a_position.cmp(&b_position).then_with(|| a.cmp(b)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => {
            let a_bouquet = bouquet_for_channel(a).unwrap_or(a).to_lowercase();
            let b_bouquet = bouquet_for_channel(b).unwrap_or(b).to_lowercase();
            a_bouquet.cmp(&b_bouquet).then_with(|| a.to_lowercase().cmp(&b.to_lowercase()))
        }
    }
}

fn tnt_position(channel: &str) -> Option<usize> {
    let name = normaliser_texte(channel).replace(['’', '\'', '-', '.', ':'], " ")
        .split_whitespace().collect::<Vec<_>>().join(" ");
    const TNT_NAMES: &[&str] = &[
        "tf1", "france 2", "france 3", "france 4", "france 5", "m6", "arte",
        "lcp", "lcp assemblee nationale", "lcp public senat", "public senat", "w9", "tmc", "tfx", "gulli",
        "bfm tv", "bfmtv", "cnews", "lci", "franceinfo", "france info", "cstar", "t18",
        "novo19", "novo 19", "tf1 series films", "l equipe", "equipe", "6ter", "rmc story",
        "rmc decouverte", "rmc life", "cherie 25", "paris premiere",
    ];
    TNT_NAMES.iter().position(|known| name == *known)
        // France 3 reprend le même canal TNT avec ses décrochages régionaux.
        .or_else(|| name.starts_with("france 3 ").then_some(2))
}

fn normaliser_texte(texte: &str) -> String {
    texte.trim().to_lowercase()
        .replace(['é', 'è', 'ê', 'ë'], "e")
        .replace(['à', 'â'], "a")
        .replace(['î', 'ï'], "i")
        .replace(['ô'], "o")
        .replace(['ù', 'û', 'ü'], "u")
}

fn date_en_francais(date: chrono::NaiveDate) -> String {
    use chrono::{Datelike, Weekday};
    let jour = match date.weekday() {
        Weekday::Mon => "Lundi",
        Weekday::Tue => "Mardi",
        Weekday::Wed => "Mercredi",
        Weekday::Thu => "Jeudi",
        Weekday::Fri => "Vendredi",
        Weekday::Sat => "Samedi",
        Weekday::Sun => "Dimanche",
    };
    let mois = match date.month() {
        1 => "janvier", 2 => "février", 3 => "mars", 4 => "avril",
        5 => "mai", 6 => "juin", 7 => "juillet", 8 => "août",
        9 => "septembre", 10 => "octobre", 11 => "novembre", _ => "décembre",
    };
    format!("{jour} {} {mois}", date.day())
}

fn date_heure_francais(timestamp: SystemTime) -> String {
    use chrono::Datelike;
    let date_heure = chrono::DateTime::<chrono::Local>::from(timestamp);
    format!("{} {} à {}", date_en_francais(date_heure.date_naive()), date_heure.year(), date_heure.format("%H:%M"))
}

