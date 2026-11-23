//! The Amethys Launcher pages, drawn with GhostEngine primitives only.
//!
//! Every page adapts to the real window width through [`Layout`]. All text
//! shown here comes from Amethys Core, the local filesystem or the discovery
//! service: nothing is invented. Features that do not exist yet are shown as
//! unavailable instead of being simulated.

use crate::gfx::*;
use crate::status::{GameStatus, StatusKind};
use amethys_ipc::IpcGameEntry;
use ghostengine::Color;

pub const SIDE_W: i32 = 240;
const GAP: i32 = 18;
const BTN_H: i32 = 40;
const MIN_CARD_W: i32 = 188;
const FOOTER_H: i32 = 58;
/// How many installations discovered by a scan are listed at once.
const DETECTED_LIMIT: usize = 24;

// ------------------------------------------------------------------ layout
/// Geometry of the current window: sidebar on the left, one content column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    pub w: i32,
    pub h: i32,
    /// Left edge of the content column.
    pub mx: i32,
    /// Width of the content column.
    pub mw: i32,
    pub side_w: i32,
}

impl Layout {
    pub fn new(w: i32, h: i32) -> Self {
        // Keep a consistent gutter while allowing the content area to shrink
        // with the actual client width instead of overflowing at compact sizes.
        let side_w = if w < 900 { 72 } else { SIDE_W };
        let gutter = if w < 520 { 12 } else { 20 };
        let mx = side_w + gutter;
        let mw = (w - mx - gutter).max(160);
        Self { w, h, mx, mw, side_w }
    }

    /// Narrow windows stack toolbars and columns instead of placing them side by side.
    pub fn compact(&self) -> bool {
        self.mw < 760
    }

    /// Number of game-card columns for the current width.
    pub fn columns(&self) -> i32 {
        if self.mw < MIN_CARD_W * 2 + GAP { 1 } else { ((self.mw + GAP) / (MIN_CARD_W + GAP)).clamp(1, 8) }
    }

    pub fn card_width(&self) -> i32 {
        let cols = self.columns();
        (self.mw - GAP * (cols - 1)) / cols
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum Active {
    Store,
    Library,
    Settings,
}

pub struct SidebarInfo {
    pub core_ok: bool,
    pub game_count: usize,
}

// ------------------------------------------------------------- small helpers
fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n > 1 { many } else { one })
}

fn tone(kind: StatusKind) -> (Color, Color) {
    match kind {
        StatusKind::Ready => (GREEN, GREEN_DARK),
        StatusKind::NeedsExecutable | StatusKind::InvalidExecutable => (AMBER, AMBER_DARK),
        StatusKind::FolderMissing => (RED, RED_DARK),
    }
}

/// Keeps the end of `s` visible (used for text being typed).
fn tail_fit(p: &mut Painter, s: &str, size: f32, w: W, max_w: i32) -> String {
    if p.width(s, size, w) <= max_w as f32 {
        return s.to_owned();
    }
    let chars: Vec<char> = s.chars().collect();
    let mut start = 0;
    while start < chars.len() {
        let candidate = format!("…{}", chars[start..].iter().collect::<String>());
        if p.width(&candidate, size, w) <= max_w as f32 {
            return candidate;
        }
        start += 1;
    }
    "…".to_owned()
}

/// Soft violet light at the top of every page.
fn backdrop(p: &mut Painter, lay: &Layout) {
    let (x, w) = (lay.side_w, lay.w - lay.side_w);
    p.glow(x + w / 3, 0, 640, Color::rgba(122, 92, 232, 40), x, 0, w, 460);
    p.glow(x + w - 120, 60, 420, Color::rgba(70, 120, 235, 16), x, 0, w, 420);
}

fn panel(p: &mut Painter, x: i32, y: i32, w: i32, h: i32, r: i32) {
    p.shadow(x, y, w, h, r, 10, 34);
    p.rrect(x, y, w, h, r, PANEL);
    p.rrect_line(x, y, w, h, r, alpha(Color::WHITE, 15));
}

// ---------------------------------------------------------------- components
#[derive(Clone, Copy, PartialEq)]
enum Btn {
    Primary,
    Secondary,
    Ghost,
    Danger,
}

fn button_width(p: &mut Painter, label: &str, icon: Option<Icon>) -> i32 {
    p.width(label, 13.5, W::Semi).round() as i32 + 36 + if icon.is_some() { 24 } else { 0 }
}

/// Draws a button and, when `action` is given, registers its hit region.
/// Returns the button width.
#[allow(clippy::too_many_arguments)]
fn button(p: &mut Painter, x: i32, y: i32, w: Option<i32>, label: &str, icon: Option<Icon>, kind: Btn, bg: Color, action: Option<Action>) -> i32 {
    let bw = w.unwrap_or_else(|| button_width(p, label, icon));
    let (ink, icon_bg, hover) = match kind {
        Btn::Primary => {
            p.gradient_rrect(x, y, bw, BTN_H, 12, 180.0, &[(0.0, VIO_HI), (1.0, VIO)], bg);
            (VIO_DARK, VIO, Hover::Glow)
        }
        Btn::Secondary => {
            p.rrect(x, y, bw, BTN_H, 12, PANEL_HI);
            p.rrect_line(x, y, bw, BTN_H, 12, alpha(Color::WHITE, 28));
            (TXT, PANEL_HI, Hover::Glow)
        }
        Btn::Ghost => {
            p.rrect_line(x, y, bw, BTN_H, 12, alpha(Color::WHITE, 30));
            (SOFT, bg, Hover::Soft)
        }
        Btn::Danger => {
            p.rrect(x, y, bw, BTN_H, 12, RED_DARK);
            p.rrect_line(x, y, bw, BTN_H, 12, alpha(RED, 110));
            (RED, RED_DARK, Hover::Danger)
        }
    };
    let tw = p.width(label, 13.5, W::Semi).round() as i32;
    let content = tw + if icon.is_some() { 24 } else { 0 };
    let mut cx = x + (bw - content) / 2;
    if let Some(ic) = icon {
        p.icon(ic, cx, y + (BTN_H - 16) / 2, 16, ink, icon_bg);
        cx += 24;
    }
    p.text(cx, y, BTN_H, label, 13.5, W::Semi, ink);
    if let Some(a) = action {
        p.hit_with(x, y, bw, BTN_H, 12, a, false, hover);
    }
    bw
}

/// A button that cannot be used right now (no hit region).
fn button_disabled(p: &mut Painter, x: i32, y: i32, label: &str, icon: Option<Icon>) -> i32 {
    let bw = button_width(p, label, icon);
    p.rrect(x, y, bw, BTN_H, 12, PANEL_HI);
    let tw = p.width(label, 13.5, W::Semi).round() as i32;
    let content = tw + if icon.is_some() { 24 } else { 0 };
    let mut cx = x + (bw - content) / 2;
    if let Some(ic) = icon {
        p.icon(ic, cx, y + (BTN_H - 16) / 2, 16, FAINT, PANEL_HI);
        cx += 24;
    }
    p.text(cx, y, BTN_H, label, 13.5, W::Semi, FAINT);
    bw
}

/// Filter pill with an optional count. Returns its width.
fn pill(p: &mut Painter, x: i32, y: i32, label: &str, count: Option<usize>, on: bool, action: Action) -> i32 {
    let wt = if on { W::Semi } else { W::Med };
    let tw = p.width(label, 13.0, wt).round() as i32;
    let count_text = count.map(|c| c.to_string());
    let cw = count_text.as_deref().map_or(0, |t| p.width(t, 11.5, W::Semi).round() as i32 + 8);
    let w = 32 + tw + cw;
    if on {
        p.rrect(x, y, w, 34, 17, ACT);
        p.rrect_line(x, y, w, 34, 17, alpha(VIO, 150));
    } else {
        p.rrect(x, y, w, 34, 17, PANEL);
        p.rrect_line(x, y, w, 34, 17, alpha(Color::WHITE, 22));
    }
    p.text(x + 16, y, 34, label, 13.0, wt, if on { VIO_HI } else { SOFT });
    if let Some(t) = count_text.as_deref() {
        p.text(x + 16 + tw + 8, y, 34, t, 11.5, W::Semi, if on { alpha(VIO_HI, 190) } else { FAINT });
    }
    p.hit(x, y, w, 34, 17, action, false);
    w
}

/// Dark translucent label drawn on top of artwork.
fn chip(p: &mut Painter, x: i32, y: i32, label: &str, max_w: i32) {
    let text = p.ellipsize(label, 11.0, W::Semi, max_w - 18);
    let w = p.width(&text, 11.0, W::Semi).round() as i32 + 18;
    p.rrect(x, y, w, 22, 11, Color::rgba(8, 6, 14, 175));
    p.text(x + 9, y, 22, &text, 11.0, W::Semi, Color::WHITE);
}

/// Tinted status pill with a dot. Returns its width.
fn status_badge(p: &mut Painter, x: i32, y: i32, kind: StatusKind) -> i32 {
    let (fg, bg) = tone(kind);
    let label = kind.label();
    let w = p.width(label, 12.0, W::Semi).round() as i32 + 34;
    p.rrect(x, y, w, 26, 13, bg);
    p.rrect_line(x, y, w, 26, 13, alpha(fg, 70));
    p.circle(x + 14, y + 13, 3, fg);
    p.text(x + 24, y, 26, label, 12.0, W::Semi, fg);
    w
}

