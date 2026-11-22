//! Interactive Amethyst Launcher test app built on GhostEngine's own UI pieces:
//! `ScrollViewport` (scrolling), `UiInteraction` (hover / press / click dispatch),
//! and the native window surface. Pages are pre-rendered with `pages::*` and
//! blitted every frame; hover and the scrollbar are drawn on top.

use crate::covers::CoverStore;
use amethys_ipc::IpcGameEntry;
use crate::gfx::{self, Action, Fonts, Hit, Page, Painter, State, BG, W};
use crate::pages::{self, Active, PageFn, SIDE_W};
use ghostengine::InputState;
use std::{collections::VecDeque, time::Instant};
use ghostengine::{
    draw_rounded_rect_aa, fill_rounded_rect_aa, Color, Framebuffer, LayoutDirection, Rect,
    ScrollViewport, UiInteraction,
};

const PANEL_W: i32 = 520;
const PANEL_H: i32 = 520;
/// Tallest page the pre-render buffer can hold.
const MAX_PAGE_H: u32 = 3200;

#[derive(Clone, Copy)]
struct Bench {
    page_avg: f64,
    page_min: f64,
    page_max: f64,
    page_n: u32,
    frame_avg: f64,
    frame_n: u32,
}

pub const WIN_W: u32 = 1600;
pub const WIN_H: u32 = 900;

/// Produces a stable path key for comparing discovered and registered installs.
fn normalized_install_path(path: &std::path::Path) -> String {
    let normalized = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    normalized.to_string_lossy()
        .replace(char::from(92), "/")
        .trim_end_matches('/')
        .to_ascii_lowercase()
}

pub struct Output {
    pub keep_running: bool,
}

pub struct App {
    fonts: &'static Fonts,
    covers: &'static CoverStore,
    pub page: Page,
    pub st: State,
    library_sort: usize,
    pub games: Vec<IpcGameEntry>,
    pub detected_games: Vec<amethys_discovery::DetectedGame>,
    scan_receiver: Option<std::sync::mpsc::Receiver<Vec<amethys_discovery::DetectedGame>>>,
    scan_in_progress: bool,
    pub core_error: Option<String>,
    pub notice: Option<String>,
    pub selected_game: Option<usize>,
    pending_remove: Option<usize>,
    search_query: String,
    search_focused: bool,
    executable_draft: String,
    editing_executable: bool,
    /// Whole page pre-rendered once (sidebar occupies the first WIN_H rows).
    content: Framebuffer,
    hits: Vec<Hit>,
    pub scroll: ScrollViewport,
    inter: UiInteraction,
    pub stats_open: bool,
    frame_ms: VecDeque<f64>,
    work_ms: f64,
    page_ms: f64,
    last_frame: Option<Instant>,
    bench: Option<Bench>,
}

impl App {
    pub fn new(fonts: &'static Fonts, covers: &'static CoverStore, library: Result<Vec<IpcGameEntry>, String>) -> Self {
        let blank = || Framebuffer::new(WIN_W, WIN_H, BG).expect("framebuffer");
        let (games, core_error) = match library { Ok(games) => (games, None), Err(error) => (Vec::new(), Some(error)) };
        let mut app = Self {
            fonts,
            covers,
            page: Page::Library,
            st: State::default(),
            library_sort: 0,
            games,
            detected_games: Vec::new(),
            scan_receiver: None,
            scan_in_progress: false,
            core_error,
            notice: None,
            selected_game: None,
            pending_remove: None,
            search_query: String::new(),
            search_focused: false,
            executable_draft: String::new(),
            editing_executable: false,
            content: blank(),
            hits: Vec::new(),
            scroll: ScrollViewport::new(Rect::new(SIDE_W as u32, 0, WIN_W - SIDE_W as u32, WIN_H), LayoutDirection::Vertical, WIN_H),
            inter: UiInteraction::new(),
            stats_open: false,
            frame_ms: VecDeque::new(),
            work_ms: 0.0,
            page_ms: 0.0,
            last_frame: None,
            bench: None,
        };
        app.render(true);
        app
    }

