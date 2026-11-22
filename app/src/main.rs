#![cfg_attr(windows, windows_subsystem = "windows")]
//! LyricsOSC — synced lyrics & live transcription for the VRChat chatbox.
//! Created by Happyuky7 (https://happyuky7.com). Copyright (C) Happyuky7 and LyricsOSC contributors.
//! Licensed under GPL-3.0-or-later.
mod audio;
mod axis;
mod config;
mod engine;
mod i18n;
mod lyrics;
mod media;
mod monitor;
mod osc;
mod romanize;
mod translate;
mod update;
mod whisper;

use config::{Config, Mode};
use eframe::egui;
use engine::{LyricStatus, Shared, State, TranslateStatus};
use i18n::t;
use std::sync::{Arc, Mutex};

const ACCENT: egui::Color32 = egui::Color32::from_rgb(255, 92, 138);

/// Font fallbacks so Chinese, Japanese, Korean, Thai, Cyrillic and emoji render.
fn load_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let candidates: &[(&str, &[&str])] = &[
        ("cjk_sc", &["C:\\Windows\\Fonts\\msyh.ttc", "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
                     "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc", "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc"]),
        ("cjk_tc", &["C:\\Windows\\Fonts\\msjh.ttc"]),
        ("ja", &["C:\\Windows\\Fonts\\YuGothM.ttc", "C:\\Windows\\Fonts\\meiryo.ttc"]),
        ("ko", &["C:\\Windows\\Fonts\\malgun.ttf"]),
        ("th", &["C:\\Windows\\Fonts\\LeelawUI.ttf", "/usr/share/fonts/truetype/noto/NotoSansThai-Regular.ttf"]),
        ("emoji", &["C:\\Windows\\Fonts\\seguiemj.ttf", "/usr/share/fonts/truetype/noto/NotoEmoji-Regular.ttf"]),
        ("sym", &["C:\\Windows\\Fonts\\seguisym.ttf", "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"]),
    ];
    for (name, paths) in candidates {
        if let Some(bytes) = paths.iter().find_map(|p| std::fs::read(p).ok()) {
            fonts.font_data.insert(name.to_string(), egui::FontData::from_owned(bytes));
            for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                fonts.families.entry(fam).or_default().push(name.to_string());
            }
        }
    }
    ctx.set_fonts(fonts);
}

fn load_png(path: &std::path::Path) -> Option<(Vec<u8>, u32, u32)> {
    let dec = png::Decoder::new(std::fs::File::open(path).ok()?);
    let mut reader = dec.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf[..info.buffer_size()].to_vec(),
        png::ColorType::Rgb => buf[..info.buffer_size()].chunks(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        _ => return None,
    };
    Some((rgba, info.width, info.height))
}

#[derive(PartialEq, Clone, Copy)]
enum Tab { Main, Chatbox, Translate, Voice, Monitor, Log, About }

struct Gui {
    state: State,
    cfg: Config,
    /// Last config written to config.json (to detect unsaved changes)
    saved: Config,
    saved_at: Option<std::time::Instant>,
    confirm_reset: bool,
    tab: Tab,
    logo: Option<egui::TextureHandle>,
    banner: Option<egui::TextureHandle>,
    update: update::Shared,
}

impl Gui {
    fn push_cfg(&self) {
        let mut g = self.state.lock().unwrap();
        g.cfg = self.cfg.clone();
        g.cfg_dirty = true;
    }

    fn lang_picker(&self, ui: &mut egui::Ui) {
        let cur = if self.cfg.ui_language == "auto" { "Auto".to_string() } else {
            i18n::languages().into_iter().find(|(c, _)| *c == self.cfg.ui_language).map(|x| x.1).unwrap_or_default()
        };
        let mut lang = self.cfg.ui_language.clone();
        egui::ComboBox::from_id_salt("lang").selected_text(format!("🌐 {cur}")).show_ui(ui, |ui| {
            ui.selectable_value(&mut lang, "auto".to_string(), "Auto");
            for (code, name) in i18n::languages() {
                ui.selectable_value(&mut lang, code.to_string(), name);
            }
        });
        if lang != self.cfg.ui_language {
            ui.memory_mut(|m| m.data.insert_temp(egui::Id::new("lang_pick"), lang));
        }
    }