fn icon_badge(p: &mut Painter, x: i32, y: i32, size: i32, ic: Icon, fg: Color, bg: Color) {
    p.rrect(x, y, size, size, size / 3, bg);
    p.icon(ic, x + size / 4, y + size / 4, size / 2, fg, bg);
}

/// Big empty state with an optional call to action. Returns the height used.
#[allow(clippy::too_many_arguments)]
fn empty_state(p: &mut Painter, x: i32, y: i32, w: i32, ic: Icon, title: &str, description: &str, cta: Option<(&str, Action, Btn)>) -> i32 {
    let h = if cta.is_some() { 286 } else { 226 };
    panel(p, x, y, w, h, 20);
    p.glow(x + w / 2, y + 40, 260, Color::rgba(122, 92, 232, 40), x + 1, y + 1, w - 2, h / 2);
    p.round_corners(x, y, w, h, 20, BG);
    let cx = x + w / 2;
    p.gradient_rrect(cx - 30, y + 32, 60, 60, 20, 135.0, &[(0.0, ACT), (1.0, hex("#1B1432"))], PANEL);
    p.rrect_line(cx - 30, y + 32, 60, 60, 20, alpha(VIO, 90));
    p.icon(ic, cx - 14, y + 48, 28, VIO_HI, ACT);
    p.text_center(x, w, y + 106, 30, title, 20.0, W::Disp, TXT);
    let dh = p.paragraph_height(w.min(560) - 40, 21, description, 13.5, W::Reg, 3);
    let lines = p.wrap(description, 13.5, W::Reg, w.min(560) - 40);
    for (i, line) in lines.iter().take(3).enumerate() {
        p.text_center(x, w, y + 142 + i as i32 * 21, 21, line, 13.5, W::Reg, MUT);
    }
    if let Some((label, action, kind)) = cta {
        let bw = button_width(p, label, None) + 16;
        button(p, cx - bw / 2, y + 142 + dh + 18, Some(bw), label, None, kind, PANEL, Some(action));
    }
    h
}

// ------------------------------------------------------------------ sidebar
#[allow(clippy::too_many_arguments)]
fn nav_item(p: &mut Painter, y: i32, label: &str, ic: Icon, active: bool, badge: Option<(&str, bool)>, page: Page) {
    let (x, w, h) = (14, SIDE_W - 28, 44);
    if active {
        p.rrect(x, y, w, h, 12, ACT);
        p.rrect(x, y + 12, 3, 20, 2, VIO);
    }
    let bg = if active { ACT } else { SIDE };
    p.icon(ic, x + 16, y + 12, 20, if active { VIO_HI } else { MUT }, bg);
    p.text(x + 48, y, h, label, 14.0, if active { W::Semi } else { W::Med }, if active { TXT } else { MUT });
    if let Some((text, strong)) = badge {
        let tw = p.width(text, 11.0, W::Semi).round() as i32;
        let bw = tw + 16;
        let bx = x + w - 12 - bw;
        if strong {
            p.rrect(bx, y + 12, bw, 20, 10, alpha(Color::WHITE, 16));
            p.text(bx + 8, y + 12, 20, text, 11.0, W::Semi, SOFT);
        } else {
            p.rrect_line(bx, y + 12, bw, 20, 10, alpha(Color::WHITE, 36));
            p.text(bx + 8, y + 12, 20, text, 11.0, W::Semi, FAINT);
        }
    }
    p.hit_with(x, y, w, h, 12, Action::Go(page), true, Hover::Soft);
}

pub fn sidebar(p: &mut Painter, lay: &Layout, active: Active, info: &SidebarInfo) {
    let vh = lay.h;
    let sw = lay.side_w;
    p.fill(0, 0, sw, vh, SIDE);
    p.fill(sw - 1, 0, 1, vh, alpha(Color::WHITE, 14));
    p.glow(0, 0, 260, Color::rgba(122, 92, 232, 38), 0, 0, sw - 1, 220);

    // Compact windows use an icon rail so navigation remains accessible.
    if sw < 100 {
        p.icon(Icon::Gem, (sw - 30) / 2, 22, 30, VIO, SIDE);
        let entries = [
            (88, Icon::Library, Active::Library, Page::Library),
            (136, Icon::Gem, Active::Store, Page::Store),
            (vh - 56, Icon::Gear, Active::Settings, Page::Settings),
        ];
        for (y, icon, item, page) in entries {
            let x = 8;
            let w = sw - 16;
            if active == item {
                p.rrect(x, y, w, 40, 12, ACT);
                p.rrect(x, y + 10, 3, 20, 2, VIO);
            }
            let bg = if active == item { ACT } else { SIDE };
            p.icon(icon, (sw - 20) / 2, y + 10, 20, if active == item { VIO_HI } else { MUT }, bg);
            p.hit_with(x, y, w, 40, 12, Action::Go(page), true, Hover::Soft);
        }
        p.circle(sw / 2, vh - 24, 4, if info.core_ok { GREEN } else { AMBER });
        return;
    }

    p.icon(Icon::Gem, 24, 24, 30, VIO, SIDE);
    p.text(64, 24, 30, "Amethys", 21.0, W::Disp, TXT);
    p.text(28, 84, 16, "NAVIGATION", 10.5, W::Semi, FAINT);
    let count = info.game_count.to_string();
    nav_item(p, 106, "Bibliothèque", Icon::Library, active == Active::Library, Some((&count, true)), Page::Library);
    nav_item(p, 156, "Boutique", Icon::Gem, active == Active::Store, Some(("Bientôt", false)), Page::Store);
    nav_item(p, vh - 144, "Paramètres", Icon::Gear, active == Active::Settings, None, Page::Settings);

    let (fx, fy, fw, fh) = (14, vh - 84, sw - 28, 62);
    p.rrect(fx, fy, fw, fh, 14, PANEL);
    p.rrect_line(fx, fy, fw, fh, 14, alpha(Color::WHITE, 15));
    let (dot, title, sub) = if info.core_ok {
        (GREEN, "Core connecté", format!("{} en local", plural(info.game_count, "jeu", "jeux")))
    } else {
        (AMBER, "Core hors ligne", "Connexion impossible".to_owned())
    };
    p.circle(fx + 20, fy + 22, 5, alpha(dot, 70));
    p.circle(fx + 20, fy + 22, 3, dot);
    p.text(fx + 34, fy + 11, 22, title, 13.0, W::Semi, TXT);
    p.text(fx + 34, fy + 33, 18, &sub, 12.0, W::Reg, MUT);
}

// ---------------------------------------------------------------- toolbar bits
fn search_field(p: &mut Painter, x: i32, y: i32, w: i32, query: &str, focused: bool) {
    p.rrect(x, y, w, BTN_H, 12, PANEL_HI);
    if focused {
        p.rrect_line(x, y, w, BTN_H, 12, VIO);
        p.rrect_line(x + 1, y + 1, w - 2, BTN_H - 2, 11, alpha(VIO, 120));
    } else {
        p.rrect_line(x, y, w, BTN_H, 12, alpha(Color::WHITE, 24));
    }
    p.icon(Icon::Search, x + 14, y + 12, 16, if focused { VIO_HI } else { MUT }, PANEL_HI);
    let reserve = if query.is_empty() { 0 } else { 34 };
    let max_w = w - 44 - 14 - reserve;
    if query.is_empty() {
        p.text(x + 42, y, BTN_H, "Rechercher un jeu…", 13.5, W::Reg, FAINT);
        if focused {
            p.rrect(x + 42, y + 11, 2, 18, 1, VIO_HI);
        }
    } else {
        let shown = tail_fit(p, query, 13.5, W::Reg, max_w);
        let tw = p.text(x + 42, y, BTN_H, &shown, 13.5, W::Reg, TXT).round() as i32;
        if focused {
            p.rrect(x + 43 + tw, y + 11, 2, 18, 1, VIO_HI);
        }
    }
    p.hit_with(x, y, w, BTN_H, 12, Action::FocusLibrarySearch, false, Hover::Soft);
    if !query.is_empty() {
        p.icon(Icon::Close, x + w - 30, y + 12, 16, MUT, PANEL_HI);
        p.hit_with(x + w - 36, y + 6, 28, 28, 14, Action::ClearSearch, false, Hover::Glow);
    }
}

fn sort_control(p: &mut Painter, right: i32, y: i32, active: usize) {
    let labels = ["Nom", "Plateforme", "État"];
    let widths: Vec<i32> = labels.iter().map(|l| p.width(l, 12.5, W::Semi).round() as i32 + 26).collect();
    let total: i32 = widths.iter().sum::<i32>() + 8;
    let x0 = right - total;
    let cap_w = p.width("Trier par", 12.5, W::Reg).round() as i32;
    p.text(x0 - cap_w - 12, y, 34, "Trier par", 12.5, W::Reg, FAINT);
    p.rrect(x0, y, total, 34, 17, PANEL);
    p.rrect_line(x0, y, total, 34, 17, alpha(Color::WHITE, 18));
    let mut sx = x0 + 4;
    for (i, label) in labels.iter().enumerate() {
        let sw = widths[i];
        if i == active {
            p.rrect(sx, y + 4, sw, 26, 13, ACT);
            p.rrect_line(sx, y + 4, sw, 26, 13, alpha(VIO, 90));
        }
        p.text(sx + 13, y + 4, 26, label, 12.5, W::Semi, if i == active { VIO_HI } else { MUT });
        p.hit_with(sx, y + 4, sw, 26, 13, Action::SetLibrarySort(i), false, Hover::Soft);
        sx += sw;
    }
}

