#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod diagnose;
mod icon;
mod player;
mod theme;
mod thumbs;
mod yt;

use eframe::egui::{
    self, Align2, Color32, CornerRadius, CursorIcon, FontId, Frame, Id, Key, LayerId, Margin, Order, Pos2, Rect,
    Response, Sense, Shadow, Stroke, StrokeKind, Ui, UiBuilder, Vec2, pos2, text::LayoutJob, vec2,
};
use player::{Control, Player};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use theme::*;
use yt::{Tools, Video};

const GAP: f32 = 22.0;
const MIN_CARD_W: f32 = 270.0;
const THUMB_RADIUS: u8 = 12;
const TOAST_FOR: Duration = Duration::from_millis(2600);
const PLAYER_BAR_H: f32 = 92.0;
const RESTART_INSTEAD_OF_PREV: f64 = 3.0;

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--diagnose") {
        print_diagnostics(args.get(2).map(String::as_str));
        return Ok(());
    }
    std::thread::spawn(yt::warm_up);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("fast.yt")
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([640.0, 460.0])
            .with_icon(Arc::new(theme::app_icon())),
        ..Default::default()
    };
    eframe::run_native(
        "fast.yt",
        options,
        Box::new(|cc| {
            cc.egui_ctx.add_bytes_loader(Arc::new(thumbs::ThumbnailLoader::default()));
            egui_extras::install_image_loaders(&cc.egui_ctx);
            theme::install(&cc.egui_ctx);
            Ok(Box::new(App::new()))
        }),
    )
}

enum Phase {
    Idle,
    Searching { started: Instant },
    Results { took: Duration },
    Failed(String),
}

type Inbox = Arc<Mutex<Option<(u64, Result<Vec<Video>, String>)>>>;

struct App {
    tools: Tools,
    query: String,
    shown_query: String,
    phase: Phase,
    videos: Vec<Video>,
    selected: Option<usize>,
    columns: usize,
    scroll_to_selected: bool,
    generation: u64,
    inbox: Inbox,
    focus_search: bool,
    toast: Option<(String, Instant)>,
    player: Player,
    queue: Vec<Video>,
    queue_pos: usize,
    now_playing: Option<Video>,
    seek_drag: Option<f64>,
    muted_from: Option<f64>,
}

impl App {
    fn new() -> Self {
        Self {
            tools: Tools::discover(),
            query: String::new(),
            shown_query: String::new(),
            phase: Phase::Idle,
            videos: Vec::new(),
            selected: None,
            columns: 1,
            scroll_to_selected: false,
            generation: 0,
            inbox: Arc::default(),
            focus_search: true,
            toast: None,
            player: Player::new(),
            queue: Vec::new(),
            queue_pos: 0,
            now_playing: None,
            seek_drag: None,
            muted_from: None,
        }
    }

    fn submit(&mut self, ctx: &egui::Context) {
        let query = self.query.trim().to_owned();
        if query.is_empty() {
            return;
        }
        if let Some(link) = yt::as_direct_link(&query) {
            let video = Video { title: link.to_owned(), id: String::new(), ..Default::default() };
            let link = link.to_owned();
            self.start_audio(vec![video], 0, Some(link), ctx);
            return;
        }
        self.generation += 1;
        let generation = self.generation;
        self.shown_query = query.clone();
        self.phase = Phase::Searching { started: Instant::now() };
        let (inbox, tools, ctx) = (self.inbox.clone(), self.tools.clone(), ctx.clone());
        std::thread::spawn(move || {
            let result = yt::search(&tools, &query);
            *inbox.lock().unwrap() = Some((generation, result));
            ctx.request_repaint();
        });
    }

    fn poll(&mut self, ctx: &egui::Context) {
        if let Some(finished_ok) = self.player.poll_finished() {
            if finished_ok && self.queue_pos + 1 < self.queue.len() {
                let (queue, next) = (std::mem::take(&mut self.queue), self.queue_pos + 1);
                self.start_audio(queue, next, None, ctx);
            } else {
                if !finished_ok {
                    self.toast("Couldn't play that one".into());
                }
                self.now_playing = None;
            }
        }

        let Some((generation, result)) = self.inbox.lock().unwrap().take() else {
            return;
        };
        if generation != self.generation {
            return;
        }
        let took = match &self.phase {
            Phase::Searching { started } => started.elapsed(),
            _ => Duration::ZERO,
        };
        match result {
            Ok(videos) => {
                self.videos = videos;
                self.selected = (!self.videos.is_empty()).then_some(0);
                self.scroll_to_selected = true;
                self.phase = Phase::Results { took };
            }
            Err(e) => self.phase = Phase::Failed(e),
        }
    }

    fn toast(&mut self, message: String) {
        self.toast = Some((message, Instant::now()));
    }

    fn play_index(&mut self, i: usize, ctx: &egui::Context) {
        if i < self.videos.len() {
            self.selected = Some(i);
            self.start_audio(self.videos.clone(), i, None, ctx);
        }
    }