    fn header(&self, ui: &mut egui::Ui) {
        // Banner (key art + app name) across the full width, language + version on top of it
        if let Some(tex) = &self.banner {
            let w = ui.available_width();
            let h = w * tex.size()[1] as f32 / tex.size()[0] as f32;
            let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
            egui::Image::new(tex).rounding(10.0).paint_at(ui, rect);
            // Tagline in the UI language (same position as in tools/make_banner.py)
            let p = ui.painter();
            let size = (h * 0.075).max(11.0);
            let x = rect.left() + w * 0.045 + 4.0;
            let y1 = rect.top() + h * 0.62;
            let font = egui::FontId::proportional(size);
            let g1 = p.layout_no_wrap(t("banner_1"), font.clone(), egui::Color32::WHITE);
            let g2 = p.layout_no_wrap(t("banner_2"), font, egui::Color32::from_rgb(255, 205, 228));
            let line2_y = y1 + h * 0.095;
            // translucent violet panel with pink border so the tagline reads over bright art
            let pad = size * 0.5;
            let panel = egui::Rect::from_min_max(
                egui::pos2(x - pad, y1 - pad * 0.5),
                egui::pos2(x + g1.size().x.max(g2.size().x) + pad, line2_y + g2.size().y + pad * 0.5));
            p.rect(panel, pad, egui::Color32::from_rgba_unmultiplied(60, 14, 70, 190),
                   egui::Stroke::new(1.0_f32, ACCENT.gamma_multiply(0.7)));
            p.galley(egui::pos2(x, y1), g1, egui::Color32::WHITE);
            p.galley(egui::pos2(x, line2_y), g2, egui::Color32::WHITE);
            ui.painter().text(rect.left_bottom() + egui::vec2(10.0, -8.0), egui::Align2::LEFT_BOTTOM,
                format!("v{}", env!("CARGO_PKG_VERSION")), egui::FontId::proportional(11.0),
                egui::Color32::from_white_alpha(170));
            return;
        }
        ui.horizontal(|ui| {
            if let Some(tex) = &self.logo {
                ui.add(egui::Image::new(tex).fit_to_exact_size(egui::vec2(56.0, 56.0)).rounding(12.0));
            } else {
                ui.label(egui::RichText::new("🎵").size(40.0));
            }
            ui.vertical(|ui| {
                ui.add_space(6.0);
                ui.label(egui::RichText::new("LyricsOSC").size(24.0).strong().color(ACCENT));
                ui.label(egui::RichText::new(format!("v{} · VRChat OSC", env!("CARGO_PKG_VERSION"))).small().weak());
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| self.lang_picker(ui));
        });
    }
}

fn mb(x: f64) -> String {
    if x >= 1024.0 { format!("{:.2} GB", x / 1024.0) } else { format!("{x:.0} MB") }
}

impl eframe::App for Gui {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        ctx.request_repaint_after(std::time::Duration::from_millis(200));
        let (sessions, audio_opts, gpus, track, pos, status, chatbox, logs, paused, usage, voice_running, py_ready, tr_status) = {
            let g = self.state.lock().unwrap();
            (g.sessions.clone(), g.audio_options.clone(), g.gpus.clone(), g.track.clone(), g.position,
             g.lyric_status, g.chatbox.clone(), g.logs.clone(), g.paused, g.usage.clone(), g.voice_running, g.python_ready,
             g.translate_status.clone())
        };
        let before = serde_json::to_string(&self.cfg).unwrap_or_default();

        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.add_space(6.0);
            self.header(ui);
            if let Some(lang) = ui.memory_mut(|m| m.data.remove_temp::<String>(egui::Id::new("lang_pick"))) {
                self.cfg.ui_language = lang;
                i18n::set_language(&self.cfg.ui_language);
                self.state.lock().unwrap().refresh_audio = true; // relabel audio sources
            }
            ui.add_space(4.0);
            // ---------------- preview (below the logo)
            egui::Frame::none().fill(egui::Color32::from_rgb(22, 24, 30)).rounding(10.0)
                .stroke(egui::Stroke::new(1.0_f32, ACCENT.gamma_multiply(0.5))).inner_margin(12.0).show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.set_min_height(84.0);
                    ui.vertical_centered(|ui| {
                        let txt = if chatbox.is_empty() { t("waiting") } else { chatbox.clone() };
                        ui.label(egui::RichText::new(txt).size(16.0).color(egui::Color32::WHITE));
                    });
                });
            ui.horizontal(|ui| {
                if let Some(tr) = &track {
                    let st = match status {
                        LyricStatus::Searching => t("st_searching"), LyricStatus::Synced => t("st_synced"),
                        LyricStatus::Plain => t("st_plain"), LyricStatus::None => t("st_none"), LyricStatus::Idle => "—".into(),
                    };
                    ui.label(format!("{} {} · {} · {}: {} · {:.1}s", if tr.playing { "▶" } else { "⏸" }, tr.source_id, tr.app, t("lyrics"), st, pos));
                } else {
                    ui.label(t("no_player"));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(if paused { t("resume") } else { t("pause") }).clicked() {
                        self.state.lock().unwrap().paused = !paused;
                    }
                    ui.label(egui::RichText::new(format!("CPU {:.1}% · RAM {}", usage.app_cpu + usage.voice_cpu,
                        mb(usage.app_ram_mb + usage.voice_ram_mb))).small().weak());
                });
            });
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                for (tab, key) in [(Tab::Main, "tab_main"), (Tab::Chatbox, "tab_chatbox"), (Tab::Translate, "tab_translate"),
                                   (Tab::Voice, "tab_voice"),
                                   (Tab::Monitor, "tab_monitor"), (Tab::Log, "tab_log"), (Tab::About, "tab_about")] {
                    let has_update = tab == Tab::About && matches!(*self.update.lock().unwrap(), update::Status::Available(_));
                    let label = if has_update { format!("{} ●", t(key)) } else { t(key) };
                    ui.selectable_value(&mut self.tab, tab, label);
                }
                if self.banner.is_some() {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| self.lang_picker(ui));
                }
            });
            ui.add_space(4.0);
        });

        // ---------------- save / defaults bar
        egui::TopBottomPanel::bottom("savebar").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let unsaved = serde_json::to_string(&self.cfg).ok() != serde_json::to_string(&self.saved).ok();
                if ui.add_enabled(unsaved, egui::Button::new(t("save")).fill(if unsaved { ACCENT.gamma_multiply(0.7) } else { egui::Color32::TRANSPARENT })).clicked() {
                    self.cfg.save();
                    self.saved = self.cfg.clone();
                    self.saved_at = Some(std::time::Instant::now());
                }
                if self.confirm_reset {
                    ui.label(t("reset_confirm"));
                    if ui.button("✔").clicked() {
                        let lang = self.cfg.ui_language.clone();
                        self.cfg = Config { ui_language: lang, ..Config::default() };
                        self.confirm_reset = false;
                    }
                    if ui.button("✖").clicked() { self.confirm_reset = false; }
                } else if ui.button(t("reset_defaults")).clicked() {
                    self.confirm_reset = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if unsaved {
                        ui.colored_label(egui::Color32::from_rgb(255, 190, 90), t("unsaved"));
                    } else if self.saved_at.map_or(false, |s| s.elapsed().as_secs() < 3) {
                        ui.colored_label(egui::Color32::from_rgb(120, 220, 140), t("saved"));
                    }
                    ui.label(egui::RichText::new(config::config_path().display().to_string()).small().weak());
                });
            });
            ui.add_space(4.0);
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match self.tab {
                Tab::Main => self.tab_main(ui, &sessions, &audio_opts),
                Tab::Chatbox => self.tab_chatbox(ui),
                Tab::Translate => self.tab_translate(ui, &tr_status),
                Tab::Voice => self.tab_voice(ui, &gpus, py_ready),
                Tab::Monitor => tab_monitor(ui, &usage, voice_running, &gpus, self.cfg.gpu_index),
                Tab::Log => {
                    for l in &logs {
                        ui.label(egui::RichText::new(l).monospace().size(11.0));
                    }
                }
                Tab::About => self.tab_about(ui),
            });
        });

        if serde_json::to_string(&self.cfg).unwrap_or_default() != before {
            self.push_cfg();
        }
    }
}