fn sort_width(p: &mut Painter) -> i32 {
    let labels = ["Nom", "Plateforme", "État"];
    let inner: i32 = labels.iter().map(|l| p.width(l, 12.5, W::Semi).round() as i32 + 26).sum();
    inner + 8 + p.width("Trier par", 12.5, W::Reg).round() as i32 + 12
}

// ------------------------------------------------------------- library data
/// Matches every search term against the actual metadata stored for a game.
pub fn library_matches_query(game: &IpcGameEntry, query: &str) -> bool {
    let terms = query.split_whitespace().map(str::to_lowercase).collect::<Vec<_>>();
    if terms.is_empty() {
        return true;
    }
    let searchable = format!(
        "{} {} {} {} {}",
        game.name,
        game.platform,
        game.platform_id.as_deref().unwrap_or_default(),
        game.install_path,
        game.executable.as_deref().unwrap_or_default(),
    )
    .to_lowercase();
    terms.iter().all(|term| searchable.contains(term))
}

/// Returns distinct, non-empty platform names in case-insensitive display order.
pub fn library_platforms(games: &[IpcGameEntry]) -> Vec<String> {
    let mut platforms = Vec::new();
    for platform in games.iter().map(|game| game.platform.trim()).filter(|value| !value.is_empty()) {
        if !platforms.iter().any(|known: &String| known.eq_ignore_ascii_case(platform)) {
            platforms.push(platform.to_owned());
        }
    }
    platforms.sort_by_key(|platform| platform.to_lowercase());
    platforms
}

pub struct LibraryView<'a> {
    pub games: &'a [IpcGameEntry],
    pub statuses: &'a [GameStatus],
    pub detected: &'a [amethys_discovery::DetectedGame],
    pub scanning: bool,
    pub progress: Option<&'a amethys_discovery::DiscoveryProgress>,
    /// `Some(n)` once a scan has finished: n installations were found.
    pub scan_found: Option<usize>,
    pub filter: usize,
    pub sort: usize,
    pub query: &'a str,
    pub search_focused: bool,
    pub core_error: Option<&'a str>,
    /// Maximum number of cards drawn before "Afficher plus".
    pub limit: usize,
}

/// Indices of the games to show, filtered then sorted. Pure and testable.
pub fn visible_games(view: &LibraryView) -> Vec<usize> {
    let platforms = library_platforms(view.games);
    let active = if view.filter <= platforms.len() { view.filter } else { 0 };
    let selected = active.checked_sub(1).and_then(|i| platforms.get(i)).map(String::as_str);
    let rank = |i: usize| view.statuses.get(i).map_or(9, |s| s.kind.rank());
    let mut visible: Vec<usize> = (0..view.games.len())
        .filter(|&i| selected.is_none_or(|pl| view.games[i].platform.trim().eq_ignore_ascii_case(pl)))
        .filter(|&i| library_matches_query(&view.games[i], view.query))
        .collect();
    visible.sort_by(|&a, &b| {
        let (ga, gb) = (&view.games[a], &view.games[b]);
        let ordering = match view.sort % 3 {
            0 => ga.name.to_lowercase().cmp(&gb.name.to_lowercase()),
            1 => ga.platform.to_lowercase().cmp(&gb.platform.to_lowercase()),
            _ => rank(a).cmp(&rank(b)),
        };
        ordering.then_with(|| ga.name.to_lowercase().cmp(&gb.name.to_lowercase()))
    });
    visible
}

// -------------------------------------------------------------- game card
fn game_card(p: &mut Painter, x: i32, y: i32, cw: i32, ch: i32, index: usize, game: &IpcGameEntry, status: Option<&GameStatus>) {
    let total_h = ch + FOOTER_H;
    let kind = status.map_or(StatusKind::NeedsExecutable, |s| s.kind);
    p.shadow(x, y, cw, total_h, 14, 9, 40);
    p.fill(x, y, cw, total_h, PANEL);
    let artwork_id = game.catalogue_game_id.as_deref().unwrap_or(&game.id);
    p.cover_or_placeholder(artwork_id, &game.id, &game.name, x, y, cw, ch);
    p.scrim(x, y, cw, ch, 0.62, 225);

    chip(p, x + 10, y + 10, &game.platform, cw - 60);
    if kind != StatusKind::Ready {
        let (fg, _) = tone(kind);
        p.circle(x + cw - 21, y + 21, 11, Color::rgba(8, 6, 14, 200));
        p.icon(Icon::Alert, x + cw - 27, y + 15, 12, fg, Color::rgba(8, 6, 14, 255));
    }
    let lh = 20;
    let lines = p.paragraph_height(cw - 28, lh, &game.name, 16.0, W::Disp, 2);
    p.paragraph(x + 14, y + ch - 14 - lines, cw - 28, lh, &game.name, 16.0, W::Disp, Color::WHITE, 2);

    let (fg, _) = tone(kind);
    let fy = y + ch;
    p.circle(x + 18, fy + 22, 4, fg);
    p.text(x + 29, fy + 12, 20, kind.label(), 12.5, W::Semi, fg);
    let path = p.ellipsize_path(&game.install_path, 11.0, W::Reg, cw - 32 - 44);
    p.text(x + 16, fy + 35, 16, &path, 11.0, W::Reg, FAINT);

    p.round_corners(x, y, cw, total_h, 14, BG);
    p.rrect_line(x, y, cw, total_h, 14, alpha(Color::WHITE, 20));
    p.hit_with(x, y, cw, total_h, 14, Action::SelectGame(index), false, Hover::Card);
    if kind == StatusKind::Ready {
        let (bx, by) = (x + cw - 14 - 32, fy + 10);
        p.circle(bx + 16, by + 16, 16, VIO);
        p.icon(Icon::Play, bx + 9, by + 9, 14, VIO_DARK, VIO);
        p.hit_with(bx - 2, by - 2, 36, 36, 18, Action::LaunchGame(index), false, Hover::Glow);
    }
}

