//! The source selector: a list above the bar with «Todo el sistema» and the apps that
//! have audio, sounding ones first with three moving bars, silent ones dimmed.

use super::bar::{paint_app, paint_speaker};
use super::theme::{self, GLASS_EDGE, HAIRLINE, RADIUS, SURFACE, TEXT, TEXT_QUIET, TEXT_SUPPORT, Weight};
use crate::sources::{AppEntry, icon_rgba, list_apps};
use eframe::egui::{
    self, Align2, Color32, ColorImage, Context, CornerRadius, Id, Key, Order, Pos2, Rect, Sense,
    Stroke, TextureHandle, TextureId, TextureOptions, pos2, text::LayoutJob, vec2,
};
use std::collections::HashMap;
use std::path::PathBuf;

const WIDTH: f32 = 280.0;
const ROW_H: f32 = 44.0;
const PAD: f32 = 6.0;
/// Silent apps are drawn at this opacity.
const SILENT: f32 = 0.55;
/// How far below its place the list starts, and how long it takes to rise.
const RISE_PX: f32 = 6.0;
const RISE_SECS: f32 = 0.16;
/// Height the list needs besides its rows: the window margin, the header and the padding.
const CHROME_H: f32 = 74.0;

/// Height for the rows of a list whose bottom edge is at `list_bottom`, in a window whose
/// top is at `window_top`; the rows scroll inside it. At least two rows.
fn rows_max_height(window_top: f32, list_bottom: f32) -> f32 {
    (list_bottom - window_top - CHROME_H).max(2.0 * ROW_H)
}

/// What the user picked in the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Choice {
    System,
    App(String),
}

/// The list's state: open or not, the apps found when it opened and their icons.
#[derive(Default)]
pub struct SourcePicker {
    open: bool,
    apps: Vec<AppEntry>,
    /// Icon textures by executable, loaded once.
    icons: HashMap<String, Option<TextureHandle>>,
}

impl SourcePicker {
    /// Opens the list with the apps that have audio right now.
    pub fn open(&mut self) {
        self.open_with(list_apps());
    }

    /// Opens the list with `apps`.
    pub fn open_with(&mut self, apps: Vec<AppEntry>) {
        self.apps = apps;
        self.open = true;
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The icon of `exe`, loaded from the file `path` finds the first time it is asked for.
    pub fn icon(&mut self, ctx: &Context, exe: &str, path: impl FnOnce() -> Option<PathBuf>) -> Option<TextureId> {
        let key = exe.to_ascii_lowercase();
        let handle = self.icons.entry(key.clone()).or_insert_with(|| {
            let (size, rgba) = icon_rgba(&path()?)?;
            let image = ColorImage::from_rgba_unmultiplied(size, &rgba);
            Some(ctx.load_texture(format!("icon-{key}"), image, TextureOptions::LINEAR))
        });
        handle.as_ref().map(TextureHandle::id)
    }

    /// Draws the list above `anchor` (the bar's source block). Returns the choice when one is
    /// made; Escape or a click outside closes it without one.
    pub fn show(&mut self, ctx: &Context, anchor: Rect, chosen: Option<&str>) -> Option<Choice> {
        let risen = theme::transition(ctx, Id::unique("source-rise"), self.open, RISE_SECS, 0.0);
        if !self.open {
            return None;
        }
        let apps = self.apps.clone();
        let icons: Vec<Option<TextureId>> =
            apps.iter().map(|a| self.icon(ctx, &a.exe, || a.path.clone())).collect();
        let frame = egui::Frame::NONE
            .fill(SURFACE)
            .stroke(Stroke::new(1.0, HAIRLINE))
            .corner_radius(CornerRadius::same(RADIUS))
            .inner_margin(egui::Margin::same(PAD as i8));
        let bottom = anchor.top() - 23.0;
        let max_rows = rows_max_height(ctx.content_rect().top(), bottom);
        let area = egui::Area::new(Id::unique("source-picker"))
            .order(Order::Foreground)
            .pivot(Align2::LEFT_BOTTOM)
            .fixed_pos(pos2(anchor.left() - 14.0, bottom + RISE_PX * (1.0 - risen)))
            .show(ctx, |ui| {
                frame
                    .show(ui, |ui| {
                        ui.set_width(WIDTH - 2.0 * PAD);
                        header(ui);
                        egui::ScrollArea::vertical()
                            .max_height(max_rows)
                            .show(ui, |ui| {
                                let mut picked = None;
                                let system = row(ui, None, true, "Todo el sistema", Some("Todo menos Teeth"), chosen.is_none(), false);
                                if system {
                                    picked = Some(Choice::System);
                                }
                                for (app, icon) in apps.iter().zip(icons) {
                                    let selected = chosen.is_some_and(|c| c.eq_ignore_ascii_case(&app.exe));
                                    if row(ui, icon, app.sounding, &app.name, None, selected, true) {
                                        picked = Some(Choice::App(app.exe.clone()));
                                    }
                                }
                                picked
                            })
                            .inner
                    })
                    .inner
            });
        let rect = area.response.rect;
        let top_edge = Rect::from_min_max(rect.min, pos2(rect.right(), rect.top() + 1.0));
        ctx.layer_painter(area.response.layer_id).rect_filled(top_edge.shrink2(vec2(f32::from(RADIUS), 0.0)), 0.0, GLASS_EDGE);
        let clicked_outside = ctx.input(|i| {
            i.pointer.any_click() && i.pointer.interact_pos().is_some_and(|p| !rect.contains(p) && !anchor.contains(p))
        });
        let escape = ctx.input(|i| i.key_pressed(Key::Escape));
        let picked = area.inner;
        if picked.is_some() || clicked_outside || escape {
            self.close();
        }
        picked
    }
}

/// «GRABAR»: small caps-style label with wide spacing.
fn header(ui: &mut egui::Ui) {
    let mut job = LayoutJob::default();
    let format = egui::TextFormat {
        font_id: theme::font(11.0, Weight::SemiBold),
        color: TEXT_QUIET,
        extra_letter_spacing: 1.0,
        ..Default::default()
    };
    job.append("GRABAR", 0.0, format);
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.add_space(10.0);
        ui.label(job);
    });
    ui.add_space(4.0);
}