impl Gui {
    fn tab_main(&mut self, ui: &mut egui::Ui, sessions: &[media::Track], audio_opts: &[audio::AudioOption]) {
        egui::Grid::new("main").num_columns(2).spacing([12.0, 10.0]).show(ui, |ui| {
            ui.label(t("mode"));
            egui::ComboBox::from_id_salt("mode").width(360.0)
                .selected_text(match self.cfg.mode { Mode::Auto => t("mode_auto"), Mode::Lyrics => t("mode_lyrics"), Mode::Audio => t("mode_audio") })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.cfg.mode, Mode::Auto, t("mode_auto"));
                    ui.selectable_value(&mut self.cfg.mode, Mode::Lyrics, t("mode_lyrics"));
                    ui.selectable_value(&mut self.cfg.mode, Mode::Audio, t("mode_audio"));
                });
            ui.end_row();

            ui.label(t("player"));
            let sel = match &self.cfg.preferred_source {
                None => t("player_auto"),
                Some(id) => sessions.iter().find(|s| &s.source_id == id).map(|s| format!("{} — {}", s.source_id, s.title))
                    .unwrap_or(format!("{id} (—)")),
            };
            egui::ComboBox::from_id_salt("src").width(360.0).selected_text(sel).show_ui(ui, |ui| {
                ui.selectable_value(&mut self.cfg.preferred_source, None, t("player_auto"));
                for s in sessions {
                    let lbl = format!("{} {} — {}{}", if s.playing { "▶" } else { "⏸" }, s.source_id,
                                      if s.artist.is_empty() { String::new() } else { format!("{} - ", s.artist) }, s.title);
                    ui.selectable_value(&mut self.cfg.preferred_source, Some(s.source_id.clone()), lbl);
                }
            });
            ui.end_row();