// ----------------------------------------------------------------- library
pub fn library(p: &mut Painter, lay: &Layout, view: &LibraryView) -> i32 {
    backdrop(p, lay);
    let (mx, mw) = (lay.mx, lay.mw);
    let compact = lay.compact();
    let games = view.games;
    let ready = view.statuses.iter().filter(|s| s.kind == StatusKind::Ready).count();

    // ---- header
    p.text(mx, 28, 42, "Bibliothèque", 34.0, W::Disp, TXT);
    let subtitle = if games.is_empty() {
        "Aucun jeu enregistré pour le moment".to_owned()
    } else {
        format!("{} · {}", plural(games.len(), "jeu", "jeux"), format!("{} {}", ready, if ready > 1 { "prêts à lancer" } else { "prêt à lancer" }))
    };
    p.text(mx, 72, 22, &subtitle, 13.5, W::Reg, MUT);

    // ---- toolbar: search, refresh, detect
    let refresh_w = button_width(p, "Actualiser", Some(Icon::Refresh));
    let scan_label = if view.scanning { "Annuler" } else { "Détecter les jeux" };
    let scan_icon = if view.scanning { Icon::Close } else { Icon::Search };
    let scan_w = button_width(p, scan_label, Some(scan_icon));
    let search_w = if compact { mw } else { 300 };
    let toolbar_w = search_w + 12 + refresh_w + 12 + scan_w;
    let (tx, ty) = if compact { (mx, 108) } else { (mx + mw - toolbar_w, 30) };
    search_field(p, tx, ty, search_w, view.query, view.search_focused);
    // On narrow windows, put the actions on a second row rather than forcing
    // the search field and buttons outside the content column.
    let stack_actions = compact && mw < refresh_w + scan_w + 12;
    let actions_x = if stack_actions { mx } else if compact { mx + mw - refresh_w - 12 - scan_w } else { tx + search_w + 12 };
    let actions_y = if compact { ty + BTN_H + 12 } else { ty };
    let scan_y = actions_y + if stack_actions { BTN_H + 8 } else { 0 };
    button(p, actions_x, actions_y, Some(refresh_w), "Actualiser", Some(Icon::Refresh), Btn::Secondary, BG, Some(Action::RefreshLibrary));
    if view.scanning {
        button(p, if stack_actions { mx } else { actions_x + refresh_w + 12 }, scan_y, Some(scan_w), scan_label, Some(scan_icon), Btn::Secondary, BG, Some(Action::CancelScan));
    } else {
        button(p, actions_x + refresh_w + 12, actions_y, Some(scan_w), scan_label, Some(scan_icon), Btn::Primary, BG, Some(Action::ScanGames));
    }
    let mut y = if compact { scan_y + BTN_H + 22 } else { 118 };

    // ---- scan progress
    if view.scanning {
        panel(p, mx, y, mw, 78, 16);
        p.text(mx + 22, y + 14, 22, "Détection des jeux installés", 14.0, W::Semi, TXT);
        let (label, ratio) = match view.progress {
            Some(pr) => (
                format!("{} · {}/{} sources · {} trouvé(s)", pr.source, pr.completed, pr.total, pr.found),
                if pr.total > 0 { pr.completed as f32 / pr.total as f32 } else { 0.0 },
            ),
            None => ("Préparation de l'analyse…".to_owned(), 0.0),
        };
        let label = p.ellipsize(&label, 12.0, W::Reg, mw - 44);
        p.text(mx + 22, y + 36, 18, &label, 12.0, W::Reg, MUT);
        let bar_w = mw - 44;
        p.rrect(mx + 22, y + 60, bar_w, 6, 3, alpha(Color::WHITE, 18));
        let fill = ((bar_w as f32 * ratio.clamp(0.0, 1.0)) as i32).max(if view.progress.is_some() { 10 } else { 0 });
        if fill > 0 {
            p.gradient_rrect(mx + 22, y + 60, fill, 6, 3, 90.0, &[(0.0, VIO_LO), (1.0, VIO_HI)], PANEL);
        }
        y += 78 + GAP;
    }

    // ---- Core error
    if let Some(error) = view.core_error {
        let narrow_error = mw < 420;
        let msg_w = if narrow_error { mw - 40 } else { mw - 200 };
        let msg_h = p.paragraph_height(msg_w, 19, error, 12.5, W::Reg, 3);
        let h = if narrow_error { (msg_h + 112).max(150) } else { (msg_h + 62).max(92) };
        panel(p, mx, y, mw, h, 16);
        icon_badge(p, mx + 20, y + 20, 44, Icon::Alert, AMBER, AMBER_DARK);
        if narrow_error {
            p.text(mx + 76, y + 16, 24, "Core indisponible", 15.0, W::Semi, TXT);
            p.paragraph(mx + 20, y + 72, msg_w, 19, error, 12.5, W::Reg, MUT, 3);
            button(p, mx + 20, y + h - 20 - BTN_H, Some(120), "Réessayer", Some(Icon::Refresh), Btn::Secondary, PANEL, Some(Action::RefreshLibrary));
        } else {
            p.text(mx + 80, y + 16, 24, "Connexion à Amethys Core impossible", 16.0, W::Semi, TXT);
            p.paragraph(mx + 80, y + 44, mw - 240, 19, error, 12.5, W::Reg, MUT, 3);
            button(p, mx + mw - 20 - 120, y + (h - BTN_H) / 2, Some(120), "Réessayer", Some(Icon::Refresh), Btn::Secondary, PANEL, Some(Action::RefreshLibrary));
        }
        y += h + GAP;
    }

    // ---- stat tiles
    if !games.is_empty() {
        let count = |kinds: &[StatusKind]| view.statuses.iter().filter(|s| kinds.contains(&s.kind)).count();
        let tiles = [
            ("Jeux", games.len(), VIO),
            ("Prêts à lancer", ready, GREEN),
            ("À configurer", count(&[StatusKind::NeedsExecutable, StatusKind::InvalidExecutable]), AMBER),
            ("Dossiers introuvables", count(&[StatusKind::FolderMissing]), RED),
        ];
        let per_row = if mw < 420 { 1 } else if compact { 2 } else { 4 };
        let tw = (mw - GAP * (per_row - 1)) / per_row;
        for (i, (label, n, color)) in tiles.iter().enumerate() {
            let (col, row) = (i as i32 % per_row, i as i32 / per_row);
            let (x, ty) = (mx + col * (tw + GAP), y + row * (78 + 12));
            panel(p, x, ty, tw, 78, 16);
            p.circle(x + 22, ty + 22, 4, *color);
            p.text(x + 34, ty + 12, 20, label, 12.5, W::Med, MUT);
            p.text(x + 20, ty + 34, 38, &n.to_string(), 28.0, W::Disp, TXT);
        }
        let rows = (tiles.len() as i32 + per_row - 1) / per_row;
        y += rows * 78 + (rows - 1) * 12 + 24;
    }

    // ---- filters and sort
    if !games.is_empty() {
        let platforms = library_platforms(games);
        let active = if view.filter <= platforms.len() { view.filter } else { 0 };
        let sort_w = sort_width(p);
        let avail = if compact { mw } else { mw - sort_w - 24 };
        let (mut fx, mut fy) = (mx, y);
        let w_all = pill(p, fx, fy, "Tous", Some(games.len()), active == 0, Action::LibFilter(0));
        fx += w_all + 8;
        for (i, platform) in platforms.iter().enumerate() {
            let n = games.iter().filter(|g| g.platform.trim().eq_ignore_ascii_case(platform)).count();
            let est = p.width(platform, 13.0, W::Semi).round() as i32 + 32 + p.width(&n.to_string(), 11.5, W::Semi).round() as i32 + 8;
            if fx + est > mx + avail && fx > mx {
                fx = mx;
                fy += 42;
            }
            fx += pill(p, fx, fy, platform, Some(n), active == i + 1, Action::LibFilter(i + 1)) + 8;
        }
        if compact {
            fy += 46;
        }
        sort_control(p, mx + mw, if compact { fy } else { y }, view.sort % 3);
        y = fy + 34 + if compact { 0 } else { 22 };
        if compact {
            y += 8;
        }
    }

    // ---- grid or empty states
    let visible = visible_games(view);
    if games.is_empty() && view.core_error.is_none() {
        let cta = if view.scanning { None } else { Some(("Détecter les jeux", Action::ScanGames, Btn::Primary)) };
        y += empty_state(p, mx, y, mw, Icon::Gem, "Votre bibliothèque est vide", "Lancez la détection pour retrouver les jeux déjà installés sur vos plateformes prises en charge. Rien n'est modifié sur votre disque.", cta) + GAP;
    } else if !games.is_empty() && visible.is_empty() {
        let (title, desc, cta) = if !view.query.trim().is_empty() {
            ("Aucun résultat", "Aucun jeu ne correspond à votre recherche. Essayez un autre nom ou une autre plateforme.", ("Effacer la recherche", Action::ClearSearch))
        } else {
            ("Aucun jeu pour ce filtre", "Aucun jeu enregistré ne correspond à cette plateforme.", ("Afficher tous les jeux", Action::LibFilter(0)))
        };
        y += empty_state(p, mx, y, mw, Icon::Search, title, desc, Some((cta.0, cta.1, Btn::Secondary))) + GAP;
    } else if !visible.is_empty() {
        if visible.len() != games.len() {
            p.text(mx, y - 8, 22, &format!("{} sur {} affichés", visible.len(), plural(games.len(), "jeu", "jeux")), 12.5, W::Reg, MUT);
            y += 22;
        }
        let cols = lay.columns();
        let cw = lay.card_width();
        let ch = cw * 4 / 3;
        let row_h = ch + FOOTER_H + GAP;
        let shown = visible.len().min(view.limit.max(1));
        for (n, &index) in visible.iter().take(shown).enumerate() {
            let (col, row) = (n as i32 % cols, n as i32 / cols);
            game_card(p, mx + col * (cw + GAP), y + row * row_h, cw, ch, index, &games[index], view.statuses.get(index));
        }
        let rows = (shown as i32 + cols - 1) / cols;
        y += rows * row_h + 6;
        if visible.len() > shown {
            let more = (visible.len() - shown).min(48);
            let label = format!("Afficher {} de plus", plural(more, "jeu", "jeux"));
            let bw = button_width(p, &label, None) + 24;
            button(p, mx + (mw - bw) / 2, y, Some(bw), &label, None, Btn::Secondary, BG, Some(Action::ShowMoreGames));
            p.text_center(mx, mw, y + BTN_H + 8, 18, &format!("{shown} affichés sur {}", visible.len()), 12.0, W::Reg, FAINT);
            y += BTN_H + 40;
        }
    }

    // ---- detected installations
    if !view.detected.is_empty() {
        y += 14;
        p.text(mx, y, 30, "Installations détectées", 22.0, W::Disp, TXT);
        p.text_right(mx + mw, y, 30, &format!("{} à importer", view.detected.len()), 13.0, W::Reg, MUT);
        y += 46;
        for (index, game) in view.detected.iter().take(DETECTED_LIMIT).enumerate() {
            if mw < 420 {
                panel(p, mx, y, mw, 96, 14);
                let pw = p.width(game.platform, 11.5, W::Semi).round() as i32 + 20;
                p.rrect(mx + 18, y + 10, pw, 22, 11, ACT);
                p.text(mx + 28, y + 10, 22, game.platform, 11.5, W::Semi, VIO_HI);
                button(p, mx + mw - 18 - 92, y + 8, Some(92), "Importer", None, Btn::Primary, PANEL, Some(Action::ImportDetectedGame(index)));
                p.text_fit(mx + 18, y + 38, 22, &game.name, 13.0, W::Semi, TXT, mw - 36);
                let path = p.ellipsize_path(&game.install_path.to_string_lossy(), 10.5, W::Reg, mw - 36);
                p.text(mx + 18, y + 64, 16, &path, 10.5, W::Reg, FAINT);
                y += 96 + 10;
            } else {
                panel(p, mx, y, mw, 66, 14);
                let pw = p.width(game.platform, 11.5, W::Semi).round() as i32 + 20;
                p.rrect(mx + 18, y + 22, pw, 22, 11, ACT);
                p.text(mx + 28, y + 22, 22, game.platform, 11.5, W::Semi, VIO_HI);
                let text_x = mx + 18 + pw + 16;
                let max_w = mw - (text_x - mx) - 150;
                p.text_fit(text_x, y + 10, 24, &game.name, 14.5, W::Semi, TXT, max_w);
                let path = p.ellipsize_path(&game.install_path.to_string_lossy(), 11.5, W::Reg, max_w);
                p.text(text_x, y + 34, 18, &path, 11.5, W::Reg, FAINT);
                button(p, mx + mw - 18 - 112, y + 13, Some(112), "Importer", None, Btn::Primary, PANEL, Some(Action::ImportDetectedGame(index)));
                y += 66 + 10;
            }
        }
        if view.detected.len() > DETECTED_LIMIT {
            p.text(mx, y + 2, 20, &format!("{DETECTED_LIMIT} premières installations affichées sur {}. Importez-les pour faire apparaître les suivantes.", view.detected.len()), 12.0, W::Reg, MUT);
            y += 28;
        }
    } else if view.scan_found == Some(0) {
        y += 8;
        panel(p, mx, y, mw, 72, 16);
        icon_badge(p, mx + 18, y + 16, 40, Icon::Check, GREEN, GREEN_DARK);
        p.text(mx + 72, y, 72, "Aucune nouvelle installation à importer.", 14.0, W::Med, TXT);
        y += 72 + GAP;
    }
    y + 40
}