    fn start_audio(&mut self, queue: Vec<Video>, pos: usize, url: Option<String>, ctx: &egui::Context) {
        let Some(video) = queue.get(pos).cloned() else { return };
        let url = url.unwrap_or_else(|| video.url());
        self.seek_drag = None;
        match self.player.play_audio(&self.tools, &url, ctx) {
            Ok(()) => {
                self.queue = queue;
                self.queue_pos = pos;
                self.now_playing = Some(video);
            }
            Err(e) => {
                self.now_playing = None;
                self.toast(format!("{e}. Install with: {}", yt::INSTALL_HINT));
            }
        }
    }

    fn skip(&mut self, delta: isize, ctx: &egui::Context) {
        if delta < 0 && self.player.status().position.unwrap_or(0.0) > RESTART_INSTEAD_OF_PREV {
            self.player.send(Control::SeekTo(0.0));
            return;
        }
        let Some(target) = self.queue_pos.checked_add_signed(delta) else { return };
        if target < self.queue.len() {
            let queue = std::mem::take(&mut self.queue);
            self.start_audio(queue, target, None, ctx);
        }
    }

    fn toggle_mute(&mut self) {
        match self.muted_from.take() {
            Some(v) => self.player.send(Control::SetVolume(v)),
            None => {
                self.muted_from = Some(self.player.volume().max(1.0));
                self.player.send(Control::SetVolume(0.0));
            }
        }
    }

    fn handle_keys(&mut self, ctx: &egui::Context, search_focused: bool) {
        if search_focused {
            if ctx.input(|i| i.key_pressed(Key::Escape)) {
                ctx.memory_mut(|m| m.surrender_focus(search_id()));
            }
            return;
        }
        let (slash, ctrl_l, enter, left, right, up, down, space) = ctx.input(|i| {
            (
                i.key_pressed(Key::Slash),
                i.modifiers.command && i.key_pressed(Key::L),
                i.key_pressed(Key::Enter),
                i.key_pressed(Key::ArrowLeft),
                i.key_pressed(Key::ArrowRight),
                i.key_pressed(Key::ArrowUp),
                i.key_pressed(Key::ArrowDown),
                i.key_pressed(Key::Space),
            )
        });
        if slash || ctrl_l {
            self.focus_search = true;
        }
        if space && self.now_playing.is_some() {
            self.player.send(Control::TogglePause);
        }
        if self.videos.is_empty() || !matches!(self.phase, Phase::Results { .. }) {
            return;
        }
        let n = self.videos.len();
        let cur = self.selected.unwrap_or(0);
        let cols = self.columns.max(1);
        let next = if right {
            Some((cur + 1).min(n - 1))
        } else if left {
            Some(cur.saturating_sub(1))
        } else if down {
            Some(if cur + cols < n { cur + cols } else { cur })
        } else if up {
            Some(cur.checked_sub(cols).unwrap_or(cur))
        } else {
            None
        };
        if let Some(next) = next {
            self.selected = Some(next);
            self.scroll_to_selected = true;
        }
        if enter {
            self.play_index(cur, ctx);
        }
    }