            ui.label(t("audio"));
            ui.horizontal(|ui| {
                let sel = audio_opts.iter().find(|o| o.id == self.cfg.audio_source).map(|o| o.label.clone())
                    .unwrap_or(self.cfg.audio_source.clone());
                egui::ComboBox::from_id_salt("audio").width(320.0).selected_text(sel).show_ui(ui, |ui| {
                    for o in audio_opts {
                        ui.selectable_value(&mut self.cfg.audio_source, o.id.clone(), &o.label);
                    }
                });
                if ui.button("↻").on_hover_text(t("refresh")).clicked() {
                    self.state.lock().unwrap().refresh_audio = true;
                }
            });
            ui.end_row();
            ui.label("");
            ui.label(egui::RichText::new(t("audio_hint")).small().weak());
            ui.end_row();

            ui.label(t("offset"));
            ui.add(egui::Slider::new(&mut self.cfg.lyric_offset, -5.0..=5.0).suffix(" s").step_by(0.1));
            ui.end_row();
        });
    }

    fn tab_chatbox(&mut self, ui: &mut egui::Ui) {
        if self.cfg.axis_enabled {
            ui.colored_label(egui::Color32::from_rgb(255, 190, 90), format!("ℹ {}", t("axis_owns_chatbox")));
        }
        ui.add_enabled_ui(!self.cfg.axis_enabled, |ui| {
            let mut on = self.cfg.osc_enabled && !self.cfg.axis_enabled;
            if ui.checkbox(&mut on, t("osc_enable")).changed() {
                self.cfg.osc_enabled = on;
            }
        });
        egui::Grid::new("osc").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            ui.label(t("host")); ui.text_edit_singleline(&mut self.cfg.osc_host); ui.end_row();
            ui.label(t("port")); ui.add(egui::DragValue::new(&mut self.cfg.osc_port).range(1..=65535)); ui.end_row();
            ui.label(t("interval"));
            ui.add(egui::Slider::new(&mut self.cfg.osc_interval, 1.0..=10.0).suffix(" s").step_by(0.1)); ui.end_row();
            ui.label(t("title_fmt"));
            ui.text_edit_singleline(&mut self.cfg.title_template).on_hover_text("{title} {artist} {album} {app}"); ui.end_row();
            ui.label("");
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new(t("variables")).small().weak());
                for var in ["{title}", "{artist}", "{album}", "{app}", "🎵", "♪", "🎧"] {
                    if ui.small_button(var).clicked() {
                        if !self.cfg.title_template.is_empty() && !self.cfg.title_template.ends_with(' ') {
                            self.cfg.title_template.push(' ');
                        }
                        self.cfg.title_template.push_str(var);
                    }
                }
            });
            ui.end_row();
            ui.label(t("paused_text")); ui.text_edit_singleline(&mut self.cfg.paused_text); ui.end_row();
        });
        ui.checkbox(&mut self.cfg.show_title, t("show_title"));
        ui.checkbox(&mut self.cfg.show_progress, t("progress"));
        ui.checkbox(&mut self.cfg.show_next_line, t("next_line"));
        ui.checkbox(&mut self.cfg.osc_sound, t("osc_sound"));
        ui.checkbox(&mut self.cfg.osc_typing_indicator, t("typing"));
        ui.checkbox(&mut self.cfg.clear_when_paused, t("clear_paused"));

        ui.add_space(10.0);
        ui.separator();
        ui.heading("Axis");
        ui.add_enabled_ui(cfg!(windows), |ui| {
            ui.checkbox(&mut self.cfg.axis_enabled, t("axis_enable"));
            ui.label(egui::RichText::new(t("axis_hint")).small().weak());
            ui.horizontal(|ui| {
                ui.label("DLL");
                ui.text_edit_singleline(&mut self.cfg.axis_dll);
            });
        });
        ui.horizontal(|ui| {
            ui.label(t("get_axis"));
            ui.hyperlink_to("axis.void-space.com", update::AXIS_SITE);
        });
    }

    fn tab_translate(&mut self, ui: &mut egui::Ui, status: &TranslateStatus) {
        // ---- romanization
        ui.heading(t("romanize_head"));
        ui.checkbox(&mut self.cfg.romanize, t("romanize_enable"));
        ui.add_enabled_ui(self.cfg.romanize, |ui| {
            ui.horizontal(|ui| {
                ui.radio_value(&mut self.cfg.romanize_mode, "below".to_string(), t("romanize_below"));
                ui.radio_value(&mut self.cfg.romanize_mode, "replace".to_string(), t("romanize_replace"));
            });
        });
        ui.label(egui::RichText::new(t("romanize_hint")).small().weak());

        // ---- translation
        ui.add_space(12.0);
        ui.separator();
        ui.heading(t("translate_head"));
        ui.checkbox(&mut self.cfg.translate, t("translate_enable"));
        ui.add_enabled_ui(self.cfg.translate, |ui| {
            egui::Grid::new("translate").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                ui.label(t("translate_to"));
                let langs = i18n::languages();
                let sel = if self.cfg.translate_to == "auto" { t("translate_auto") } else {
                    langs.iter().find(|(c, _)| *c == self.cfg.translate_to).map(|x| x.1.clone()).unwrap_or(self.cfg.translate_to.clone())
                };
                egui::ComboBox::from_id_salt("tr_to").width(260.0).selected_text(sel).show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.cfg.translate_to, "auto".to_string(), t("translate_auto"));
                    for (code, name) in &langs {
                        ui.selectable_value(&mut self.cfg.translate_to, code.to_string(), name);
                    }
                });
                ui.end_row();

                ui.label(t("provider"));
                let name = |p: &str| match p { "deepl" => t("provider_deepl"), "libretranslate" => t("provider_libre"), _ => t("provider_mymemory") };
                egui::ComboBox::from_id_salt("tr_provider").width(260.0).selected_text(name(&self.cfg.translate_provider)).show_ui(ui, |ui| {
                    for p in ["mymemory", "deepl", "libretranslate"] {
                        ui.selectable_value(&mut self.cfg.translate_provider, p.to_string(), name(p));
                    }
                });
                ui.end_row();

                match self.cfg.translate_provider.as_str() {
                    "deepl" => {
                        ui.label(t("deepl_key"));
                        ui.add(egui::TextEdit::singleline(&mut self.cfg.deepl_key).password(true).desired_width(260.0));
                        ui.end_row();
                        ui.label("");
                        ui.label(egui::RichText::new(t("deepl_hint")).small().weak());
                        ui.end_row();
                    }
                    "libretranslate" => {
                        ui.label(t("libre_url"));
                        ui.add(egui::TextEdit::singleline(&mut self.cfg.libre_url).desired_width(260.0));
                        ui.end_row();
                        ui.label(t("libre_key"));
                        ui.add(egui::TextEdit::singleline(&mut self.cfg.libre_key).password(true).desired_width(260.0));
                        ui.end_row();
                        ui.label("");
                        ui.label(egui::RichText::new(t("libre_hint")).small().weak());
                        ui.end_row();
                    }
                    _ => {
                        ui.label(t("mymemory_email"));
                        ui.add(egui::TextEdit::singleline(&mut self.cfg.mymemory_email).desired_width(260.0));
                        ui.end_row();
                        ui.label("");
                        ui.label(egui::RichText::new(t("mymemory_hint")).small().weak());
                        ui.end_row();
                    }
                }
            });
        });
        ui.label(egui::RichText::new(t("translate_hint")).small().weak());

        // ---- status (fed by the engine)
        if self.cfg.translate {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(format!("{}:", t("tr_status")));
                let ok = egui::Color32::from_rgb(120, 220, 140);
                let warn = egui::Color32::from_rgb(255, 190, 90);
                let err = egui::Color32::from_rgb(255, 140, 140);
                match status {
                    TranslateStatus::Idle => { ui.label(egui::RichText::new("—").weak()); }
                    TranslateStatus::Working => { ui.spinner(); ui.label(t("tr_working")); }
                    TranslateStatus::Done => { ui.colored_label(ok, t("tr_done")); }
                    TranslateStatus::Same => { ui.colored_label(ok, t("tr_same")); }
                    TranslateStatus::Error(e) => {
                        let msg = match e {
                            translate::Error::Quota => t("tr_quota"),
                            translate::Error::Unsupported => t("tr_unsupported"),
                            translate::Error::NoKey => t("tr_no_key"),
                            translate::Error::Other(m) => format!("{}: {m}", t("tr_error")),
                        };
                        ui.colored_label(if matches!(e, translate::Error::Other(_)) { err } else { warn }, msg);
                    }
                }
            });
        }
    }

    fn tab_about(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if let Some(tex) = &self.logo {
                ui.add(egui::Image::new(tex).fit_to_exact_size(egui::vec2(72.0, 72.0)));
            }
            ui.vertical(|ui| {
                ui.label(egui::RichText::new("LyricsOSC").size(24.0).strong().color(ACCENT));
                ui.label(format!("{} {}", t("version"), update::CURRENT));
                ui.horizontal(|ui| {
                    ui.label(t("created_by"));
                    ui.hyperlink_to("Happyuky7", update::AUTHOR_SITE);
                });
            });
        });
        ui.add_space(8.0);
        ui.label(t("about"));

        // ---- updates
        ui.add_space(12.0);
        ui.separator();
        ui.heading(t("updates"));
        let status = self.update.lock().unwrap().clone();
        ui.horizontal(|ui| {
            let checking = status == update::Status::Checking;
            if ui.add_enabled(!checking, egui::Button::new(t("check_updates"))).clicked() {
                update::check(self.update.clone());
            }
            match &status {
                update::Status::Checking => { ui.spinner(); ui.label(t("checking")); }
                update::Status::UpToDate => { ui.colored_label(egui::Color32::from_rgb(120, 220, 140), t("up_to_date")); }
                update::Status::Available(v) => {
                    ui.colored_label(egui::Color32::from_rgb(255, 190, 90), t("update_available").replace("{version}", v));
                }
                update::Status::Failed => { ui.colored_label(egui::Color32::from_rgb(255, 140, 140), t("update_failed")); }
                update::Status::Idle => {}
            }
        });
        if let update::Status::Available(_) = status {
            ui.hyperlink_to(t("download_update"), format!("{}/download", update::WEBSITE));
        }

        // ---- links
        ui.add_space(12.0);
        ui.separator();
        ui.heading(t("links"));
        egui::Grid::new("links").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label(t("website")); ui.hyperlink_to("lyricsosc.happyuky7.com", update::WEBSITE); ui.end_row();
            ui.label(t("docs")); ui.hyperlink_to("lyricsosc.happyuky7.com/docs", format!("{}/docs", update::WEBSITE)); ui.end_row();
            ui.label("GitHub"); ui.hyperlink_to("Happyuky7/LyricsOSC", update::REPO); ui.end_row();
            ui.label(t("author")); ui.hyperlink_to("happyuky7.com", update::AUTHOR_SITE); ui.end_row();
            ui.label("Axis"); ui.hyperlink_to("axis.void-space.com", update::AXIS_SITE); ui.end_row();
        });

        // ---- licences
        ui.add_space(12.0);
        ui.separator();
        ui.heading(t("licenses"));
        ui.label("Copyright © Happyuky7 and LyricsOSC contributors");
        ui.horizontal(|ui| {
            ui.label(t("license_code"));
            ui.hyperlink_to("GPL-3.0-or-later", "https://www.gnu.org/licenses/gpl-3.0.html");
        });
        ui.horizontal(|ui| {
            ui.label(t("license_art"));
            ui.hyperlink_to("CC BY-NC-ND 4.0", "https://creativecommons.org/licenses/by-nc-nd/4.0/");
        });
        ui.horizontal(|ui| {
            ui.label(t("lyrics_by"));
            ui.hyperlink_to("LRCLIB", "https://lrclib.net");
        });
        ui.label(egui::RichText::new("faster-whisper (MIT) · egui (MIT/Apache-2.0)").small().weak());
    }

    fn tab_voice(&mut self, ui: &mut egui::Ui, gpus: &[whisper::Gpu], py_ready: bool) {
        if !py_ready {
            ui.colored_label(egui::Color32::from_rgb(255, 180, 80), format!("⚠ {}", t("python_missing")));
            let installing = self.state.lock().unwrap().installing_voice;
            ui.horizontal(|ui| {
                if ui.add_enabled(!installing, egui::Button::new(format!("⬇ {}", t("install_voice"))).fill(ACCENT.gamma_multiply(0.7))).clicked() {
                    self.state.lock().unwrap().install_voice = true;
                }
                if installing { ui.spinner(); ui.label(t("installing")); }
            });
            ui.label(egui::RichText::new(t("install_voice_hint")).small().weak());
            ui.add_space(8.0);
        }
        egui::Grid::new("voice").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            ui.label(t("model"));
            ui.vertical(|ui| {
                egui::ComboBox::from_id_salt("model").selected_text(&self.cfg.whisper_model).show_ui(ui, |ui| {
                    for m in ["tiny", "base", "small", "medium", "large-v3-turbo", "large-v3"] {
                        ui.selectable_value(&mut self.cfg.whisper_model, m.to_string(), m);
                    }
                });
                ui.label(egui::RichText::new(t("model_hint")).small().weak());
            });
            ui.end_row();

            ui.label(t("device"));
            egui::ComboBox::from_id_salt("dev").selected_text(match self.cfg.whisper_device.as_str() {
                "cpu" => t("device_cpu"), "cuda" => t("device_cuda"), _ => t("device_auto") })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.cfg.whisper_device, "auto".into(), t("device_auto"));
                    ui.selectable_value(&mut self.cfg.whisper_device, "cuda".into(), t("device_cuda"));
                    ui.selectable_value(&mut self.cfg.whisper_device, "cpu".into(), t("device_cpu"));
                });
            ui.end_row();

            ui.label(t("gpu"));
            if gpus.is_empty() {
                ui.label(t("no_gpu"));
            } else {
                let sel = gpus.iter().find(|g| g.index == self.cfg.gpu_index)
                    .map(|g| format!("#{} {} ({})", g.index, g.name, mb(g.memory_mb as f64))).unwrap_or_default();
                egui::ComboBox::from_id_salt("gpu").width(320.0).selected_text(sel).show_ui(ui, |ui| {
                    for g in gpus {
                        ui.selectable_value(&mut self.cfg.gpu_index, g.index, format!("#{} {} ({})", g.index, g.name, mb(g.memory_mb as f64)));
                    }
                });
            }
            ui.end_row();

            ui.label(t("compute"));
            egui::ComboBox::from_id_salt("ct").selected_text(&self.cfg.compute_type).show_ui(ui, |ui| {
                for c in ["auto", "float16", "int8_float16", "int8", "float32"] {
                    ui.selectable_value(&mut self.cfg.compute_type, c.to_string(), c);
                }
            });
            ui.end_row();

            ui.label(t("beam"));
            ui.add(egui::Slider::new(&mut self.cfg.beam_size, 1..=5));
            ui.end_row();

            ui.label(t("chunk"));
            ui.add(egui::Slider::new(&mut self.cfg.chunk_seconds, 2.0..=10.0).suffix(" s").step_by(0.5));
            ui.end_row();

            ui.label(t("languages"));
            ui.horizontal_wrapped(|ui| {
                for (code, name) in [("zh", "中文"), ("en", "English"), ("es", "Español"), ("ja", "日本語"), ("ko", "한국어"),
                                     ("pt", "Português"), ("fr", "Français"), ("ru", "Русский"), ("tl", "Filipino"),
                                     ("th", "ไทย"), ("de", "Deutsch"), ("it", "Italiano"), ("vi", "Tiếng Việt"),
                                     ("id", "Indonesia"), ("tr", "Türkçe"), ("pl", "Polski"), ("uk", "Українська")] {
                    let mut on = self.cfg.languages.iter().any(|l| l == code);
                    if ui.checkbox(&mut on, name).changed() {
                        if on { self.cfg.languages.push(code.into()) } else { self.cfg.languages.retain(|l| l != code) }
                    }
                }
            });
            ui.end_row();
        });
        ui.checkbox(&mut self.cfg.vad, t("vad")).on_hover_text(t("vad_hint"));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button(format!("🔄 {}", t("restart_voice"))).clicked() {
                self.state.lock().unwrap().restart_voice = true;
            }
            if !gpus.is_empty() && py_ready && ui.button(format!("⚡ {}", t("install_gpu"))).clicked() {
                self.state.lock().unwrap().install_gpu = true;
            }
        });
    }
}