// ------------------------------------------------------------- game detail
pub struct DetailView<'a> {
    pub game: Option<&'a IpcGameEntry>,
    pub index: usize,
    pub status: Option<&'a GameStatus>,
    pub executable_draft: &'a str,
    pub editing_executable: bool,
}

/// One label / value row of an information panel. Returns the row height.
fn info_row(p: &mut Painter, x: i32, y: i32, w: i32, first: bool, label: &str, value: &str, color: Color, ic: Option<Icon>) -> i32 {
    if !first {
        p.fill(x, y, w, 1, alpha(Color::WHITE, 12));
    }
    let label_w = 150.min(w / 3);
    p.text(x, y + 12, 20, label, 12.5, W::Reg, MUT);
    let vx = x + label_w + 12;
    let vw = w - label_w - 12;
    let mut text_x = vx;
    if let Some(i) = ic {
        p.icon(i, vx, y + 12, 16, color, PANEL);
        text_x += 24;
    }
    let shown = if value.contains('/') || value.contains('\\') { p.ellipsize_path(value, 13.0, W::Med, vw - (text_x - vx)) } else { value.to_owned() };
    let lh = 20;
    let h = p.paragraph_height(vw - (text_x - vx), lh, &shown, 13.0, W::Med, 3);
    p.paragraph(text_x, y + 12, vw - (text_x - vx), lh, &shown, 13.0, W::Med, color, 3);
    h.max(20) + 24
}