    /// Re-draws the current page (sidebar, content, hit regions).
    fn render(&mut self, reset_scroll: bool) {
        let (active, page): (Active, PageFn) = match self.page {
            Page::Store => (Active::Store, pages::store),
            Page::Library => (Active::Library, pages::library),
            Page::Product => (Active::Store, pages::product),
            Page::Settings => (Active::Settings, pages::settings),
        };
        let t0 = Instant::now();
        let vh = WIN_H as i32;
        let mut p = Painter::new(WIN_W, MAX_PAGE_H, self.fonts).with_covers(self.covers);
        pages::sidebar(&mut p, active, vh);
        let h = (if self.page == Page::Library {
            pages::library_live(
                &mut p,
                vh,
                &self.games,
                &self.detected_games,
                self.scan_in_progress,
                self.st.lib_filter,
                self.library_sort,
                &self.search_query,
                self.search_focused,
                self.core_error.as_deref(),
                self.notice.as_deref(),
            )
        } else if self.page == Page::Product {
            pages::game_detail_live(
                &mut p,
                self.games.get(self.selected_game.unwrap_or(usize::MAX)),
                self.selected_game.unwrap_or(usize::MAX),
                &self.executable_draft,
                self.editing_executable,
                self.notice.as_deref(),
            )
        } else if self.page == Page::Store {
            pages::catalogue_unavailable(&mut p, vh)
        } else if self.page == Page::Settings {
            pages::settings_local(&mut p, vh, self.core_error.as_deref())
        } else {
            page(&mut p, vh, &self.st)
        }).clamp(vh, MAX_PAGE_H as i32);
        if let Some(index) = self.pending_remove {
            // A modal must be the only interactive layer while it is open.
            p.hits.clear();
            let game_name = self.games.get(index).map(|game| game.name.as_str()).unwrap_or("ce jeu");
            let title = format!("Retirer « {game_name} » ?");
            let mw = 520;
            let mh = 210;
            let mx = (WIN_W as i32 - mw) / 2;
            let my = (WIN_H as i32 - mh) / 2;
            p.fill(0, 0, WIN_W as i32, WIN_H as i32, Color::rgba(0, 0, 0, 150));
            p.rrect(mx, my, mw, mh, 18, BG);
            p.rrect_line(mx, my, mw, mh, 18, Color::rgba(255, 255, 255, 35));
            p.text(mx + 26, my + 22, 32, "Confirmer le retrait", 22.0, W::Semi, Color::WHITE);
            p.paragraph(mx + 26, my + 64, mw - 52, 20, &title, 15.0, W::Med, Color::WHITE, 2);
            p.paragraph(mx + 26, my + 100, mw - 52, 18, "Cette action retire uniquement l'entrée de la bibliothèque. Les fichiers du jeu sur le disque ne seront pas supprimés.", 12.0, W::Reg, Color::rgba(210, 205, 220, 255), 3);
            p.rrect(mx + mw - 250, my + 158, 104, 34, 9, PANEL);
            p.rrect_line(mx + mw - 250, my + 158, 104, 34, 9, LINE);
            p.text(mx + mw - 232, my + 158, 34, "Annuler", 13.0, W::Med, TXT);
            p.hit(mx + mw - 250, my + 158, 104, 34, 9, Action::CancelRemoveGame, true);
            p.rrect(mx + mw - 134, my + 158, 108, 34, 9, VIO);
            p.text(mx + mw - 116, my + 158, 34, "Retirer", 13.0, W::Semi, BG);
            p.hit(mx + mw - 134, my + 158, 108, 34, 9, Action::ConfirmRemoveGame, true);
        }
        self.hits = std::mem::take(&mut p.hits);
        self.content = p.fb;
        let old = self.scroll.offset();
        self.scroll.set_content_extent(h as u32);
        self.scroll.set_offset(if reset_scroll { 0 } else { old });
        self.inter.clear();
        self.page_ms = t0.elapsed().as_secs_f64() * 1000.0;
    }