/// One row; returns whether it was clicked.
fn row(ui: &mut egui::Ui, icon: Option<TextureId>, sounding: bool, name: &str, subtitle: Option<&str>, selected: bool, app: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
    let hover = theme::hover_t(ui.ctx(), Id::unique(("source-row", name)), response.hovered());
    let p = ui.painter();
    let fill = if selected { 26.0 } else { 14.0 * hover };
    if fill > 0.0 {
        p.rect_filled(rect, CornerRadius::same(6), Color32::from_white_alpha(fill as u8));
    }
    let fade = if sounding || selected { 1.0 } else { SILENT };
    let ink = |c: Color32| c.gamma_multiply(fade);
    let icon_c = pos2(rect.left() + 20.0, rect.center().y);
    match (icon, app) {
        (Some(texture), _) => {
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            let tint = Color32::from_white_alpha((255.0 * fade) as u8);
            p.image(texture, Rect::from_center_size(icon_c, vec2(18.0, 18.0)), uv, tint);
        }
        (None, true) => paint_app(p, icon_c, ink(TEXT_SUPPORT)),
        (None, false) => paint_speaker(p, icon_c, ink(TEXT_SUPPORT)),
    }
    let weight = if selected { Weight::Medium } else { Weight::Regular };
    let text_x = rect.left() + 40.0;
    match subtitle {
        Some(sub) => {
            p.text(pos2(text_x, rect.center().y - 1.0), Align2::LEFT_BOTTOM, name, theme::font(13.0, weight), ink(TEXT));
            p.text(pos2(text_x, rect.center().y + 1.0), Align2::LEFT_TOP, sub, theme::font(11.0, Weight::Regular), ink(TEXT_QUIET));
        }
        None => {
            p.text(pos2(text_x, rect.center().y), Align2::LEFT_CENTER, name, theme::font(13.0, weight), ink(TEXT));
        }
    }
    let mut right = rect.right() - 12.0;
    if selected {
        paint_check(p, pos2(right - 6.0, rect.center().y));
        right -= 24.0;
    }
    if sounding && app {
        paint_bars(ui.ctx(), p, pos2(right - 8.0, rect.center().y));
    }
    response.clicked()
}

fn paint_check(p: &egui::Painter, c: Pos2) {
    let stroke = Stroke::new(1.8, TEXT);
    p.line_segment([pos2(c.x - 5.0, c.y), pos2(c.x - 1.5, c.y + 3.5)], stroke);
    p.line_segment([pos2(c.x - 1.5, c.y + 3.5), pos2(c.x + 5.0, c.y - 4.0)], stroke);
}

/// Three thin bars that move while the app sounds; still without Windows animations.
fn paint_bars(ctx: &Context, p: &egui::Painter, c: Pos2) {
    let t = if theme::animations_enabled() {
        ctx.request_repaint();
        ctx.input(|i| i.time) as f32
    } else {
        0.0
    };
    for (i, phase) in [0.0_f32, 1.7, 3.1].into_iter().enumerate() {
        let h = 5.0 + 6.0 * (0.5 + 0.5 * (t * 6.0 + phase).sin());
        let x = c.x - 4.0 + i as f32 * 4.0;
        p.rect_filled(Rect::from_min_max(pos2(x - 1.0, c.y + 6.0 - h), pos2(x + 1.0, c.y + 6.0)), 0.0, TEXT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rows_get_the_room_between_the_window_top_and_the_list_bottom() {
        // Window top at 0, list bottom at 300: margins and header take 74 points.
        assert_eq!(rows_max_height(0.0, 300.0), 226.0);
        // Never less than two rows, however small the window.
        assert_eq!(rows_max_height(0.0, 60.0), 2.0 * ROW_H);
    }
}