    fn top_bar(&mut self, ui: &mut Ui) -> bool {
        let full = ui.max_rect();
        let painter = ui.painter().clone();

        let mark = Rect::from_min_size(pos2(full.left(), full.center().y - 15.0), vec2(30.0, 30.0));
        paint_logo_mark(&painter, mark);
        let word = painter.text(
            pos2(mark.right() + 10.0, full.center().y),
            Align2::LEFT_CENTER,
            "fast",
            FontId::new(22.0, bold()),
            TEXT,
        );
        painter.text(word.right_center(), Align2::LEFT_CENTER, ".yt", FontId::new(22.0, bold()), ACCENT);

        let width = (full.width() - 2.0 * 160.0).clamp(240.0, 680.0);
        let left = (full.center().x - width / 2.0).max(full.left() + 140.0);
        let width = width.min(full.right() - left);
        let bar = Rect::from_min_size(pos2(left, full.center().y - 22.0), vec2(width, 44.0));

        let focused = ui.memory(|m| m.has_focus(search_id()));
        let glow = ui.ctx().animate_bool_with_time(Id::new("search-glow"), focused, 0.15);
        if glow > 0.0 {
            painter.add(
                Shadow { offset: [0, 0], blur: 22, spread: 0, color: ACCENT.gamma_multiply(0.22 * glow) }
                    .as_shape(bar, CornerRadius::same(22)),
            );
        }
        painter.rect(
            bar,
            CornerRadius::same(22),
            lerp_color(SURFACE, SURFACE_HI, glow),
            Stroke::new(1.0, lerp_color(SURFACE_HI, ACCENT.gamma_multiply(0.8), glow)),
            StrokeKind::Inside,
        );
        paint_search_icon(&painter, pos2(bar.left() + 24.0, bar.center().y), lerp_color(MUTED, TEXT, glow));

        let hint_w = if focused { 0.0 } else { 34.0 };
        if !focused {
            let key = Rect::from_center_size(pos2(bar.right() - 24.0, bar.center().y), vec2(22.0, 22.0));
            painter.rect_stroke(key, CornerRadius::same(6), Stroke::new(1.0, FAINT), StrokeKind::Inside);
            painter.text(key.center(), Align2::CENTER_CENTER, "/", FontId::proportional(13.0), MUTED);
        }

        let field =
            Rect::from_min_max(pos2(bar.left() + 44.0, bar.top()), pos2(bar.right() - 16.0 - hint_w, bar.bottom()));
        let mut submitted = false;
        ui.scope_builder(UiBuilder::new().max_rect(field), |ui| {
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                let edit = egui::TextEdit::singleline(&mut self.query)
                    .id(search_id())
                    .frame(Frame::NONE)
                    .hint_text(egui::RichText::new("Search music").color(FAINT))
                    .font(FontId::proportional(16.5))
                    .text_color(TEXT)
                    .desired_width(f32::INFINITY);
                let resp = ui.add(edit);
                if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    submitted = true;
                }
                if self.focus_search {
                    resp.request_focus();
                    self.focus_search = false;
                }
            });
        });

        if let Phase::Searching { .. } = self.phase {
            paint_sweep(ui, bar.shrink2(vec2(22.0, 0.0)), bar.bottom() - 1.0);
        }
        submitted
    }

    fn body(&mut self, ui: &mut Ui) {
        match &self.phase {
            Phase::Idle => self.hero(ui),
            Phase::Searching { .. } => skeleton_grid(ui),
            Phase::Failed(e) => {
                let e = e.clone();
                centered_message(ui, "Something went sideways", &e);
            }
            Phase::Results { took } => {
                if self.videos.is_empty() {
                    let q = self.shown_query.clone();
                    centered_message(ui, "No results", &format!("Nothing came back for “{q}”."));
                } else {
                    let took = *took;
                    self.results(ui, took);
                }
            }
        }
    }

    fn hero(&self, ui: &mut Ui) {
        let rect = ui.max_rect();
        let painter = ui.painter();
        let c = pos2(rect.center().x, rect.top() + rect.height() * 0.40);

        paint_logo_mark(painter, Rect::from_center_size(pos2(c.x, c.y - 70.0), vec2(64.0, 64.0)));
        painter.text(c, Align2::CENTER_CENTER, "What are we listening to?", FontId::new(34.0, bold()), TEXT);
        painter.text(
            pos2(c.x, c.y + 38.0),
            Align2::CENTER_CENTER,
            "No ads. No feed. Just search and play.",
            FontId::proportional(16.0),
            MUTED,
        );

        let keys: [(&str, &str); 4] = [("/", "search"), ("Enter", "play"), ("Space", "pause"), ("← ↑ → ↓", "move")];
        let font = FontId::proportional(13.0);
        let items: Vec<_> = keys
            .iter()
            .map(|(k, label)| {
                let kg = painter.layout_no_wrap(k.to_string(), FontId::new(12.5, semibold()), TEXT);
                let lg = painter.layout_no_wrap(label.to_string(), font.clone(), MUTED);
                (kg, lg)
            })
            .collect();
        let item_w = |(kg, lg): &(Arc<egui::Galley>, Arc<egui::Galley>)| kg.size().x + 16.0 + 8.0 + lg.size().x;
        let total: f32 = items.iter().map(item_w).sum::<f32>() + 28.0 * (items.len() - 1) as f32;
        let mut x = c.x - total / 2.0;
        let y = c.y + 92.0;
        for item in &items {
            let (kg, lg) = item;
            let key = Rect::from_min_size(pos2(x, y - 12.0), vec2(kg.size().x + 16.0, 24.0));
            painter.rect(key, CornerRadius::same(6), SURFACE, Stroke::new(1.0, SURFACE_HI), StrokeKind::Inside);
            painter.galley(key.center() - kg.size() / 2.0, kg.clone(), TEXT);
            painter.galley(pos2(key.right() + 8.0, y - lg.size().y / 2.0), lg.clone(), MUTED);
            x += item_w(item) + 28.0;
        }

        let mut missing = Vec::new();
        if self.tools.ytdlp.is_none() {
            missing.push("yt-dlp");
        }
        if self.tools.mpv.is_none() {
            missing.push("mpv");
        }
        if !missing.is_empty() {
            painter.text(
                pos2(c.x, y + 52.0),
                Align2::CENTER_CENTER,
                format!("Missing {}. Install with: {}", missing.join(" and "), yt::INSTALL_HINT),
                FontId::proportional(13.5),
                ACCENT_SOFT,
            );
        }
    }

    fn results(&mut self, ui: &mut Ui, took: Duration) {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(format!("“{}”", self.shown_query)).font(FontId::new(15.0, semibold())).color(TEXT),
            );
            ui.label(
                egui::RichText::new(format!("{} results  ·  {:.2}s", self.videos.len(), took.as_secs_f32()))
                    .size(13.5)
                    .color(MUTED),
            );
        });
        ui.add_space(14.0);

        let avail = ui.available_width() - 4.0;
        let cols = (((avail + GAP) / (MIN_CARD_W + GAP)).floor() as usize).max(1);
        self.columns = cols;
        let card_w = (avail - GAP * (cols - 1) as f32) / cols as f32;
        let card_h = card_w * 9.0 / 16.0 + 86.0;
        let playing_id = self.now_playing.as_ref().map(|v| v.id.clone());

        let mut clicked = None;
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            ui.add_space(6.0);
            for (row, chunk) in self.videos.chunks(cols).enumerate() {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = GAP;
                    for (col, video) in chunk.iter().enumerate() {
                        let i = row * cols + col;
                        let selected = self.selected == Some(i);
                        let playing = playing_id.as_deref() == Some(video.id.as_str());
                        let resp = video_card(ui, video, vec2(card_w, card_h), selected, playing);
                        if selected && self.scroll_to_selected {
                            ui.scroll_to_rect(resp.rect.expand(16.0), None);
                        }
                        if resp.hovered() && ui.input(|inp| inp.pointer.delta() != Vec2::ZERO) {
                            self.selected = Some(i);
                        }
                        if resp.clicked() {
                            clicked = Some(i);
                        }
                    }
                });
                ui.add_space(GAP - 4.0);
            }
            ui.add_space(24.0);
        });
        self.scroll_to_selected = false;
        if let Some(i) = clicked {
            self.play_index(i, &ui.ctx().clone());
        }
    }

    fn player_bar(&mut self, ui: &mut Ui) {
        let Some(video) = self.now_playing.clone() else { return };
        let ctx = ui.ctx().clone();
        let status = self.player.status();
        let full = ui.max_rect();
        let painter = ui.painter().clone();

        let thumb = Rect::from_min_size(pos2(full.left(), full.center().y - 30.0), vec2(107.0, 60.0));
        painter.rect_filled(thumb, CornerRadius::same(8), SURFACE_HI);
        if !video.id.is_empty() {
            egui::Image::new(video.thumbnail_url())
                .corner_radius(CornerRadius::same(8))
                .show_loading_spinner(false)
                .paint_at(ui, thumb);
        }

        let controls_w = (full.width() * 0.42).clamp(280.0, 560.0);
        let cx = full.center().x;
        let info_left = thumb.right() + 14.0;
        let info_w = (cx - controls_w / 2.0 - 24.0 - info_left).max(60.0);
        let playing = status.position.is_some() && !status.paused;
        let eq_w = if playing { 22.0 } else { 0.0 };
        let title = single_line(&painter, &video.title, FontId::new(14.5, semibold()), TEXT, info_w - eq_w);
        let title_h = title.size().y;
        painter.galley(pos2(info_left, full.center().y - title_h - 1.0), title, TEXT);
        if playing {
            paint_equalizer(
                &painter,
                pos2(info_left + info_w - 14.0, full.center().y - 10.0),
                ui.input(|i| i.time) as f32,
            );
            ctx.request_repaint_after(Duration::from_millis(50));
        }
        let mut sub = video.channel_name().to_owned();
        if self.queue.len() > 1 {
            let up_next = if self.queue_pos + 1 < self.queue.len() { "  ·  autoplay on" } else { "" };
            sub = format!("{sub}{up_next}");
        }
        let sub = single_line(&painter, &sub, FontId::proportional(13.0), MUTED, info_w);
        painter.galley(pos2(info_left, full.center().y + 4.0), sub, MUTED);

        let cy = full.center().y - 13.0;
        let play_rect = Rect::from_center_size(pos2(cx, cy), vec2(40.0, 40.0));
        let resp = icon_button(ui, play_rect, Id::new("play-pause"));
        let hover = resp.hovered() as u8 as f32;
        painter.circle_filled(play_rect.center(), 20.0 + 1.5 * hover, if hover > 0.0 { ACCENT_SOFT } else { ACCENT });
        if status.paused {
            paint_play(&painter, play_rect.center() + vec2(1.5, 0.0), 8.0, Color32::WHITE);
        } else {
            paint_pause(&painter, play_rect.center(), Color32::WHITE);
        }
        if resp.clicked() {
            self.player.send(Control::TogglePause);
        }

        let has_prev = self.queue_pos > 0 || status.position.unwrap_or(0.0) > RESTART_INSTEAD_OF_PREV;
        let has_next = self.queue_pos + 1 < self.queue.len();
        for (dx, dir, enabled) in [(-52.0, -1isize, has_prev), (52.0, 1, has_next)] {
            let r = Rect::from_center_size(pos2(cx + dx, cy), vec2(32.0, 32.0));
            let resp = icon_button(ui, r, Id::new(("skip", dir)));
            let color = if !enabled {
                FAINT
            } else if resp.hovered() {
                TEXT
            } else {
                MUTED
            };
            paint_skip(&painter, r.center(), dir > 0, color);
            if resp.clicked() && enabled {
                self.skip(dir, &ctx);
                return;
            }
        }

        let seek_y = full.center().y + 24.0;
        let time_w = 46.0;
        let seek = Rect::from_center_size(pos2(cx, seek_y), vec2(controls_w - 2.0 * time_w, 4.0));
        let duration = status.duration.filter(|d| *d > 0.0);
        let hit = ui.interact(seek.expand2(vec2(0.0, 9.0)), Id::new("seek"), Sense::click_and_drag());
        let pointer = hit.interact_pointer_pos().map(|p| ((p.x - seek.left()) / seek.width()).clamp(0.0, 1.0) as f64);
        if let Some(d) = duration {
            if hit.clicked() || hit.drag_stopped() {
                if let Some(f) = pointer.or(self.seek_drag) {
                    self.player.send(Control::SeekTo(f * d));
                }
                self.seek_drag = None;
            } else if hit.is_pointer_button_down_on() {
                self.seek_drag = pointer.or(self.seek_drag);
            }
        }
        let shown = self.seek_drag.zip(duration).map(|(f, d)| f * d).or(status.position);
        let active = hit.hovered() || self.seek_drag.is_some();
        let thick = if active { 6.0 } else { 4.0 };
        let track = Rect::from_center_size(seek.center(), vec2(seek.width(), thick));
        painter.rect_filled(track, CornerRadius::same(3), SURFACE_HI);
        match (shown, duration) {
            (None, _) => paint_sweep(ui, track, track.center().y),
            (Some(pos), Some(d)) => {
                let x = track.left() + track.width() * (pos / d).clamp(0.0, 1.0) as f32;
                painter.rect_filled(Rect::from_min_max(track.min, pos2(x, track.max.y)), CornerRadius::same(3), ACCENT);
                if active {
                    painter.circle_filled(pos2(x, track.center().y), 7.0, Color32::WHITE);
                    hit.clone().on_hover_cursor(CursorIcon::PointingHand);
                }
            }
            (Some(_), None) => {
                painter.rect_filled(track, CornerRadius::same(3), ACCENT.gamma_multiply(0.6));
            }
        }
        let small = FontId::proportional(12.0);
        let left_label = match shown {
            Some(_) if status.buffering => "Buffering".into(),
            Some(pos) => fmt_time(pos),
            None => "Loading".into(),
        };
        if status.buffering && shown.is_some() {
            paint_sweep(ui, track, track.center().y);
        }
        let right_label = match (duration, shown) {
            (Some(d), _) => fmt_time(d),
            (None, Some(_)) => "LIVE".into(),
            (None, None) => String::new(),
        };
        painter.text(pos2(seek.left() - 10.0, seek_y), Align2::RIGHT_CENTER, left_label, small.clone(), MUTED);
        painter.text(pos2(seek.right() + 10.0, seek_y), Align2::LEFT_CENTER, right_label, small, MUTED);

        let close = Rect::from_center_size(pos2(full.right() - 14.0, full.center().y), vec2(28.0, 28.0));
        let close_resp = icon_button(ui, close, Id::new("close")).on_hover_text("Stop");
        paint_close(&painter, close.center(), if close_resp.hovered() { TEXT } else { MUTED });

        let vol_w = (full.right() - 40.0 - (cx + controls_w / 2.0 + 60.0)).clamp(0.0, 110.0);
        if vol_w > 40.0 {
            let vol = Rect::from_min_size(pos2(close.left() - 18.0 - vol_w, full.center().y - 2.0), vec2(vol_w, 4.0));
            let speaker = Rect::from_center_size(pos2(vol.left() - 20.0, vol.center().y), vec2(26.0, 26.0));
            let speaker_resp = icon_button(ui, speaker, Id::new("mute"));
            let volume = status.volume.clamp(0.0, 100.0);
            paint_speaker(&painter, speaker.center(), volume, if speaker_resp.hovered() { TEXT } else { MUTED });
            if speaker_resp.clicked() {
                self.toggle_mute();
            }
            let vhit = ui.interact(vol.expand2(vec2(4.0, 9.0)), Id::new("volume"), Sense::click_and_drag());
            if (vhit.is_pointer_button_down_on() || vhit.clicked())
                && let Some(p) = vhit.interact_pointer_pos()
            {
                let v = (((p.x - vol.left()) / vol.width()).clamp(0.0, 1.0) * 100.0).round() as f64;
                if (v - volume).abs() >= 1.0 {
                    self.muted_from = None;
                    self.player.send(Control::SetVolume(v));
                }
            }
            let scroll = if vhit.hovered() { ui.input(|i| i.smooth_scroll_delta.y) } else { 0.0 };
            if scroll != 0.0 {
                self.muted_from = None;
                self.player.send(Control::SetVolume((volume + scroll as f64 * 0.1).clamp(0.0, 100.0)));
            }
            let vactive = vhit.hovered() || vhit.is_pointer_button_down_on();
            let vtrack = Rect::from_center_size(vol.center(), vec2(vol.width(), if vactive { 6.0 } else { 4.0 }));
            painter.rect_filled(vtrack, CornerRadius::same(3), SURFACE_HI);
            let vx = vtrack.left() + vtrack.width() * (volume / 100.0) as f32;
            painter.rect_filled(
                Rect::from_min_max(vtrack.min, pos2(vx, vtrack.max.y)),
                CornerRadius::same(3),
                if vactive { ACCENT } else { TEXT },
            );
            if vactive {
                painter.circle_filled(pos2(vx, vtrack.center().y), 6.0, Color32::WHITE);
            }
        }

        if close_resp.clicked() {
            self.player.stop();
            self.now_playing = None;
        }
    }

    fn paint_toast(&mut self, ctx: &egui::Context) {
        let Some((text, at)) = &self.toast else { return };
        let age = at.elapsed();
        if age > TOAST_FOR {
            self.toast = None;
            return;
        }
        let secs = age.as_secs_f32();
        let fade_in = (secs / 0.18).min(1.0);
        let fade_out = ((TOAST_FOR.as_secs_f32() - secs) / 0.4).min(1.0);
        let a = ease_out(fade_in.min(fade_out));

        let painter = ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("toast")));
        let screen = ctx.content_rect();
        let mut job = LayoutJob::simple_singleline(text.clone(), FontId::new(14.0, semibold()), TEXT.gamma_multiply(a));
        job.wrap.max_width = (screen.width() - 80.0).min(620.0);
        job.wrap.max_rows = 1;
        job.wrap.break_anywhere = true;
        let galley = painter.layout_job(job);
        let size = galley.size() + vec2(36.0, 22.0);
        let bottom = if self.now_playing.is_some() { PLAYER_BAR_H } else { 0.0 };
        let center = pos2(screen.center().x, screen.bottom() - bottom - 44.0 + 12.0 * (1.0 - a));
        let rect = Rect::from_center_size(center, size);
        painter.add(
            Shadow { offset: [0, 10], blur: 30, spread: 0, color: Color32::from_black_alpha((140.0 * a) as u8) }
                .as_shape(rect, CornerRadius::same(22)),
        );
        painter.rect(
            rect,
            CornerRadius::same(22),
            SURFACE_HI.gamma_multiply(a),
            Stroke::new(1.0, ACCENT.gamma_multiply(0.5 * a)),
            StrokeKind::Inside,
        );
        painter.galley(rect.center() - galley.size() / 2.0, galley, TEXT);
        ctx.request_repaint();
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll(ctx);
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let search_focused = ctx.memory(|m| m.has_focus(search_id()));
        self.handle_keys(&ctx, search_focused);

        let mut submitted = false;
        egui::Panel::top("top")
            .exact_size(76.0)
            .frame(Frame::new().fill(BG).inner_margin(Margin::symmetric(28, 0)))
            .show(ui, |ui| submitted = self.top_bar(ui));
        if submitted {
            self.submit(&ctx);
        }

        if self.now_playing.is_some() {
            egui::Panel::bottom("player")
                .exact_size(PLAYER_BAR_H)
                .frame(Frame::new().fill(PLAYER_BG).inner_margin(Margin::symmetric(22, 0)))
                .show(ui, |ui| self.player_bar(ui));
        }

        egui::CentralPanel::default()
            .frame(Frame::new().fill(BG).inner_margin(Margin { left: 28, right: 24, top: 6, bottom: 0 }))
            .show(ui, |ui| self.body(ui));

        self.paint_toast(&ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.player.stop();
    }
}