pub fn game_detail(p: &mut Painter, lay: &Layout, view: &DetailView) -> i32 {
    backdrop(p, lay);
    let (mx, mw) = (lay.mx, lay.mw);
    button(p, mx, 26, None, "Bibliothèque", Some(Icon::Back), Btn::Ghost, BG, Some(Action::Go(Page::Library)));

    let Some(game) = view.game else {
        empty_state(p, mx, 100, mw, Icon::Info, "Aucun jeu sélectionné", "Retournez à la bibliothèque pour choisir un jeu.", Some(("Retour à la bibliothèque", Action::Go(Page::Library), Btn::Secondary)));
        return 420;
    };
    let status = view.status.cloned().unwrap_or_else(|| crate::status::classify(false, game.executable.is_some(), None));
    let ready = status.is_ready();

    // ---- hero geometry
    let hero_y = 84;
    let compact = lay.compact();
    // Hide the side poster on very narrow windows so the title and actions
    // retain the full hero width instead of being squeezed into a thin column.
    let (poster_w, poster_h, pad) = if mw < 420 { (0, 0, 16) } else if compact { (112, 149, 20) } else { (200, 267, 32) };
    let poster_gap = if poster_w == 0 { 0 } else if compact { 16 } else { 36 };
    let cx0 = mx + pad + poster_w + poster_gap;
    let cw = mx + mw - pad - cx0;

    // actions flow (wraps when the hero is narrow)
    struct Act<'a> {
        label: &'a str,
        icon: Icon,
        kind: Btn,
        action: Option<Action>,
    }
    let acts = [
        if ready {
            Act { label: "Lancer", icon: Icon::Play, kind: Btn::Primary, action: Some(Action::LaunchGame(view.index)) }
        } else {
            Act { label: "Lancer indisponible", icon: Icon::Play, kind: Btn::Secondary, action: None }
        },
        Act { label: "Exécutable", icon: Icon::Edit, kind: Btn::Secondary, action: Some(Action::EditExecutable) },
        Act { label: "Jaquette", icon: Icon::Image, kind: Btn::Ghost, action: Some(Action::ChangeGameCover) },
        Act { label: "Par défaut", icon: Icon::Refresh, kind: Btn::Ghost, action: Some(Action::ResetGameCover) },
        Act { label: "Retirer", icon: Icon::Trash, kind: Btn::Danger, action: Some(Action::RemoveGame(view.index)) },
    ];
    let widths: Vec<i32> = acts.iter().map(|a| button_width(p, a.label, Some(a.icon))).collect();
    let mut placements = Vec::new();
    let (mut bx, mut brow) = (0, 0);
    for w in &widths {
        if bx > 0 && bx + w > cw {
            bx = 0;
            brow += 1;
        }
        placements.push((bx, brow));
        bx += w + 10;
    }
    let rows = brow + 1;

    // text stack above the buttons
    let title_lines = p.wrap(&game.name, 38.0, W::Disp, cw).len().clamp(1, 2) as i32;
    let metadata = game.catalogue_metadata.as_ref();
    let subtitle = metadata.map(|m| {
        [m.developer.as_deref(), m.publisher.as_deref()]
            .iter()
            .flatten()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
            .join(" · ")
    }).filter(|s| !s.is_empty());
    let top_h = 28 + 12 + title_lines * 44 + 8 + if subtitle.is_some() { 30 } else { 0 };
    let hero_h = (poster_h + 2 * pad).max(pad + top_h + 20 + rows * (BTN_H + 10) - 10 + pad);

    // ---- hero background: blurred artwork, darkened
    let artwork_id = game.catalogue_game_id.as_deref().unwrap_or(&game.id);
    p.blurred_cover_banner(artwork_id, &game.id, &game.name, mx, hero_y, mw, hero_h);
    p.gradient(mx, hero_y, mw, hero_h, 90.0, &[(0.0, Color::rgba(11, 10, 17, 170)), (1.0, Color::rgba(11, 10, 17, 120))]);
    p.gradient(mx, hero_y, mw, hero_h, 180.0, &[(0.0, Color::rgba(11, 10, 17, 0)), (1.0, Color::rgba(11, 10, 17, 190))]);
    p.round_corners(mx, hero_y, mw, hero_h, 22, BG);
    p.rrect_line(mx, hero_y, mw, hero_h, 22, alpha(Color::WHITE, 24));

    // poster
    if poster_w > 0 {
        let (px, py) = (mx + pad, hero_y + (hero_h - poster_h) / 2);
        p.shadow(px, py, poster_w, poster_h, 16, 14, 90);
        p.cover_or_placeholder(artwork_id, &game.id, &game.name, px, py, poster_w, poster_h);
        p.round_corners(px, py, poster_w, poster_h, 16, Color::rgb(0x0F, 0x0D, 0x16));
        p.rrect_line(px, py, poster_w, poster_h, 16, alpha(Color::WHITE, 40));
    }

    // chips, title, subtitle
    let mut ty = hero_y + pad;
    let mut chip_x = cx0;
    let platform = p.ellipsize(&game.platform, 12.0, W::Semi, cw / 2);
    let pw = p.width(&platform, 12.0, W::Semi).round() as i32 + 22;
    p.rrect(chip_x, ty, pw, 26, 13, Color::rgba(8, 6, 14, 190));
    p.rrect_line(chip_x, ty, pw, 26, 13, alpha(Color::WHITE, 26));
    p.text(chip_x + 11, ty, 26, &platform, 12.0, W::Semi, Color::WHITE);
    chip_x += pw + 8;
    status_badge(p, chip_x, ty, status.kind);
    ty += 28 + 12;
    p.paragraph(cx0, ty, cw, 44, &game.name, 38.0, W::Disp, Color::WHITE, 2);
    ty += title_lines * 44 + 8;
    if let Some(sub) = &subtitle {
        p.text_fit(cx0, ty, 22, sub, 14.0, W::Med, SOFT, cw);
    }

    // buttons, anchored at the bottom of the hero
    let by0 = hero_y + hero_h - pad - (rows * (BTN_H + 10) - 10);
    for (i, act) in acts.iter().enumerate() {
        let (x_off, row) = placements[i];
        let (x, y) = (cx0 + x_off, by0 + row * (BTN_H + 10));
        if act.action.is_none() {
            button_disabled(p, x, y, act.label, Some(act.icon));
        } else {
            button(p, x, y, Some(widths[i]), act.label, Some(act.icon), act.kind, Color::rgb(0x12, 0x10, 0x1A), act.action);
        }
    }

    // ---- below the hero
    let mut y = hero_y + hero_h + GAP + 4;

    if view.editing_executable {
        let narrow_edit = mw < 420;
        let h = if narrow_edit { 220 } else { 168 };
        panel(p, mx, y, mw, h, 18);
        p.text(mx + 24, y + 18, 24, "Configurer l'exécutable", 16.0, W::Semi, TXT);
        p.text_fit(mx + 24, y + 44, 20, "Chemin relatif au dossier du jeu (ex. bin/game.exe), ou chemin absolu situé dans ce dossier.", 12.5, W::Reg, MUT, mw - 48);
        let (fx, fy, fw) = (mx + 24, y + 74, mw - 48);
        p.rrect(fx, fy, fw, BTN_H, 12, PANEL_HI);
        p.rrect_line(fx, fy, fw, BTN_H, 12, VIO);
        p.rrect_line(fx + 1, fy + 1, fw - 2, BTN_H - 2, 11, alpha(VIO, 110));
        if view.executable_draft.is_empty() {
            p.text(fx + 14, fy, BTN_H, "bin/game.exe", 13.5, W::Reg, FAINT);
            p.rrect(fx + 14, fy + 11, 2, 18, 1, VIO_HI);
        } else {
            let shown = tail_fit(p, view.executable_draft, 13.5, W::Reg, fw - 40);
            let tw = p.text(fx + 14, fy, BTN_H, &shown, 13.5, W::Reg, TXT).round() as i32;
            p.rrect(fx + 15 + tw, fy + 11, 2, 18, 1, VIO_HI);
        }
        let bw = 130;
        if narrow_edit {
            let by = y + 74 + BTN_H + 14;
            button(p, mx + 24, by, Some(120), "Enregistrer", Some(Icon::Check), Btn::Primary, PANEL, Some(Action::SaveExecutable));
            button(p, mx + 24, by + BTN_H + 8, Some(110), "Annuler", None, Btn::Ghost, PANEL, Some(Action::CancelExecutableEdit));
        } else {
            button(p, mx + mw - 24 - bw, y + 74 + BTN_H + 14, Some(bw), "Enregistrer", Some(Icon::Check), Btn::Primary, PANEL, Some(Action::SaveExecutable));
            button(p, mx + mw - 24 - bw - 10 - 110, y + 74 + BTN_H + 14, Some(110), "Annuler", None, Btn::Ghost, PANEL, Some(Action::CancelExecutableEdit));
        }
        y += h + GAP;
    }

    // columns: information on the left, technical details on the right
    let tech_w = if compact { mw } else { 360 };
    let main_w = if compact { mw } else { mw - tech_w - GAP };
    let (lx, rx) = (mx, if compact { mx } else { mx + main_w + GAP });
    let mut ly = y;

    // installation
    {
        let inner = main_w - 48;
        let (exec_text, exec_color) = match game.executable.as_deref() {
            Some(e) => (e.to_owned(), TXT),
            None => ("Non configuré".to_owned(), AMBER),
        };
        let (avail_text, avail_color, avail_icon) = match &status.block_reason {
            None => ("Prêt à lancer".to_owned(), GREEN, Icon::Check),
            Some(reason) => (reason.clone(), AMBER, Icon::Alert),
        };
        let (dir_text, dir_color, dir_icon) = if status.directory_exists { ("Présent sur le disque", GREEN, Icon::Check) } else { ("Introuvable sur le disque", RED, Icon::Alert) };
        // Measure first so the panel fits its rows.
        let start = p.hits.len();
        let mut probe = 62;
        let mut tmp_y = 0;
        let _ = (&mut probe, &mut tmp_y);
        let _ = start;
        let rows_h = measure_rows(p, inner, &[
            (game.install_path.as_str(), false),
            (dir_text, true),
            (exec_text.as_str(), false),
            (avail_text.as_str(), true),
        ]);
        let h = 58 + rows_h + 8;
        panel(p, lx, ly, main_w, h, 18);
        p.text(lx + 24, ly + 18, 24, "Installation", 16.0, W::Semi, TXT);
        let mut ry = ly + 54;
        ry += info_row(p, lx + 24, ry, inner, true, "Dossier d'installation", &game.install_path, TXT, Some(Icon::Folder));
        ry += info_row(p, lx + 24, ry, inner, false, "État du dossier", dir_text, dir_color, Some(dir_icon));
        ry += info_row(p, lx + 24, ry, inner, false, "Exécutable configuré", &exec_text, exec_color, None);
        info_row(p, lx + 24, ry, inner, false, "Disponibilité", &avail_text, avail_color, Some(avail_icon));
        ly += h + GAP;
    }

    // about
    {
        let inner = main_w - 48;
        let genres: Vec<&str> = metadata.map(|m| m.genres.iter().map(String::as_str).collect()).unwrap_or_default();
        let description = metadata.and_then(|m| m.description.as_deref()).filter(|d| !d.trim().is_empty());
        let mut h = 58;
        if metadata.is_some() {
            h += 3 * 44 - 8;
            if !genres.is_empty() {
                h += 50;
            }
            if let Some(d) = description {
                h += 22 + p.paragraph_height(inner, 22, d, 13.0, W::Reg, 12) + 8;
            }
            h += 12;
        } else {
            h += 56;
        }
        panel(p, lx, ly, main_w, h, 18);
        p.text(lx + 24, ly + 18, 24, "À propos", 16.0, W::Semi, TXT);
        let mut ay = ly + 54;
        if let Some(m) = metadata {
            ay += info_row(p, lx + 24, ay, inner, true, "Développeur", m.developer.as_deref().unwrap_or("Non renseigné"), if m.developer.is_some() { TXT } else { FAINT }, None).max(44);
            ay += info_row(p, lx + 24, ay, inner, false, "Éditeur", m.publisher.as_deref().unwrap_or("Non renseigné"), if m.publisher.is_some() { TXT } else { FAINT }, None).max(44);
            ay += info_row(p, lx + 24, ay, inner, false, "Date de sortie", m.release_date.as_deref().unwrap_or("Non renseignée"), if m.release_date.is_some() { TXT } else { FAINT }, None).max(44);
            if !genres.is_empty() {
                ay += 10;
                let mut gx = lx + 24;
                for genre in genres.iter().take(8) {
                    let gw = p.width(genre, 12.0, W::Med).round() as i32 + 24;
                    if gx + gw > lx + 24 + inner {
                        break;
                    }
                    p.rrect(gx, ay, gw, 28, 14, ACT);
                    p.rrect_line(gx, ay, gw, 28, 14, alpha(VIO, 70));
                    p.text(gx + 12, ay, 28, genre, 12.0, W::Med, VIO_HI);
                    gx += gw + 8;
                }
                ay += 40;
            }
            if let Some(d) = description {
                ay += 12;
                p.paragraph(lx + 24, ay, inner, 22, d, 13.0, W::Reg, SOFT, 12);
            }
        } else {
            p.icon(Icon::Info, lx + 24, ay + 6, 18, FAINT, PANEL);
            p.text_fit(lx + 52, ay, 30, "Aucune métadonnée catalogue n'est encore associée à ce jeu.", 13.0, W::Reg, MUT, inner - 28);
        }
        ly += h + GAP;
    }

    // technical details
    let mut ry = if compact { ly } else { y };
    {
        let inner = tech_w - 48;
        let rows: [(&str, String); 3] = [
            ("Game ID Amethys", game.catalogue_game_id.clone().unwrap_or_else(|| "Non attribué".to_owned())),
            ("ID d'installation locale", game.id.clone()),
            ("Identifiant de plateforme", game.platform_id.clone().unwrap_or_else(|| "Non fourni".to_owned())),
        ];
        let h = 80 + rows.len() as i32 * 54 + 10;
        panel(p, rx, ry, tech_w, h, 18);
        p.text(rx + 24, ry + 18, 24, "Détails techniques", 16.0, W::Semi, TXT);
        p.text_fit(rx + 24, ry + 42, 18, "Pour le diagnostic et le support", 12.0, W::Reg, FAINT, inner);
        let mut ty = ry + 76;
        for (i, (label, value)) in rows.iter().enumerate() {
            if i > 0 {
                p.fill(rx + 24, ty - 6, inner, 1, alpha(Color::WHITE, 12));
            }
            p.text(rx + 24, ty, 18, label, 11.5, W::Reg, FAINT);
            p.text_fit(rx + 24, ty + 20, 20, value, 12.5, W::Med, SOFT, inner);
            ty += 54;
        }
        ry += h + GAP;
    }
    ly.max(ry) + 30
}

/// Height of the rows `info_row` would draw, without drawing them.
fn measure_rows(p: &mut Painter, inner: i32, rows: &[(&str, bool)]) -> i32 {
    let label_w = 150.min(inner / 3);
    let vw = inner - label_w - 12;
    rows.iter()
        .map(|(value, has_icon)| {
            let width = vw - if *has_icon || value.contains('/') || value.contains('\\') { 24 } else { 0 };
            let shown = if value.contains('/') || value.contains('\\') { p.ellipsize_path(value, 13.0, W::Med, width) } else { (*value).to_owned() };
            p.paragraph_height(width, 20, &shown, 13.0, W::Med, 3).max(20) + 24
        })
        .sum()
}