fn bar(ui: &mut egui::Ui, label: &str, frac: f32, text: String) {
    ui.horizontal(|ui| {
        ui.add_sized([150.0, 18.0], egui::Label::new(label));
        ui.add(egui::ProgressBar::new(frac.clamp(0.0, 1.0)).text(text).fill(ACCENT.gamma_multiply(0.8)).desired_width(320.0));
    });
}

fn tab_monitor(ui: &mut egui::Ui, u: &monitor::Usage, voice_running: bool, gpus: &[whisper::Gpu], gpu_index: u32) {
    let total_ram = 16.0 * 1024.0; // scale only; the text shows the real value
    ui.heading("LyricsOSC");
    bar(ui, &t("cpu"), u.app_cpu / 100.0, format!("{:.1}%", u.app_cpu));
    bar(ui, &t("ram"), (u.app_ram_mb / 1024.0) as f32, mb(u.app_ram_mb));
    ui.add_space(8.0);
    ui.heading(t("voice_engine"));
    if voice_running {
        bar(ui, &t("cpu"), u.voice_cpu / 100.0, format!("{:.1}%", u.voice_cpu));
        bar(ui, &t("ram"), (u.voice_ram_mb / total_ram) as f32 * 4.0, mb(u.voice_ram_mb));
        if u.voice_vram_mb > 0.0 {
            let tot = u.gpu_vram_total_mb.unwrap_or(8192.0);
            bar(ui, &t("vram"), (u.voice_vram_mb / tot) as f32, mb(u.voice_vram_mb));
        }
    } else {
        ui.label(egui::RichText::new(t("not_running")).weak());
    }
    if let (Some(util), Some(used), Some(tot)) = (u.gpu_util, u.gpu_vram_used_mb, u.gpu_vram_total_mb) {
        ui.add_space(8.0);
        let name = gpus.iter().find(|g| g.index == gpu_index).map(|g| g.name.clone()).unwrap_or("GPU".into());
        ui.heading(name);
        bar(ui, &t("gpu_use"), util / 100.0, format!("{util:.0}%"));
        bar(ui, &t("vram"), (used / tot) as f32, format!("{} / {}", mb(used), mb(tot)));
    }
    ui.add_space(8.0);
    ui.label(egui::RichText::new(format!("Total: CPU {:.1}% · RAM {}", u.app_cpu + u.voice_cpu, mb(u.app_ram_mb + u.voice_ram_mb))).strong());
}