fn search_id() -> Id {
    Id::new("search")
}

fn icon_button(ui: &mut Ui, rect: Rect, id: Id) -> Response {
    ui.interact(rect, id, Sense::click()).on_hover_cursor(CursorIcon::PointingHand)
}

fn video_card(ui: &mut Ui, video: &Video, size: Vec2, selected: bool, playing: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let hot = resp.hovered() || selected;
    let t = ease_out(ui.ctx().animate_bool_with_time(resp.id.with("hot"), hot, 0.14));
    let painter = ui.painter();

    let lift = -3.0 * t;
    let back = rect.expand(8.0).translate(vec2(0.0, lift));
    if t > 0.0 {
        painter.add(
            Shadow { offset: [0, 14], blur: 36, spread: 0, color: Color32::from_black_alpha((150.0 * t) as u8) }
                .as_shape(back, CornerRadius::same(THUMB_RADIUS + 4)),
        );
        painter.rect_filled(back, CornerRadius::same(THUMB_RADIUS + 4), SURFACE.gamma_multiply(t));
    }

    let thumb = Rect::from_min_size(rect.min + vec2(0.0, lift), vec2(rect.width(), rect.width() * 9.0 / 16.0));
    painter.rect_filled(thumb, CornerRadius::same(THUMB_RADIUS), SURFACE_HI);
    egui::Image::new(video.thumbnail_url())
        .corner_radius(CornerRadius::same(THUMB_RADIUS))
        .show_loading_spinner(false)
        .paint_at(ui, thumb);

    if t > 0.0 {
        painter.rect_filled(thumb, CornerRadius::same(THUMB_RADIUS), Color32::from_black_alpha((70.0 * t) as u8));
        let r = 18.0 + 8.0 * t;
        let c = thumb.center();
        painter.circle_filled(c, r, ACCENT.gamma_multiply(0.92 * t));
        paint_play(painter, c + vec2(r * 0.08, 0.0), r * 0.42, Color32::WHITE.gamma_multiply(t));
    }
    if selected {
        painter.rect_stroke(
            thumb.expand(1.0),
            CornerRadius::same(THUMB_RADIUS + 1),
            Stroke::new(2.0, ACCENT),
            StrokeKind::Outside,
        );
    }
    if playing {
        let g = painter.layout_no_wrap("NOW PLAYING".into(), FontId::new(11.0, bold()), Color32::WHITE);
        let pill = Rect::from_min_size(thumb.min + vec2(10.0, 10.0), g.size() + vec2(32.0, 8.0));
        painter.rect_filled(pill, CornerRadius::same(7), ACCENT);
        paint_equalizer(painter, pos2(pill.left() + 9.0, pill.center().y - 5.0), ui.input(|i| i.time) as f32);
        painter.galley(pos2(pill.left() + 24.0, pill.center().y - g.size().y / 2.0), g, Color32::WHITE);
    }

    if let Some(label) = video.duration_label() {
        let live = video.is_live();
        let g = painter.layout_no_wrap(label, FontId::new(12.0, semibold()), Color32::WHITE);
        let badge =
            Rect::from_min_size(thumb.max - g.size() - vec2(10.0 + 14.0, 10.0 + 6.0), g.size() + vec2(14.0, 6.0));
        let fill = if live { ACCENT } else { Color32::from_black_alpha(200) };
        painter.rect_filled(badge, CornerRadius::same(6), fill);
        painter.galley(badge.center() - g.size() / 2.0, g, Color32::WHITE);
    }

    let text_left = pos2(thumb.left() + 2.0, thumb.bottom() + 12.0);
    let mut job = LayoutJob::simple(video.title.clone(), FontId::new(15.5, semibold()), TEXT, rect.width() - 4.0);
    job.wrap.max_rows = 2;
    let title = painter.layout_job(job);
    let title_h = title.size().y;
    painter.galley(text_left, title, TEXT);

    let meta = single_line(painter, &video.meta_line(), FontId::proportional(13.5), MUTED, rect.width() - 4.0);
    painter.galley(text_left + vec2(0.0, title_h + 5.0), meta, MUTED);

    resp
}