// ------------------------------------------------------------------- store
/// The catalogue has no connected provider yet; nothing is invented here.
pub fn store(p: &mut Painter, lay: &Layout) -> i32 {
    backdrop(p, lay);
    let (mx, mw) = (lay.mx, lay.mw);
    p.text(mx, 28, 42, "Boutique", 34.0, W::Disp, TXT);
    p.text(mx, 72, 22, "Le catalogue n'est pas encore connecté", 13.5, W::Reg, MUT);

    let y = 118;
    let h = 252;
    panel(p, mx, y, mw, h, 22);
    p.glow(mx + 160, y + 40, 420, Color::rgba(122, 92, 232, 55), mx + 1, y + 1, mw - 2, h - 2);
    p.round_corners(mx, y, mw, h, 22, BG);
    p.rrect_line(mx, y, mw, h, 22, alpha(Color::WHITE, 18));
    if mw < 420 {
        p.gradient_rrect(mx + 24, y + 24, 56, 56, 18, 135.0, &[(0.0, ACT), (1.0, hex("#1B1432"))], PANEL);
        p.rrect_line(mx + 24, y + 24, 56, 56, 18, alpha(VIO, 100));
        p.icon(Icon::Gem, mx + 38, y + 38, 28, VIO_HI, ACT);
        p.text(mx + 94, y + 28, 22, "Version 0.3", 12.0, W::Semi, VIO_HI);
        p.text(mx + 24, y + 96, 28, "La boutique arrive", 20.0, W::Disp, TXT);
        p.paragraph(mx + 24, y + 132, mw - 48, 20,
            "La version 0.2 se concentre sur votre bibliothèque locale. Les catalogues, prix, promotions et achats ne sont pas encore connectés.",
            12.5, W::Reg, SOFT, 4);
        button(p, mx + 24, y + h - 24 - BTN_H, Some(mw - 48), "Ouvrir la bibliothèque", Some(Icon::Library), Btn::Secondary, PANEL, Some(Action::Go(Page::Library)));
    } else {
        p.gradient_rrect(mx + 32, y + 32, 64, 64, 22, 135.0, &[(0.0, ACT), (1.0, hex("#1B1432"))], PANEL);
        p.rrect_line(mx + 32, y + 32, 64, 64, 22, alpha(VIO, 100));
        p.icon(Icon::Gem, mx + 48, y + 48, 32, VIO_HI, ACT);
        p.rrect(mx + 120, y + 34, 78, 24, 12, ACT);
        p.rrect_line(mx + 120, y + 34, 78, 24, 12, alpha(VIO, 120));
        p.text(mx + 120, y + 34, 24, "  Version 0.3", 12.0, W::Semi, VIO_HI);
        p.text(mx + 120, y + 62, 40, "La boutique arrive avec la version 0.3", 26.0, W::Disp, TXT);
        p.paragraph(mx + 120, y + 112, mw - 152, 23,
            "La version 0.2 se concentre sur votre bibliothèque locale. Les catalogues, prix, promotions et achats ne sont pas encore connectés : rien de tout cela n'est simulé.",
            14.5, W::Reg, SOFT, 3);
        button(p, mx + 120, y + h - 32 - BTN_H, None, "Ouvrir la bibliothèque", Some(Icon::Library), Btn::Secondary, PANEL, Some(Action::Go(Page::Library)));
    }

    let items = [
        ("Catalogue fiable", "Les jeux et les offres viendront de fournisseurs réels, jamais de produits ou de prix inventés."),
        ("Personnalisation locale", "Vos recommandations seront calculées sur votre PC à partir de votre bibliothèque et de vos préférences."),
        ("Sans serveur payant obligatoire", "Le catalogue public pourra être distribué sous forme de fichiers statiques ; vos données personnelles resteront locales."),
    ];
    let per_row = if lay.compact() { 1 } else { 3 };
    let tw = (mw - GAP * (per_row - 1)) / per_row;
    let top = y + h + GAP + 6;
    let card_h = 132;
    for (i, (title, text)) in items.iter().enumerate() {
        let (col, row) = (i as i32 % per_row, i as i32 / per_row);
        let (x, cy) = (mx + col * (tw + GAP), top + row * (card_h + GAP));
        panel(p, x, cy, tw, card_h, 18);
        p.circle(x + 28, cy + 30, 4, VIO);
        p.text(x + 42, cy + 18, 24, title, 15.0, W::Semi, TXT);
        p.paragraph(x + 24, cy + 52, tw - 48, 20, text, 12.5, W::Reg, MUT, 4);
    }
    let rows = (items.len() as i32 + per_row - 1) / per_row;
    top + rows * (card_h + GAP) + 30
}

// ---------------------------------------------------------------- settings
pub struct SettingsView<'a> {
    pub core_error: Option<&'a str>,
    pub game_count: usize,
    pub ready_count: usize,
    pub media_root: Option<&'a std::path::Path>,
    pub scanning: bool,
    pub progress: Option<&'a amethys_discovery::DiscoveryProgress>,
}

pub fn settings(p: &mut Painter, lay: &Layout, view: &SettingsView) -> i32 {
    backdrop(p, lay);
    let (mx, mw) = (lay.mx, lay.mw);
    p.text(mx, 28, 42, "Paramètres", 34.0, W::Disp, TXT);
    p.text(mx, 72, 22, "Configuration locale d'Amethys", 13.5, W::Reg, MUT);

    // ---- Core status
    let mut y = 118;
    let core_h = if view.core_error.is_some() {
        if lay.compact() { 260 } else { 216 }
    } else if lay.compact() {
        222
    } else {
        178
    };
    panel(p, mx, y, mw, core_h, 20);
    let (ic, fg, bg, state) = if view.core_error.is_some() {
        (Icon::Alert, AMBER, AMBER_DARK, "Déconnecté")
    } else {
        (Icon::Check, GREEN, GREEN_DARK, "Connecté")
    };
    icon_badge(p, mx + 24, y + 24, 48, ic, fg, bg);
    p.text(mx + 88, y + 20, 26, "Amethys Core", 18.0, W::Semi, TXT);
    p.circle(mx + 90, y + 58, 4, fg);
    p.text(mx + 102, y + 48, 22, state, 13.0, W::Semi, fg);
    if let Some(error) = view.core_error {
        p.paragraph(mx + 88, y + 76, mw - 112, 19, error, 12.5, W::Reg, MUT, 3);
    } else {
        p.text(mx + 88, y + 76, 20, "La bibliothèque est chargée depuis le service local, par IPC.", 12.5, W::Reg, MUT);
    }
    let compact = lay.compact();
    let first_button_y = if compact { y + core_h - 24 - BTN_H * 2 - 8 } else { y + core_h - 24 - BTN_H };
    let refresh_label = if mw < 340 { "Actualiser" } else { "Actualiser la bibliothèque" };
    let scan_button_label = if mw < 340 { "Détecter" } else { "Détecter les jeux installés" };
    let rw = button(p, mx + 24, first_button_y, None, refresh_label, Some(Icon::Refresh), Btn::Secondary, PANEL, Some(Action::RefreshLibrary));
    let sx = if compact { mx + 24 } else { mx + 24 + rw + 12 };
    let second_button_y = if compact { first_button_y + BTN_H + 8 } else { first_button_y };
    if view.scanning {
        let progress = view
            .progress
            .map(|v| format!("{} / {} · {}", v.completed, v.total, v.source))
            .unwrap_or_else(|| "Analyse en préparation…".to_owned());
        let cw = button(p, sx, second_button_y, None, "Annuler la détection", Some(Icon::Close), Btn::Secondary, PANEL, Some(Action::CancelScan));
        if !compact {
            p.text_fit(sx + cw + 16, second_button_y, BTN_H, &progress, 12.0, W::Med, VIO_HI, (mx + mw - 24) - (sx + cw + 16));
        }
    } else {
        button(p, sx, second_button_y, None, scan_button_label, Some(Icon::Search), Btn::Primary, PANEL, Some(Action::ScanGames));
    }
    y += core_h + GAP;

    // ---- two-column cards
    let two = !lay.compact();
    let cw = if two { (mw - GAP) / 2 } else { mw };
    let h = 156;
    let (x1, y1, x2, y2) = if two { (mx, y, mx + cw + GAP, y) } else { (mx, y, mx, y + h + GAP) };

    panel(p, x1, y1, cw, h, 18);
    p.text(x1 + 24, y1 + 18, 24, "Bibliothèque locale", 16.0, W::Semi, TXT);
    p.text(x1 + 24, y1 + 52, 44, &view.game_count.to_string(), 34.0, W::Disp, VIO_HI);
    p.text(x1 + 24, y1 + 100, 20, &format!("{} enregistrés sur cet appareil", if view.game_count > 1 { "jeux" } else { "jeu" }), 12.5, W::Reg, MUT);
    p.text(x1 + 24, y1 + 122, 20, &format!("dont {} prêts à lancer", view.ready_count), 12.5, W::Reg, FAINT);

    panel(p, x2, y2, cw, h, 18);
    p.text(x2 + 24, y2 + 18, 24, "Stockage des médias", 16.0, W::Semi, TXT);
    if let Some(root) = view.media_root {
        p.icon(Icon::Folder, x2 + 24, y2 + 56, 18, VIO_HI, PANEL);
        let path = root.to_string_lossy();
        p.paragraph(x2 + 52, y2 + 54, cw - 76, 20, &path, 12.5, W::Reg, SOFT, 3);
        p.text(x2 + 24, y2 + 124, 20, "Jaquettes personnalisées conservées sur cet appareil", 12.0, W::Reg, FAINT);
    } else {
        p.text(x2 + 24, y2 + 56, 22, "Indisponible", 14.0, W::Semi, AMBER);
        p.text(x2 + 24, y2 + 84, 20, "Le stockage local n'a pas été initialisé.", 12.5, W::Reg, MUT);
    }
    y = if two { y1 + h + GAP } else { y2 + h + GAP };

    // ---- keyboard shortcuts
    let shortcuts = [
        ("Tab · Maj+Tab", "Parcourir les éléments"),
        ("Entrée", "Activer l'élément sélectionné"),
        ("Échap", "Annuler, fermer, puis quitter"),
        ("F11", "Activer ou quitter le plein écran"),
        ("Ctrl+V", "Coller dans un champ de texte"),
    ];
    let shortcut_row_h = if mw < 420 { 56 } else { 38 };
    let sh = 70 + shortcuts.len() as i32 * shortcut_row_h;
    panel(p, mx, y, mw, sh, 18);
    p.text(mx + 24, y + 18, 24, "Raccourcis clavier", 16.0, W::Semi, TXT);
    for (i, (keys, what)) in shortcuts.iter().enumerate() {
        let ry = y + 56 + i as i32 * shortcut_row_h;
        let kw = p.width(keys, 12.0, W::Semi).round() as i32 + 22;
        p.rrect(mx + 24, ry + 2, kw, 26, 8, PANEL_HI);
        p.rrect_line(mx + 24, ry + 2, kw, 26, 8, alpha(Color::WHITE, 28));
        p.text(mx + 35, ry + 2, 26, keys, 12.0, W::Semi, SOFT);
        if mw < 420 {
            p.text_fit(mx + 24, ry + 29, 24, what, 12.0, W::Reg, MUT, mw - 48);
        } else {
            p.text(mx + 24 + 150, ry, 36, what, 13.0, W::Reg, MUT);
        }
    }
    y += sh + GAP;

    // ---- privacy and about
    let ph = 132;
    panel(p, mx, y, mw, ph, 18);
    p.text(mx + 24, y + 18, 24, "Confidentialité et fonctionnement", 16.0, W::Semi, TXT);
    p.paragraph(
        mx + 24,
        y + 52,
        mw - 48,
        21,
        "La bibliothèque et les images personnalisées sont stockées localement. Aucun serveur Amethys payant n'est nécessaire pour utiliser ces fonctions. Les comptes et boutiques connectées arriveront avec leurs services dédiés.",
        13.0,
        W::Reg,
        MUT,
        4,
    );
    y += ph + GAP;

    let ah = 92;
    panel(p, mx, y, mw, ah, 18);
    p.icon(Icon::Gem, mx + 24, y + 22, 24, VIO, PANEL);
    p.text_fit(mx + 60, y + 16, 24, "Amethys Launcher 0.2 · version de développement", 14.0, W::Semi, TXT, mw - 84);
    p.text_fit(mx + 60, y + 44, 20, "Interface rendue par GhostEngine (CPU). Le backend de fenêtre est expérimental et temporaire.", 12.5, W::Reg, MUT, mw - 84);
    y + ah + 40
}