/// Tab shown at startup. `LYRICSOSC_TAB=about|voice|chatbox|translate|monitor|log` overrides it (handy for testing).
fn start_tab() -> Tab {
    match std::env::var("LYRICSOSC_TAB").unwrap_or_default().to_lowercase().as_str() {
        "chatbox" => Tab::Chatbox,
        "translate" => Tab::Translate,
        "voice" => Tab::Voice,
        "monitor" => Tab::Monitor,
        "log" => Tab::Log,
        "about" => Tab::About,
        _ => Tab::Main,
    }
}

fn main() -> eframe::Result<()> {
    let cfg = Config::load();
    i18n::set_language(&cfg.ui_language);
    let state: State = Arc::new(Mutex::new(Shared { cfg: cfg.clone(), refresh_audio: true, ..Default::default() }));
    engine::spawn(state.clone());

    let icon = load_png(&config::resolve("assets/icon.png"))
        .map(|(rgba, w, h)| Arc::new(egui::IconData { rgba, width: w, height: h }));
    let mut vp = egui::ViewportBuilder::default().with_inner_size([660.0, 820.0]).with_min_inner_size([520.0, 560.0])
        .with_title("LyricsOSC");
    if let Some(i) = icon { vp = vp.with_icon(i); }
    eframe::run_native("LyricsOSC", eframe::NativeOptions { viewport: vp, ..Default::default() }, Box::new(move |cc| {
        load_fonts(&cc.egui_ctx);
        let mut visuals = egui::Visuals::dark();
        visuals.selection.bg_fill = ACCENT.gamma_multiply(0.6);
        visuals.hyperlink_color = ACCENT;
        cc.egui_ctx.set_visuals(visuals);
        let logo = load_png(&config::resolve("assets/logo.png")).map(|(rgba, w, h)| {
            cc.egui_ctx.load_texture("logo", egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba),
                                     egui::TextureOptions::LINEAR)
        });
        let banner = load_png(&config::resolve("assets/banner.png")).map(|(rgba, w, h)| {
            cc.egui_ctx.load_texture("banner", egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba),
                                     egui::TextureOptions::LINEAR)
        });
        Ok(Box::new(Gui { state, saved: cfg.clone(), cfg, saved_at: None, confirm_reset: false, tab: start_tab(), logo, banner, update: {
            let u = update::Shared::default();
            update::check(u.clone()); // one silent check at startup
            u
        } }))
    }))
}