    fn apply(&mut self, action: Action) {
        if !matches!(action, Action::FocusLibrarySearch) {
            self.search_focused = false;
        }
        if !matches!(action, Action::EditExecutable | Action::SaveExecutable | Action::CancelExecutableEdit) {
            self.editing_executable = false;
        }
        match action {
            Action::Go(page) => {
                self.page = page;
                if page == Page::Library { self.selected_game = None; }
                self.render(true);
            }
            Action::SelectGame(index) => {
                if let Some(game) = self.games.get(index) {
                    self.executable_draft = game.executable.clone().unwrap_or_default();
                    self.selected_game = Some(index);
                    self.page = Page::Product;
                    self.render(true);
                }
            }
            Action::StoreFilter(i) => {
                self.st.store_filter = i;
                self.render(false);
            }
            Action::LibFilter(i) => {
                self.st.lib_filter = i;
                self.render(false);
            }
            Action::CycleLibrarySort => {
                self.library_sort = (self.library_sort + 1) % 3;
                self.render(false);
            }
            Action::FocusLibrarySearch => {
                self.search_focused = true;
                self.render(false);
            }
            Action::EditExecutable => {
                self.editing_executable = true;
                self.search_focused = false;
                self.render(false);
            }
            Action::SaveExecutable => {
                let result = match self.selected_game.and_then(|index| self.games.get(index).map(|game| (index, game))) {
                    Some((_, _)) if self.executable_draft.trim().is_empty() => {
                        Err("Indique le chemin de l'exécutable avant de l'enregistrer.".to_owned())
                    }
                    Some((index, game)) => crate::core_client::set_game_executable(
                        &game.id,
                        self.executable_draft.trim(),
                    )
                    .map(|updated| (index, updated)),
                    None => Err("Aucun jeu n'est sélectionné.".to_owned()),
                };
                match result {
                    Ok((index, updated)) => {
                        self.games[index] = updated;
                        self.notice = Some("Chemin de l'exécutable enregistré dans Amethys Core.".to_owned());
                        self.executable_draft = self.games[index].executable.clone().unwrap_or_default();
                        self.editing_executable = false;
                    }
                    Err(error) => {
                        self.notice = Some(format!("Impossible d'enregistrer l'exécutable : {error}"));
                        self.editing_executable = true;
                    }
                }
                self.render(false);
            }
            Action::CancelExecutableEdit => {
                self.executable_draft = self.selected_game
                    .and_then(|index| self.games.get(index))
                    .and_then(|game| game.executable.clone())
                    .unwrap_or_default();
                self.editing_executable = false;
                self.render(false);
            }
            Action::ScanGames => {
                if !self.scan_in_progress {
                    let (sender, receiver) = std::sync::mpsc::channel();
                    std::thread::spawn(move || {
                        let detected = amethys_discovery::discover_all_games();
                        let _ = sender.send(detected);
                    });
                    self.scan_receiver = Some(receiver);
                    self.scan_in_progress = true;
                    self.detected_games.clear();
                    self.notice = Some("Recherche des installations locales en cours…".to_owned());
                    self.render(false);
                }
            }
            Action::ImportDetectedGame(index) => {
                let result = self.detected_games.get(index)
                    .ok_or_else(|| "Cette installation détectée n'est plus disponible.".to_owned())
                    .and_then(crate::core_client::add_detected_game);
                match result {
                    Ok(game) => {
                        let name = game.name.clone();
                        match crate::core_client::list_games() {
                            Ok(games) => { self.games = games; self.core_error = None; }
                            Err(error) => self.core_error = Some(error),
                        }
                        self.detected_games.remove(index);
                        self.notice = Some(format!("{name} a été ajouté à votre bibliothèque."));
                    }
                    Err(error) => self.notice = Some(format!("Import impossible : {error}")),
                }
                self.render(false);
            }
            Action::RefreshLibrary => {
                self.st.lib_filter = 0;
                match crate::core_client::list_games_or_start_agent() {
                    Ok(games) => { self.games = games; self.core_error = None; self.notice = Some("Bibliothèque actualisée.".to_owned()); }
                    Err(error) => { self.core_error = Some(error); self.notice = None; }
                }
                self.render(false);
            }
            Action::LaunchGame(index) => {
                let result = match self.games.get(index) {
                    Some(game) => crate::core_client::launch_game(game).map(|pid| format!("Démarrage demandé : {} (processus {}).", game.name, pid)),
                    None => Err("Ce jeu n'existe plus dans la bibliothèque chargée.".to_owned()),
                };
                self.notice = Some(match result { Ok(message) => message, Err(error) => format!("Échec du lancement : {error}") });
                self.render(false);
            }
            Action::RemoveGame(index) => {
                if self.games.get(index).is_some() {
                    self.pending_remove = Some(index);
                    self.render(false);
                } else {
                    self.notice = Some("Ce jeu n'existe plus dans la bibliothèque chargée.".to_owned());
                    self.render(false);
                }
            }
            Action::CancelRemoveGame => {
                self.pending_remove = None;
                self.render(false);
            }
            Action::ConfirmRemoveGame => {
                let result = match self.pending_remove.take().and_then(|index| self.games.get(index).map(|game| game.id.clone())) {
                    Some(id) => crate::core_client::remove_game(&id),
                    None => Err("Ce jeu n'existe plus dans la bibliothèque chargée.".to_owned()),
                };
                match result {
                    Ok(true) => {
                        self.notice = Some("Jeu retiré de la bibliothèque. Ses fichiers n'ont pas été supprimés.".to_owned());
                        self.st.lib_filter = 0;
                        self.page = Page::Library;
                        self.selected_game = None;
                        match crate::core_client::list_games() {
                            Ok(games) => { self.games = games; self.core_error = None; }
                            Err(error) => self.core_error = Some(error),
                        }
                    }
                    Ok(false) => self.notice = Some("Le jeu n'était déjà plus présent dans la bibliothèque.".to_owned()),
                    Err(error) => self.notice = Some(format!("Impossible de retirer le jeu : {error}")),
                }
                self.render(false);
            }
        }
    }