fn single_line(painter: &egui::Painter, text: &str, font: FontId, color: Color32, width: f32) -> Arc<egui::Galley> {
    let mut job = LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap.max_width = width.max(1.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    painter.layout_job(job)
}

fn skeleton_grid(ui: &mut Ui) {
    ui.add_space(34.0);
    let avail = ui.available_width() - 4.0;
    let cols = (((avail + GAP) / (MIN_CARD_W + GAP)).floor() as usize).max(1);
    let card_w = (avail - GAP * (cols - 1) as f32) / cols as f32;
    let thumb_h = card_w * 9.0 / 16.0;
    let time = ui.input(|i| i.time) as f32;
    let top = ui.cursor().top();
    let painter = ui.painter();
    for i in 0..cols * 3 {
        let (row, col) = (i / cols, i % cols);
        let x = ui.max_rect().left() + col as f32 * (card_w + GAP);
        let y = top + row as f32 * (thumb_h + 86.0 + GAP - 4.0);
        let pulse = 0.5 + 0.5 * (time * 3.2 - i as f32 * 0.35).sin();
        let fill = lerp_color(SURFACE, SURFACE_HI, pulse);
        painter.rect_filled(
            Rect::from_min_size(pos2(x, y), vec2(card_w, thumb_h)),
            CornerRadius::same(THUMB_RADIUS),
            fill,
        );
        painter.rect_filled(
            Rect::from_min_size(pos2(x, y + thumb_h + 14.0), vec2(card_w * 0.86, 13.0)),
            CornerRadius::same(6),
            fill,
        );
        painter.rect_filled(
            Rect::from_min_size(pos2(x, y + thumb_h + 36.0), vec2(card_w * 0.52, 11.0)),
            CornerRadius::same(6),
            fill,
        );
    }
    ui.ctx().request_repaint();
}

fn centered_message(ui: &mut Ui, title: &str, detail: &str) {
    let rect = ui.max_rect();
    let c = pos2(rect.center().x, rect.top() + rect.height() * 0.38);
    let painter = ui.painter();
    painter.text(c, Align2::CENTER_CENTER, title, FontId::new(24.0, bold()), TEXT);
    let mut job =
        LayoutJob::simple(detail.to_owned(), FontId::proportional(14.5), MUTED, (rect.width() - 80.0).min(640.0));
    job.halign = egui::Align::Center;
    let g = painter.layout_job(job);
    painter.galley(pos2(c.x, c.y + 26.0), g, MUTED);
}

fn paint_sweep(ui: &Ui, span: Rect, y: f32) {
    let t = ui.input(|i| i.time) as f32;
    let w = span.width() * 0.28;
    let x = span.left() + (span.width() - w) * (0.5 + 0.5 * (t * 2.4).sin());
    ui.painter().line_segment([pos2(x, y), pos2(x + w, y)], Stroke::new(2.0, ACCENT));
    ui.ctx().request_repaint();
}

fn paint_logo_mark(painter: &egui::Painter, rect: Rect) {
    let r = (rect.width() * 0.28) as u8;
    painter.add(
        Shadow { offset: [0, 4], blur: 18, spread: 0, color: ACCENT.gamma_multiply(0.35) }
            .as_shape(rect, CornerRadius::same(r)),
    );
    painter.rect_filled(rect, CornerRadius::same(r), ACCENT);
    paint_play(painter, rect.center() + vec2(rect.width() * 0.03, 0.0), rect.width() * 0.2, Color32::WHITE);
}

fn paint_play(painter: &egui::Painter, c: Pos2, s: f32, color: Color32) {
    painter.add(egui::Shape::convex_polygon(
        vec![pos2(c.x - s * 0.8, c.y - s), pos2(c.x + s, c.y), pos2(c.x - s * 0.8, c.y + s)],
        color,
        Stroke::NONE,
    ));
}

fn paint_pause(painter: &egui::Painter, c: Pos2, color: Color32) {
    for dx in [-4.5, 4.5] {
        painter.rect_filled(Rect::from_center_size(c + vec2(dx, 0.0), vec2(4.5, 15.0)), CornerRadius::same(1), color);
    }
}

fn paint_skip(painter: &egui::Painter, c: Pos2, forward: bool, color: Color32) {
    let d = if forward { 1.0 } else { -1.0 };
    painter.add(egui::Shape::convex_polygon(
        vec![pos2(c.x - 6.0 * d, c.y - 7.0), pos2(c.x + 5.0 * d, c.y), pos2(c.x - 6.0 * d, c.y + 7.0)],
        color,
        Stroke::NONE,
    ));
    painter.rect_filled(
        Rect::from_center_size(pos2(c.x + 7.0 * d, c.y), vec2(2.5, 14.0)),
        CornerRadius::same(1),
        color,
    );
}

fn paint_speaker(painter: &egui::Painter, c: Pos2, volume: f64, color: Color32) {
    painter.add(egui::Shape::convex_polygon(
        vec![
            c + vec2(-8.0, -3.0),
            c + vec2(-4.5, -3.0),
            c + vec2(0.5, -7.5),
            c + vec2(0.5, 7.5),
            c + vec2(-4.5, 3.0),
            c + vec2(-8.0, 3.0),
        ],
        color,
        Stroke::NONE,
    ));
    let stroke = Stroke::new(1.6, color);
    if volume <= 0.0 {
        painter.line_segment([c + vec2(3.5, -3.5), c + vec2(9.5, 3.5)], stroke);
        painter.line_segment([c + vec2(3.5, 3.5), c + vec2(9.5, -3.5)], stroke);
        return;
    }
    let waves = if volume > 50.0 { 2 } else { 1 };
    for w in 0..waves {
        let r = 5.0 + 4.0 * w as f32;
        let pts: Vec<Pos2> = (0..=8)
            .map(|i| {
                let a = -0.9 + 1.8 * i as f32 / 8.0;
                c + vec2(1.5 + r * a.cos(), r * a.sin())
            })
            .collect();
        painter.add(egui::Shape::line(pts, stroke));
    }
}

fn paint_close(painter: &egui::Painter, c: Pos2, color: Color32) {
    let s = Stroke::new(1.8, color);
    painter.line_segment([c + vec2(-5.0, -5.0), c + vec2(5.0, 5.0)], s);
    painter.line_segment([c + vec2(-5.0, 5.0), c + vec2(5.0, -5.0)], s);
}

fn paint_equalizer(painter: &egui::Painter, origin: Pos2, time: f32) {
    for i in 0..3 {
        let phase = time * (5.0 + i as f32 * 1.7) + i as f32 * 1.3;
        let h = 3.0 + 7.0 * (0.5 + 0.5 * phase.sin());
        let x = origin.x + i as f32 * 4.0;
        painter.rect_filled(
            Rect::from_min_max(pos2(x, origin.y + 10.0 - h), pos2(x + 2.5, origin.y + 10.0)),
            CornerRadius::same(1),
            Color32::WHITE,
        );
    }
}

fn paint_search_icon(painter: &egui::Painter, c: Pos2, color: Color32) {
    let stroke = Stroke::new(1.8, color);
    let lens = c + vec2(-1.5, -1.5);
    painter.circle_stroke(lens, 6.0, stroke);
    painter.line_segment([lens + vec2(4.3, 4.3), lens + vec2(8.5, 8.5)], stroke);
}

fn fmt_time(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    let (h, m, s) = (s / 3600, s / 60 % 60, s % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_unmultiplied(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()), l(a.a(), b.a()))
}

fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

fn print_diagnostics(target: Option<&str>) {
    attach_parent_console();
    println!("Running fast.yt diagnostics (takes about a minute; nothing will play out loud)...");
    let report = diagnose::run(target);
    println!("{report}");
    let saved = std::env::temp_dir().join("fastyt-diagnostics.txt");
    if std::fs::write(&saved, &report).is_ok() {
        println!("Saved to {}", saved.display());
    }
}

/// Release builds on Windows are GUI apps with no console; attach to the terminal that launched
/// `fastyt --diagnose` so the report is visible there.
#[cfg(windows)]
fn attach_parent_console() {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn AttachConsole(process_id: u32) -> i32;
    }
    const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
    // SAFETY: AttachConsole takes no pointers; failure (e.g. no parent console) is harmless.
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

#[cfg(not(windows))]
fn attach_parent_console() {}

#[cfg(test)]
mod tests {
    use super::fmt_time;

    #[test]
    fn formats_times() {
        assert_eq!(fmt_time(0.0), "0:00");
        assert_eq!(fmt_time(65.9), "1:05");
        assert_eq!(fmt_time(3725.0), "1:02:05");
        assert_eq!(fmt_time(-3.0), "0:00");
    }
}