// ------------------------------------------------------------------ dialog
/// Confirmation dialog for removing a library entry, drawn at the current
/// scroll offset so it always appears in the visible part of the window.
pub fn remove_dialog(p: &mut Painter, lay: &Layout, game_name: &str, offset: i32) {
    // The sidebar is painted once at the top of the page; the content scrolls.
    p.fill(0, 0, lay.side_w, lay.h, Color::rgba(0, 0, 0, 150));
    p.fill(lay.side_w, offset, lay.w - lay.side_w, lay.h, Color::rgba(0, 0, 0, 150));
    let mw = 500.min(lay.w - lay.side_w - 24);
    let mh = 252;
    let mx = lay.side_w + (lay.w - lay.side_w - mw) / 2;
    let my = offset + (lay.h - mh) / 2;
    p.shadow(mx, my, mw, mh, 20, 28, 160);
    p.rrect(mx, my, mw, mh, 20, Color::rgb(0x17, 0x14, 0x21));
    p.rrect_line(mx, my, mw, mh, 20, alpha(Color::WHITE, 34));
    icon_badge(p, mx + 24, my + 24, 44, Icon::Trash, RED, RED_DARK);
    p.text(mx + 84, my + 24, 24, "Retirer de la bibliothèque ?", 17.0, W::Semi, TXT);
    let name = p.ellipsize(game_name, 13.5, W::Med, mw - 108);
    p.text(mx + 84, my + 50, 20, &name, 13.5, W::Med, VIO_HI);
    p.paragraph(
        mx + 24,
        my + 88,
        mw - 48,
        20,
        "Seule l'entrée Amethys est retirée. Les fichiers du jeu sur le disque ne sont pas supprimés.",
        13.0,
        W::Reg,
        MUT,
        3,
    );
    let by = my + mh - 24 - BTN_H;
    let rw = button_width(p, "Retirer", Some(Icon::Trash)) + 8;
    let cancel_w = 120;
    let dialog_bg = Color::rgb(0x17, 0x14, 0x21);
    let confirm = button(p, mx + mw - 24 - rw, by, Some(rw), "Retirer", Some(Icon::Trash), Btn::Danger, dialog_bg, None);
    let cancel = button(p, mx + mw - 24 - rw - 10 - cancel_w, by, Some(cancel_w), "Annuler", None, Btn::Ghost, dialog_bg, None);
    // These hits are page-space (they scroll with the page, which is frozen while the dialog is open).
    p.hit_with(mx + mw - 24 - confirm, by, confirm, BTN_H, 12, Action::ConfirmRemoveGame, false, Hover::Danger);
    p.hit_with(mx + mw - 24 - rw - 10 - cancel, by, cancel, BTN_H, 12, Action::CancelRemoveGame, false, Hover::Soft);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(platform: &str) -> IpcGameEntry {
        IpcGameEntry {
            id: platform.to_owned(),
            name: "Fixture".to_owned(),
            platform: platform.to_owned(),
            platform_id: None,
            catalogue_game_id: Some("3000".to_owned()),
            catalogue_metadata: None,
            install_path: ".".to_owned(),
            executable: None,
        }
    }

    fn named(name: &str, platform: &str) -> IpcGameEntry {
        let mut g = game(platform);
        g.name = name.to_owned();
        g.id = format!("{name}-{platform}");
        g
    }

    fn view<'a>(games: &'a [IpcGameEntry], statuses: &'a [GameStatus], filter: usize, sort: usize, query: &'a str) -> LibraryView<'a> {
        LibraryView {
            games,
            statuses,
            detected: &[],
            scanning: false,
            progress: None,
            scan_found: None,
            filter,
            sort,
            query,
            search_focused: false,
            core_error: None,
            limit: 48,
        }
    }

    #[test]
    fn platform_filters_are_unique_trimmed_and_sorted() {
        let games = [game("Steam"), game(" Epic Games "), game("steam"), game("GOG"), game(""), game("   ")];
        assert_eq!(library_platforms(&games), vec!["Epic Games".to_owned(), "GOG".to_owned(), "Steam".to_owned()]);
    }

    #[test]
    fn empty_library_has_no_platform_filters() {
        assert!(library_platforms(&[]).is_empty());
    }

    #[test]
    fn search_matches_multiple_terms_across_real_metadata() {
        let mut entry = game("Steam");
        entry.name = "Hollow Knight".to_owned();
        entry.install_path = "C:/Games/HollowKnight".to_owned();
        assert!(library_matches_query(&entry, "hollow steam"));
        assert!(library_matches_query(&entry, "C:/Games knight"));
        assert!(!library_matches_query(&entry, "hollow epic"));
        assert!(library_matches_query(&entry, "  "));
    }

    #[test]
    fn visible_games_filter_by_platform_then_sort_by_name() {
        let games = [named("Zelda", "GOG"), named("Alpha", "Steam"), named("Beta", "Steam")];
        let statuses: Vec<GameStatus> = games.iter().map(|_| crate::status::classify(true, true, None)).collect();
        // platforms sorted: GOG (1), Steam (2)
        assert_eq!(visible_games(&view(&games, &statuses, 2, 0, "")), vec![1, 2]);
        assert_eq!(visible_games(&view(&games, &statuses, 0, 0, "")), vec![1, 2, 0]);
        assert_eq!(visible_games(&view(&games, &statuses, 0, 0, "zel")), vec![0]);
    }

    #[test]
    fn out_of_range_filter_falls_back_to_all_games() {
        let games = [named("A", "Steam"), named("B", "GOG")];
        let statuses: Vec<GameStatus> = games.iter().map(|_| crate::status::classify(true, true, None)).collect();
        assert_eq!(visible_games(&view(&games, &statuses, 99, 0, "")).len(), 2);
    }

    #[test]
    fn status_sort_puts_launchable_games_first() {
        let games = [named("A", "Steam"), named("B", "Steam"), named("C", "Steam")];
        let statuses = vec![
            crate::status::classify(false, true, Some("x".into())),
            crate::status::classify(true, true, None),
            crate::status::classify(true, false, Some("y".into())),
        ];
        assert_eq!(visible_games(&view(&games, &statuses, 0, 2, "")), vec![1, 2, 0]);
    }

    #[test]
    fn card_columns_adapt_to_window_width() {
        assert_eq!(Layout::new(1600, 900).columns(), 6);
        assert!(Layout::new(1100, 700).columns() < Layout::new(1600, 900).columns());
        assert!(Layout::new(960, 600).columns() >= 2);
        let wide = Layout::new(1600, 900);
        assert!(wide.card_width() * wide.columns() + GAP * (wide.columns() - 1) <= wide.mw);

        let compact = Layout::new(720, 520);
        assert!(compact.compact());
        assert_eq!(compact.side_w, 72);
        assert_eq!(compact.mx, 92);
        assert_eq!(compact.mw, 588);
        assert!(compact.card_width() >= MIN_CARD_W);
        let tiny = Layout::new(360, 420);
        assert_eq!(tiny.columns(), 1);
        assert!(tiny.mw < compact.mw);
    }

    #[test]
    fn plural_follows_french_rules() {
        assert_eq!(plural(0, "jeu", "jeux"), "0 jeu");
        assert_eq!(plural(1, "jeu", "jeux"), "1 jeu");
        assert_eq!(plural(2, "jeu", "jeux"), "2 jeux");
    }
}