    fn blit(&self, fb: &mut Framebuffer) {
        let (logical_w, target_w) = (WIN_W as usize, fb.width() as usize);
        if target_w < logical_w {
            return;
        }

        let rows = (fb.height() as usize).min(WIN_H as usize);
        let offset = self.scroll.offset() as usize;
        let sidebar_w = SIDE_W as usize;
        let source = self.content.pixels();
        let target = fb.pixels_mut();

        // Borrow the destination pixel buffer only once per frame. The previous
        // implementation repeatedly requested mutable access inside the row loop.
        for y in 0..rows {
            let dst_start = y * target_w;
            let src_start = y * logical_w;
            target[dst_start..dst_start + sidebar_w]
                .copy_from_slice(&source[src_start..src_start + sidebar_w]);

            let content_src_start = (y + offset) * logical_w + sidebar_w;
            let content_dst_start = dst_start + sidebar_w;
            target[content_dst_start..dst_start + logical_w]
                .copy_from_slice(&source[content_src_start..(y + offset + 1) * logical_w]);
        }
    }

    /// Screen-space, viewport-clipped bounds of every hit region.
    fn screen_bounds(&self) -> (Vec<Rect>, Vec<bool>) {
        let off = self.scroll.offset() as i32;
        let (mut bounds, mut enabled) = (Vec::new(), Vec::new());
        for h in &self.hits {
            let (x0, y0) = (h.rect.x as i32, h.rect.y as i32 - if h.fixed { 0 } else { off });
            let (x1, y1) = (x0 + h.rect.width as i32, y0 + h.rect.height as i32);
            let (cx0, cx1) = if h.fixed { (0, WIN_W as i32) } else { (SIDE_W, WIN_W as i32) };
            let (a, b, c, d) = (x0.max(cx0), y0.max(0), x1.min(cx1), y1.min(WIN_H as i32));
            if c > a && d > b {
                bounds.push(Rect::new(a as u32, b as u32, (c - a) as u32, (d - b) as u32));
                enabled.push(true);
            } else {
                bounds.push(Rect::new(0, 0, 0, 0));
                enabled.push(false);
            }
        }
        (bounds, enabled)
    }

    /// Pages, hover, scrollbar and search field. `mask` hides the pointer from the page
    /// (used while the pointer is over the stats overlay).
    fn draw_scene(&mut self, fb: &mut Framebuffer, real: &InputState, mask: bool) {
        let blank = InputState::default();
        let input = if mask { &blank } else { real };
        let main = Rect::new(SIDE_W as u32, 0, WIN_W - SIDE_W as u32, WIN_H);
        let pointer = input.mouse_position;
        let in_main = pointer.is_some_and(|p| main.contains(p));
        if input.scroll_delta_y != 0 && in_main {
            self.scroll.scroll_by(input.scroll_delta_y);
        }

        // hover / press / click through GhostEngine's interaction model
        let (bounds, enabled) = self.screen_bounds();
        let events = self.inter.update(pointer, input.left_mouse_pressed, input.left_mouse_released, &enabled, &bounds);
        let hover = self.inter.hovered().map(|i| (bounds[i], self.hits[i].radius, self.inter.is_pressed(i)));
        let clicked = events.clicked.map(|i| self.hits[i].action);
        if let Some(action) = clicked {
            self.apply(action);
        }

        self.blit(fb);

        if clicked.is_none() {
            if let Some((r, radius, pressed)) = hover {
                fill_rounded_rect_aa(fb, r, radius, Color::rgba(185, 162, 255, if pressed { 56 } else { 26 }));
                draw_rounded_rect_aa(fb, r, radius, Color::rgba(185, 162, 255, 190));
            }
        }

        // overlay scrollbar, visible only while the pointer is over the content
        if self.scroll.is_scrollable() && in_main {
            let total = (self.scroll.max_offset() + WIN_H) as f32;
            let thumb = ((WIN_H as f32 * WIN_H as f32 / total) as u32).max(48);
            let travel = WIN_H - thumb - 8;
            let y = 4 + (self.scroll.offset() as f32 / self.scroll.max_offset() as f32 * travel as f32) as u32;
            fill_rounded_rect_aa(fb, Rect::new(WIN_W - 10, y, 6, thumb), 3, Color::rgba(185, 162, 255, 120));
        }
    }

    // ------------------------------------------------------------ stats overlay
    fn chip_rect(&self) -> Rect {
        Rect::new(WIN_W - 12 - 250, WIN_H - 12 - 34, 250, 34)
    }

    fn panel_rect(&self) -> Rect {
        Rect::new(WIN_W - 12 - PANEL_W as u32, WIN_H - 12 - 34 - 8 - PANEL_H as u32, PANEL_W as u32, PANEL_H as u32)
    }

    fn bench_button(&self) -> Rect {
        let p = self.panel_rect();
        Rect::new(p.x + 20, p.y + PANEL_H as u32 - 20 - 44 - 44, 200, 44)
    }

    fn avg_frame_ms(&self) -> f64 {
        if self.frame_ms.is_empty() {
            0.0
        } else {
            self.frame_ms.iter().sum::<f64>() / self.frame_ms.len() as f64
        }
    }

    /// Measures page rendering and full frames on this machine (blocks for ~1-3 s).
    fn run_bench(&mut self, fb: &mut Framebuffer) {
        let n = 10;
        let mut times = Vec::new();
        for _ in 0..n {
            let t = Instant::now();
            self.render(false);
            times.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        let idle = InputState::default();
        let frames = 200;
        let t = Instant::now();
        for _ in 0..frames {
            self.draw_scene(fb, &idle, false);
        }
        let frame_avg = t.elapsed().as_secs_f64() * 1000.0 / frames as f64;
        self.bench = Some(Bench {
            page_avg: times.iter().sum::<f64>() / n as f64,
            page_min: times.iter().cloned().fold(f64::MAX, f64::min),
            page_max: times.iter().cloned().fold(0.0, f64::max),
            page_n: n,
            frame_avg,
            frame_n: frames,
        });
    }

    fn draw_overlay(&mut self, fb: &mut Framebuffer) {
        let avg = self.avg_frame_ms();
        let fps = if avg > 0.0 { 1000.0 / avg } else { 0.0 };
        let mut c = Painter::new(250, 34, self.fonts);
        c.rrect(0, 0, 250, 34, 17, gfx::ACT);
        c.rrect_line(0, 0, 250, 34, 17, gfx::VIO);
        c.circle(18, 17, 5, gfx::GREEN);
        c.text(32, 0, 34, &format!("GhostEngine · {:.1} ms · {:.0} img/s", self.work_ms, fps), 13.0, W::Semi, gfx::ACT_TXT);
        self.copy_to(fb, &c.fb, self.chip_rect(), 17);
        if !self.stats_open {
            return;
        }
        let mut p = Painter::new(PANEL_W as u32, PANEL_H as u32, self.fonts);
        p.fill(0, 0, PANEL_W, PANEL_H, BG);
        p.rrect(0, 0, PANEL_W, PANEL_H, 18, gfx::PANEL);
        p.rrect_line(0, 0, PANEL_W, PANEL_H, 18, gfx::VIO);
        p.text(20, 16, 28, "GhostEngine — mesures en direct", 17.0, W::Disp, gfx::TXT);
        let stats = self.covers.stats();
        let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
        let rows = [
            ("Cadence (moy. 60 frames)", format!("{avg:.2} ms · {fps:.0} img/s")),
            ("Travail de la dernière frame", format!("{:.2} ms", self.work_ms)),
            ("Dernier rendu de page", format!("{:.0} ms", self.page_ms)),
            ("Framebuffer", format!("{WIN_W}×{WIN_H} RGBA · {:.1} Mo", (WIN_W * WIN_H * 4) as f64 / 1_048_576.0)),
            ("Images décodées", format!("{} fichiers · {:.0} ms", self.covers.files, self.covers.decode_ms)),
            ("Cache de textures", format!("{} entrées · {:.1} Mo", stats.entries, stats.used_bytes as f64 / 1_048_576.0)),
            ("Threads CPU disponibles", format!("{threads}")),
        ];
        let mut y = 56;
        for (k, v) in rows.iter() {
            p.fill(20, y, PANEL_W - 40, 1, gfx::LINE);
            p.text(20, y + 1, 26, k, 13.0, W::Reg, gfx::MUT);
            p.text_right(PANEL_W - 20, y + 1, 26, v, 13.0, W::Med, gfx::TXT);
            y += 28;
        }
        y += 10;
        p.text(20, y, 18, "FONCTIONS DU MOTEUR UTILISÉES ICI", 11.0, W::Semi, gfx::MUT);
        y += 22;
        for line in [
            "Rectangles arrondis anti-aliasés, cercles, polygones",
            "Dégradés linéaires et fonds à alpha",
            "Décodage PNG/JPEG + TextureCache + mipmaps",
            "Texte OpenType (shaping) en 5 graisses",
            "ScrollViewport, UiInteraction, UiComponent::text_input",
        ] {
            p.text(20, y, 18, line, 12.0, W::Reg, gfx::SOFT);
            y += 19;
        }
        let b = self.bench_button();
        let by = (b.y - self.panel_rect().y) as i32;
        p.rrect(20, by, 200, 44, 12, gfx::VIO);
        let label = "Lancer le benchmark";
        let tw = p.width(label, 14.0, W::Semi).round() as i32;
        p.text(20 + (200 - tw) / 2, by, 44, label, 14.0, W::Semi, gfx::VIO_DARK);
        let (l1, l2) = match self.bench {
            Some(b) => (
                format!("Rendu de page : {:.0} ms (min {:.0}, max {:.0}, {} mesures)", b.page_avg, b.page_min, b.page_max, b.page_n),
                format!("Frame complète : {:.2} ms soit {:.0} img/s ({} frames)", b.frame_avg, 1000.0 / b.frame_avg, b.frame_n),
            ),
            None => ("Mesure le rendu sur cette machine (≈ 1 à 3 s).".to_string(), String::new()),
        };
        p.text(20, by + 52, 18, &l1, 12.0, W::Reg, gfx::SOFT);
        p.text(20, by + 70, 18, &l2, 12.0, W::Reg, gfx::SOFT);
        self.copy_to(fb, &p.fb, self.panel_rect(), 18);
    }

    /// Copies `src` onto the framebuffer with rounded corners (pixels outside the arc are skipped).
    fn copy_to(&self, fb: &mut Framebuffer, src: &Framebuffer, at: Rect, radius: usize) {
        let (sw, sh, fw) = (src.width() as usize, src.height() as usize, fb.width() as usize);
        for row in 0..sh {
            let (dy, dx) = (at.y as usize + row, at.x as usize);
            if dy >= fb.height() as usize || dx + sw > fw {
                break;
            }
            let dist = if row < radius { radius - row } else if row + radius >= sh { row + radius + 1 - sh } else { 0 };
            let inset = if dist == 0 { 0 } else { radius - ((radius * radius - dist * dist.min(radius)) as f32).sqrt() as usize };
            let (l, r) = (inset.min(sw / 2), sw - inset.min(sw / 2));
            fb.pixels_mut()[dy * fw + dx + l..dy * fw + dx + r].copy_from_slice(&src.pixels()[row * sw + l..row * sw + r]);
        }
    }

    /// Runs one frame: input -> state -> pixels. Returns whether to keep running.
    fn handle_search_input(&mut self, input: &InputState) {
        if !self.search_focused && !self.editing_executable {
            return;
        }
        let mut changed = false;
        let target = if self.search_focused {
            &mut self.search_query
        } else {
            &mut self.executable_draft
        };
        if input.backspace_pressed {
            changed |= target.pop().is_some();
        }
        if input.paste_pressed {
            if let Some(text) = input.clipboard_text.as_deref() {
                if !text.is_empty() {
                    target.push_str(text);
                    changed = true;
                }
            }
        }
        if !input.text_events.is_empty() {
            for event in &input.text_events {
                match event {
                    ghostengine::TextInputEvent::Insert(text)
                    | ghostengine::TextInputEvent::CompositionCommit(text) => {
                        if !text.is_empty() {
                            target.push_str(text);
                            changed = true;
                        }
                    }
                    ghostengine::TextInputEvent::CompositionUpdate(_)
                    | ghostengine::TextInputEvent::CompositionCancel => {}
                }
            }
        } else {
            for character in &input.pressed_characters {
                if !character.is_control() {
                    target.push(*character);
                    changed = true;
                }
            }
        }
        if input.enter_pressed && self.search_focused {
            self.search_focused = false;
        }
        if changed || (input.enter_pressed && self.search_focused == false) {
            self.render(false);
        }
    }

    pub fn frame(&mut self, fb: &mut Framebuffer, input: &InputState) -> Output {
        let start = Instant::now();
        let scan_result = self.scan_receiver.as_ref().map(|receiver| receiver.try_recv());
        match scan_result {
            Some(Ok(mut detected)) => {
                self.scan_receiver = None;
                self.scan_in_progress = false;
                // Seed identity sets from the saved library, then deduplicate the
                // discovery result itself. Paths are canonicalized when possible so
                // aliases such as "..", symlinks and Windows case differences do
                // not create duplicate import suggestions.
                let mut known_platform_ids = std::collections::HashSet::new();
                let mut known_install_paths = std::collections::HashSet::new();
                for game in &self.games {
                    let platform = game.platform.to_ascii_lowercase();
                    if let Some(id) = game.platform_id.as_deref() {
                        known_platform_ids.insert((platform.clone(), id.to_ascii_lowercase()));
                    }
                    known_install_paths.insert(normalized_install_path(&game.install_path));
                }
                detected.retain(|candidate| {
                    let platform = candidate.platform.to_ascii_lowercase();
                    let platform_id = candidate.platform_id.as_deref()
                        .map(str::to_ascii_lowercase);
                    let install_path = normalized_install_path(&candidate.install_path);
                    let known_id = platform_id.as_ref().is_some_and(|id| {
                        known_platform_ids.contains(&(platform.clone(), id.clone()))
                    });
                    let known_path = known_install_paths.contains(&install_path);
                    if known_id || known_path {
                        return false;
                    }
                    if let Some(id) = platform_id {
                        known_platform_ids.insert((platform, id));
                    }
                    known_install_paths.insert(install_path);
                    true
                });
                let count = detected.len();
                self.detected_games = detected;
                self.notice = Some(format!("{count} installation(s) détectée(s) à importer."));
                self.render(false);
            }
            Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) => {
                self.scan_receiver = None;
                self.scan_in_progress = false;
                self.notice = Some("La détection des jeux s'est interrompue de manière inattendue.".to_owned());
                self.render(false);
            }
            Some(Err(std::sync::mpsc::TryRecvError::Empty)) | None => {}
        }
        if let Some(last) = self.last_frame {
            self.frame_ms.push_back(start.duration_since(last).as_secs_f64() * 1000.0);
            if self.frame_ms.len() > 60 {
                self.frame_ms.pop_front();
            }
        }
        self.last_frame = Some(start);
        if input.escape_down {
            if self.pending_remove.is_some() {
                self.pending_remove = None;
                self.render(false);
            } else if self.search_focused || self.editing_executable {
                self.search_focused = false;
                self.editing_executable = false;
                self.render(false);
            } else if self.stats_open {
                self.stats_open = false;
            } else {
                return Output { keep_running: false };
            }
        }
        self.handle_search_input(input);
        let ptr = input.mouse_position;
        let over_chip = ptr.is_some_and(|p| self.chip_rect().contains(p));
        let over_panel = self.stats_open && ptr.is_some_and(|p| self.panel_rect().contains(p));
        let mut bench_now = false;
        if input.left_mouse_pressed {
            if over_chip {
                self.stats_open = !self.stats_open;
            } else if over_panel && ptr.is_some_and(|p| self.bench_button().contains(p)) {
                bench_now = true;
            }
        }
        self.draw_scene(fb, input, over_chip || over_panel);
        if bench_now {
            self.run_bench(fb);
            self.draw_scene(fb, &InputState::default(), false);
        }
        self.draw_overlay(fb);
        if !bench_now {
            self.work_ms = start.elapsed().as_secs_f64() * 1000.0;
        }
        Output { keep_running: true }
    }

}
