//! Interactive Amethys Launcher application built on GhostEngine's own UI pieces:
//! `ScrollViewport` (scrolling), `UiInteraction` (hover / press / click dispatch)
//! and the native window surface.
//!
//! Pages are pre-rendered once per state change with `pages::*` and blitted
//! every frame. Everything that moves (hover fades, page transitions, smooth
//! scrolling, toasts) is composited on top of that cached image, so animation
//! stays cheap. The window is resizable: the layout follows the real size.
//!
//! Every value shown comes from Amethys Core, the local filesystem or the
//! discovery service. Nothing is simulated.

use crate::covers::CoverStore;
use crate::gfx::{self, alpha, fb_fill, fb_fill_rrect, fb_stroke_rrect, Action, Fonts, Hit, Hover, Icon, Page, Painter, State, W};
use crate::pages::{self, Active, DetailView, Layout, LibraryView, SettingsView, SidebarInfo};
use crate::status::{self, GameStatus};
use amethys_ipc::{IpcCatalogGameDetail, IpcCatalogSearchEntry, IpcGameEntry};
use ghostengine::{
    Color, FrameOutcome, Framebuffer, InputState, LayoutDirection, Point, Rect, ScrollViewport,
    UiInteraction,
};
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

enum ScanMessage {
    Progress(amethys_discovery::DiscoveryProgress),
    Finished(amethys_discovery::DiscoveryOutcome),
}

enum CoreMutationTask {
    Import {
        detected: amethys_discovery::DetectedGame,
        context: (String, String, std::path::PathBuf),
    },
    Remove { id: String },
    SetExecutable { id: String, executable: String },
}

enum StoreTaskResult {
    Search(Result<Vec<IpcCatalogSearchEntry>, String>),
    Detail(Result<Box<IpcCatalogGameDetail>, String>),
    Synchronized(Result<(String, u64), String>),
}

enum CoreMutationResult {
    Imported {
        context: (String, String, std::path::PathBuf),
        result: Result<IpcGameEntry, String>,
    },
    Removed(Result<bool, String>),
    ExecutableUpdated {
        id: String,
        result: Result<IpcGameEntry, String>,
    },
}

/// Default window size (the window itself can be resized freely).
pub const WIN_W: u32 = 1440;
pub const WIN_H: u32 = 900;
/// Smallest window the layout is designed for.
pub const MIN_W: u32 = 360;

const PANEL_W: i32 = 520;
const PANEL_H: i32 = 500;
/// Cards added each time "Afficher plus" is used.
const PAGE_SIZE: usize = 48;
const DESKTOP_PREFERENCES_FILE: &str = "launcher-preferences-v1";
/// Target frame time while something is animating (about 60 images/s).
const FRAME_BUDGET: Duration = Duration::from_millis(16);
const PAGE_FADE_S: f32 = 0.22;
const HOVER_IN_S: f32 = 0.12;
const HOVER_OUT_S: f32 = 0.22;
const SCROLL_RATE: f32 = 16.0;
fn fit_stats_panel_scale(win_w: u32, win_h: u32, layout_scale: f32) -> f32 {
    let width_scale = win_w.saturating_sub(20) as f32 / PANEL_W as f32;
    let height_scale = win_h.saturating_sub(62) as f32 / PANEL_H as f32;
    layout_scale.min(width_scale).min(height_scale).max(0.1)
}


#[derive(Clone, Copy)]
struct Bench {
    page_avg: f64,
    page_min: f64,
    page_max: f64,
    page_n: u32,
    frame_avg: f64,
    frame_n: u32,
}

// ----------------------------------------------------------------- animation
fn ease_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// Moves `level` toward `target` at a constant speed (`seconds` for a full 0..1 sweep).
fn step_level(level: f32, target: f32, dt: f32, seconds: f32) -> f32 {
    let step = dt / seconds.max(0.001);
    if level < target {
        (level + step).min(target)
    } else {
        (level - step).max(target)
    }
}

/// Opacity of a toast `age` seconds after it appeared: quick fade in, slow fade out.
fn toast_opacity(age: f32, life: f32) -> f32 {
    let fade_in = (age / 0.18).clamp(0.0, 1.0);
    let fade_out = ((life - age) / 0.30).clamp(0.0, 1.0);
    fade_in.min(fade_out)
}

/// Computes the next actionable hit index for keyboard navigation.
fn focus_index_for_action(hits: &[Hit], action: Action) -> Option<usize> {
    hits.iter().position(|hit| hit.action == action)
}

/// Returns the next focusable hit, wrapping at either end of the page.
fn next_focus_index(current: Option<usize>, count: usize, backwards: bool) -> Option<usize> {
    if count == 0 {
        return None;
    }
    Some(match (current, backwards) {
        (None, false) => 0,
        (None, true) => count - 1,
        (Some(index), false) => (index + 1) % count,
        (Some(index), true) => (index + count - 1) % count,
    })
}

/// Keeps Tab navigation inside the destructive confirmation while it is open.
fn next_modal_focus_index(hits: &[Hit], current: Option<usize>, backwards: bool) -> Option<usize> {
    let modal_indices: Vec<usize> = hits
        .iter()
        .enumerate()
        .filter_map(|(index, hit)| {
            matches!(
                hit.action,
                Action::CancelRemoveGame | Action::ConfirmRemoveGame
            )
            .then_some(index)
        })
        .collect();
    let current_modal = modal_indices.iter().position(|index| Some(*index) == current);
    let next = next_focus_index(current_modal, modal_indices.len(), backwards)?;
    modal_indices.get(next).copied()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ToastKind {
    Success,
    Error,
    Info,
}

struct Toast {
    kind: ToastKind,
    text: String,
    born: Instant,
}

impl Toast {
    fn life(&self) -> f32 {
        if self.kind == ToastKind::Error {
            7.0
        } else {
            4.2
        }
    }
}

/// A hover highlight that is fading in or out.
struct Fade {
    action: Action,
    rect: Rect,
    radius: u32,
    fixed: bool,
    hover: Hover,
    level: f32,
}

fn fill_rr(fb: &mut Framebuffer, r: Rect, radius: u32, c: Color) {
    fb_fill_rrect(fb, r.x as i32, r.y as i32, r.width as i32, r.height as i32, radius as i32, c);
}

fn stroke_rr(fb: &mut Framebuffer, r: Rect, radius: u32, c: Color) {
    fb_stroke_rrect(fb, r.x as i32, r.y as i32, r.width as i32, r.height as i32, radius as i32, c);
}

fn same_rect(a: Rect, b: Rect) -> bool {
    a.x == b.x && a.y == b.y && a.width == b.width && a.height == b.height
}

fn inset(r: Rect, d: u32) -> Rect {
    if r.width > d * 2 + 1 && r.height > d * 2 + 1 {
        Rect::new(r.x + d, r.y + d, r.width - d * 2, r.height - d * 2)
    } else {
        r
    }
}

/// Loads the user's persisted desktop preferences. Unknown or malformed values
/// safely fall back to the default sort order.
fn load_library_sort(path: &std::path::Path) -> usize {
    // On Windows, replacing an existing file with std::fs::rename is not
    // supported. If the process was interrupted between moving the old file
    // aside and installing the new one, restore the last known-good copy.
    let backup = path.with_extension("bak");
    if !path.exists() && backup.exists() {
        let _ = std::fs::rename(&backup, path);
    }
    let Ok(contents) = std::fs::read_to_string(path) else {
        return 0;
    };
    let mut schema = None;
    let mut sort = None;
    for line in contents.lines() {
        if let Some(value) = line.strip_prefix("schema=") {
            schema = value.parse::<u32>().ok();
        } else if let Some(value) = line.strip_prefix("library_sort=") {
            sort = value.parse::<usize>().ok();
        }
    }
    if schema == Some(1) {
        sort.filter(|value| *value < 3).unwrap_or(0)
    } else {
        0
    }
}

/// Saves preferences through a synced sibling file before replacing the old one.
fn save_library_sort(path: &std::path::Path, sort: usize) -> std::io::Result<()> {
    use std::io::Write as _;
    let parent = path.parent().filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "preference path has no parent"))?;
    std::fs::create_dir_all(parent)?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary = path.with_extension(format!("{}-{nonce}.tmp", std::process::id()));
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&temporary)?;
    let contents = format!("schema=1\nlibrary_sort={}\n", sort.min(2));
    if let Err(error) = file.write_all(contents.as_bytes()).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }
    drop(file);
    if let Err(first_error) = std::fs::rename(&temporary, path) {
        // Unix rename replaces an existing destination atomically. Windows
        // does not, so keep a recoverable backup while replacing the file.
        if !path.exists() {
            let _ = std::fs::remove_file(&temporary);
            return Err(first_error);
        }
        let backup = path.with_extension("bak");
        if backup.exists() {
            std::fs::remove_file(&backup)?;
        }
        if let Err(error) = std::fs::rename(path, &backup) {
            let _ = std::fs::remove_file(&temporary);
            return Err(error);
        }
        if let Err(error) = std::fs::rename(&temporary, path) {
            let _ = std::fs::rename(&backup, path);
            let _ = std::fs::remove_file(&temporary);
            return Err(error);
        }
        let _ = std::fs::remove_file(&backup);
    }
    #[cfg(unix)]
    std::fs::File::open(parent)?.sync_all()?;
    Ok(())
}

/// Produces a stable path key for comparing discovered and registered installs.
fn normalized_install_path(path: &std::path::Path) -> String {
    let normalized = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let value = normalized.to_string_lossy().into_owned();

    // Windows paths are case-insensitive and accept both separator styles.
    // Unix paths are case-sensitive, and a backslash is a valid filename
    // character: normalizing either on Unix can merge distinct installations.
    #[cfg(windows)]
    {
        value
            .replace(char::from(92), "/")
            .trim_end_matches('/')
            .to_ascii_lowercase()
    }
    #[cfg(not(windows))]
    {
        value
    }
}


fn validate_external_url(url: &str) -> Result<&str, String> {
    let url = url.trim();
    let Some((scheme, remainder)) = url.split_once("://") else {
        return Err("URL HTTP(S) invalide ou non autorisée.".to_owned());
    };
    if !matches!(scheme, "http" | "https")
        || url.chars().any(|character| character.is_control() || character.is_whitespace() || character == '\\')
    {
        return Err("URL HTTP(S) invalide ou non autorisée.".to_owned());
    }
    let host = remainder.split(['/', '?', '#']).next().unwrap_or_default();
    if host.is_empty() || host.starts_with(':') || host.starts_with('@') {
        return Err("URL HTTP(S) invalide ou non autorisée.".to_owned());
    }
    Ok(url)
}

/// Opens a catalogue-provided store URL only when it is a plain HTTP(S) URL.
fn open_external_url(url: &str) -> Result<(), String> {
    let url = validate_external_url(url)?;
    #[cfg(windows)]
    let mut command = {
        let mut command = std::process::Command::new("rundll32.exe");
        command.args(["url.dll,FileProtocolHandler", url]);
        command
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = std::process::Command::new("open");
        command.arg(url);
        command
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = {
        let mut command = std::process::Command::new("xdg-open");
        command.arg(url);
        command
    };
    command.spawn().map(|_| ()).map_err(|error| error.to_string())
}

pub struct App {
    fonts: &'static Fonts,
    covers: &'static mut CoverStore,
    media_store: Option<&'static amethys_media::MediaStore>,
    pub page: Page,
    pub st: State,
    library_sort: usize,
    library_limit: usize,
    keyboard_focus: Option<usize>,
    pub games: Vec<IpcGameEntry>,
    /// Launch readiness per game, computed once per library load.
    pub statuses: Vec<GameStatus>,
    pub detected_games: Vec<amethys_discovery::DetectedGame>,
    scan_receiver: Option<std::sync::mpsc::Receiver<ScanMessage>>,
    scan_cancel: Option<Arc<AtomicBool>>,
    scan_progress: Option<amethys_discovery::DiscoveryProgress>,
    scan_in_progress: bool,
    scan_found: Option<usize>,
    library_loading: bool,
    library_refreshing: bool,
    library_silent_refresh: bool,
    /// Last automatic library refresh while Core may be discovering executables.
    last_auto_library_refresh: Instant,
    library_receiver: Option<std::sync::mpsc::Receiver<Result<Vec<IpcGameEntry>, String>>>,
    mutation_receiver: Option<std::sync::mpsc::Receiver<CoreMutationResult>>,
    mutation_in_progress: bool,
    pub core_error: Option<String>,
    toast: Option<Toast>,
    pub selected_game: Option<usize>,
    pending_remove: Option<usize>,
    search_query: String,
    search_focused: bool,
    store_query: String,
    store_search_focused: bool,
    store_promotions: bool,
    store_results: Vec<IpcCatalogSearchEntry>,
    store_detail: Option<IpcCatalogGameDetail>,
    store_loading: bool,
    store_catalog_version: Option<String>,
    store_game_count: Option<u64>,
    store_error: Option<String>,
    store_receiver: Option<std::sync::mpsc::Receiver<StoreTaskResult>>,
    executable_draft: String,
    editing_executable: bool,
    executable_focused: bool,
    win_w: i32,
    win_h: i32,
    lay: Layout,
    /// Whole page pre-rendered once (sidebar occupies the first `win_h` rows).
    content: Framebuffer,
    page_h: i32,
    hits: Vec<Hit>,
    pub scroll: ScrollViewport,
    scroll_pos: f32,
    scroll_target: f32,
    inter: UiInteraction,
    fades: Vec<Fade>,
    page_fade: Option<Instant>,
    esc_was_down: bool,
    pub stats_open: bool,
    frame_ms: VecDeque<f64>,
    work_ms: f64,
    page_ms: f64,
    last_frame: Option<Instant>,
    bench: Option<Bench>,
}

impl App {
    pub fn new(
        fonts: &'static Fonts,
        covers: &'static mut CoverStore,
        media_store: Option<&'static amethys_media::MediaStore>,
        library: Result<Vec<IpcGameEntry>, String>,
    ) -> Self {
        Self::with_size(fonts, covers, media_store, library, WIN_W, WIN_H)
    }

    pub fn with_size(
        fonts: &'static Fonts,
        covers: &'static mut CoverStore,
        media_store: Option<&'static amethys_media::MediaStore>,
        library: Result<Vec<IpcGameEntry>, String>,
        width: u32,
        height: u32,
    ) -> Self {
        let (win_w, win_h) = (width.max(MIN_W) as i32, height.max(200) as i32);
        let lay = Layout::new(win_w, win_h);
        let (games, core_error) = match library {
            Ok(games) => (games, None),
            Err(error) => (Vec::new(), Some(error)),
        };
        let statuses = status::statuses_of(&games);
        let library_sort = media_store
            .map(|store| load_library_sort(&store.root().join(DESKTOP_PREFERENCES_FILE)))
            .unwrap_or(0);
        let mut app = Self {
            fonts,
            covers,
            media_store,
            page: Page::Library,
            st: State::default(),
            library_sort,
            library_limit: PAGE_SIZE,
            keyboard_focus: None,
            games,
            statuses,
            detected_games: Vec::new(),
            scan_receiver: None,
            scan_cancel: None,
            scan_progress: None,
            scan_in_progress: false,
            scan_found: None,
            library_loading: false,
            library_refreshing: false,
            library_silent_refresh: false,
            last_auto_library_refresh: Instant::now(),
            library_receiver: None,
            mutation_receiver: None,
            mutation_in_progress: false,
            core_error,
            toast: None,
            selected_game: None,
            pending_remove: None,
            search_query: String::new(),
            search_focused: false,
            store_query: String::new(),
            store_search_focused: false,
            store_promotions: false,
            store_results: Vec::new(),
            store_detail: None,
            store_loading: false,
            store_catalog_version: None,
            store_game_count: None,
            store_error: None,
            store_receiver: None,
            executable_draft: String::new(),
            editing_executable: false,
            executable_focused: false,
            win_w,
            win_h,
            lay,
            content: Framebuffer::new(win_w as u32, win_h as u32, gfx::BG).expect("framebuffer"),
            page_h: win_h,
            hits: Vec::new(),
            scroll: ScrollViewport::new(
                Rect::new((lay.side_w as f32 * lay.scale).round() as u32, 0, (win_w - (lay.side_w as f32 * lay.scale).round() as i32).max(1) as u32, win_h as u32),
                LayoutDirection::Vertical,
                win_h as u32,
            ),
            scroll_pos: 0.0,
            scroll_target: 0.0,
            inter: UiInteraction::new(),
            fades: Vec::new(),
            page_fade: Some(Instant::now()),
            esc_was_down: false,
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

    // ------------------------------------------------------------ state helpers
    fn notify(&mut self, kind: ToastKind, text: impl Into<String>) {
        self.toast = Some(Toast { kind, text: text.into(), born: Instant::now() });
    }

    /// Starts a Core library request without blocking the window thread.
    fn start_library_request(&mut self, refreshing: bool) {
        if self.library_loading {
            return;
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = crate::core_client::list_games_or_start_agent();
            let _ = sender.send(result);
        });
        self.library_receiver = Some(receiver);
        self.library_loading = true;
        self.library_refreshing = refreshing;
        if !refreshing {
            self.core_error = None;
        }
        self.render(false);
    }

    /// Runs a master-catalogue request away from the window thread.
    fn start_store_task(&mut self, synchronize: bool) {
        if self.store_loading {
            return;
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        let query = self.store_query.trim().to_owned();
        let (genres, owned_ids, owned_names) = self.recommendation_profile();
        let personalized = query.is_empty() && (!owned_ids.is_empty() || !owned_names.is_empty());
        if self.store_promotions {
            return;
        }
        std::thread::spawn(move || {
            if synchronize {
                let result = crate::core_client::sync_master_catalog();
                let _ = sender.send(StoreTaskResult::Synchronized(result));
            } else if personalized {
                let result = crate::core_client::recommend_master_catalog(
                    genres, owned_ids, owned_names, 48,
                );
                let _ = sender.send(StoreTaskResult::Search(result));
            } else {
                let result = crate::core_client::search_master_catalog(&query, 48);
                let _ = sender.send(StoreTaskResult::Search(result));
            }
        });
        self.store_receiver = Some(receiver);
        self.store_loading = true;
        self.store_error = None;
        self.render(false);
    }

    fn poll_store_task(&mut self) {
        let result = match self.store_receiver.as_ref().map(|receiver| receiver.try_recv()) {
            Some(Ok(result)) => Some(result),
            Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) => {
                self.store_receiver = None;
                self.store_loading = false;
                self.store_error = Some("La requête du catalogue s'est interrompue.".to_owned());
                self.render(false);
                return;
            }
            Some(Err(std::sync::mpsc::TryRecvError::Empty)) | None => None,
        };
        let Some(result) = result else { return };
        self.store_receiver = None;
        self.store_loading = false;
        match result {
            StoreTaskResult::Search(Ok(games)) => {
                self.store_results = games;
                for game in &self.store_results {
                    if let Some(path) = game.cover_cache_path.as_deref() {
                        let game_id = game.amethys_id.to_string();
                        let _ = self.covers.load_library_cover(
                            &game_id,
                            std::path::Path::new(path),
                        );
                    }
                }
                self.store_error = None;
            }
            StoreTaskResult::Search(Err(error)) => self.store_error = Some(error),
            StoreTaskResult::Detail(Ok(detail)) => {
                if let Some(path) = detail.cover_cache_path.as_deref() {
                    let game_id = detail.amethys_id.to_string();
                    if let Err(error) = self.covers.load_library_cover(&game_id, std::path::Path::new(path)) {
                        self.notify(ToastKind::Info, format!("Jaquette du catalogue indisponible : {error}"));
                    }
                }
                self.store_detail = Some(detail);
                self.store_error = None;
                self.page = Page::StoreProduct;
                self.keyboard_focus = None;
                self.start_page_fade();
            }
            StoreTaskResult::Detail(Err(error)) => self.store_error = Some(error),
            StoreTaskResult::Synchronized(Ok((version, count))) => {
                self.store_catalog_version = Some(version);
                self.store_game_count = Some(count);
                self.store_error = None;
                self.notify(ToastKind::Success, format!("Catalogue synchronisé : {count} jeux."));
                // Re-run the current query, now using the freshly synchronized index.
                self.start_store_task(false);
            }
            StoreTaskResult::Synchronized(Err(error)) => self.store_error = Some(error),
        }
        self.render(false);
    }

    /// Starts the initial Core request without blocking window creation.
    pub fn start_library_load(&mut self) {
        self.start_library_request(false);
    }

    /// Refreshes the library in the background while keeping the current view responsive.
    fn start_library_refresh(&mut self) {
        self.st.lib_filter = 0;
        self.start_library_request(true);
    }

    /// Runs a Core write operation away from the window thread.
    fn start_core_mutation(&mut self, task: CoreMutationTask) {
        if self.mutation_in_progress {
            self.notify(ToastKind::Info, "Une opération sur la bibliothèque est déjà en cours.");
            self.render(false);
            return;
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = match task {
                CoreMutationTask::Import { detected, context } => {
                    CoreMutationResult::Imported {
                        context,
                        result: crate::core_client::add_detected_game(&detected),
                    }
                }
                CoreMutationTask::Remove { id } => {
                    CoreMutationResult::Removed(crate::core_client::remove_game(&id))
                }
                CoreMutationTask::SetExecutable { id, executable } => {
                    CoreMutationResult::ExecutableUpdated {
                        id: id.clone(),
                        result: crate::core_client::set_game_executable(&id, &executable),
                    }
                }
            };
            let _ = sender.send(result);
        });
        self.mutation_receiver = Some(receiver);
        self.mutation_in_progress = true;
        self.notify(ToastKind::Info, "Opération transmise à Amethys Core…");
        self.render(false);
    }

    /// Applies completed Core write operations on the UI thread.
    fn poll_core_mutation(&mut self) {
        let result = match self.mutation_receiver.as_ref().map(|receiver| receiver.try_recv()) {
            Some(Ok(result)) => Some(result),
            Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) => {
                self.mutation_receiver = None;
                self.mutation_in_progress = false;
                self.notify(ToastKind::Error, "L'opération Amethys Core s'est interrompue de manière inattendue.");
                self.render(false);
                return;
            }
            Some(Err(std::sync::mpsc::TryRecvError::Empty)) | None => None,
        };
        let Some(result) = result else { return };
        self.mutation_receiver = None;
        self.mutation_in_progress = false;
        match result {
            CoreMutationResult::Imported { context, result } => match result {
                Ok(game) => {
                    let name = game.name.clone();
                    let platform = context.1.clone();
                    self.detected_games.retain(|candidate| {
                        !(candidate.name == context.0
                            && candidate.platform == context.1
                            && candidate.install_path == context.2)
                    });
                    if self.detected_games.is_empty() {
                        self.scan_found = Some(0);
                    }
                    self.notify(ToastKind::Success, format!("{name} ({platform}) a été ajouté à votre bibliothèque."));
                    self.start_library_refresh();
                }
                Err(error) => {
                    self.notify(ToastKind::Error, format!(
                        "Import de {} ({}) impossible depuis {} : {error}",
                        context.0, context.1, context.2.display()
                    ));
                }
            },
            CoreMutationResult::Removed(result) => match result {
                Ok(true) => {
                    self.st.lib_filter = 0;
                    self.page = Page::Library;
                    self.selected_game = None;
                    self.pending_remove = None;
                    self.start_page_fade();
                    self.notify(ToastKind::Success, "Jeu retiré de la bibliothèque. Ses fichiers n'ont pas été supprimés.");
                    self.start_library_refresh();
                }
                Ok(false) => {
                    self.pending_remove = None;
                    self.notify(ToastKind::Info, "Le jeu n'était déjà plus présent dans la bibliothèque.");
                }
                Err(error) => {
                    self.pending_remove = None;
                    self.notify(ToastKind::Error, format!("Impossible de retirer le jeu : {error}"));
                }
            },
            CoreMutationResult::ExecutableUpdated { id, result } => match result {
                Ok(updated) => {
                    if let Some(index) = self.games.iter().position(|game| game.id == id) {
                        self.games[index] = updated;
                        self.statuses = status::statuses_of(&self.games);
                        self.executable_draft = self.games[index].executable.clone().unwrap_or_default();
                    }
                    self.editing_executable = false;
                    self.executable_focused = false;
                    self.keyboard_focus = None;
                    self.notify(ToastKind::Success, "Chemin de l'exécutable enregistré dans Amethys Core.");
                }
                Err(error) => {
                    self.editing_executable = true;
                    self.executable_focused = true;
                    self.notify(ToastKind::Error, format!("Impossible d'enregistrer l'exécutable : {error}"));
                }
            },
        }
        self.render(false);
    }

    /// Applies a Core library response on the UI thread.
    fn poll_library_load(&mut self) {
        let result = match self.library_receiver.as_ref().map(|receiver| receiver.try_recv()) {
            Some(Ok(result)) => Some(result),
            Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) => Some(Err(
                "La requête de bibliothèque s'est interrompue.".to_owned()
            )),
            Some(Err(std::sync::mpsc::TryRecvError::Empty)) | None => None,
        };
        if let Some(result) = result {
            let refreshing = self.library_refreshing;
            let silent_refresh = self.library_silent_refresh;
            self.library_receiver = None;
            self.library_loading = false;
            self.library_refreshing = false;
            self.library_silent_refresh = false;
            match result {
                Ok(games) => {
                    self.set_games(games);
                    self.core_error = None;
                    if refreshing && !silent_refresh {
                        self.notify(ToastKind::Success, "Bibliothèque actualisée.");
                    }
                }
                Err(error) => {
                    self.core_error = Some(error.clone());
                    if refreshing && !silent_refresh {
                        self.notify(ToastKind::Error, format!("Actualisation impossible : {error}"));
                    }
                }
            }
            self.render(false);
        }
    }

    /// Whether the current library contains ownership signals for personalization.
    fn has_recommendation_profile(&self) -> bool {
        !self.games.is_empty()
    }

    /// Extracts the local ownership and genre profile without contacting Steam.
    fn recommendation_profile(&self) -> (Vec<String>, Vec<u64>, Vec<String>) {
        let mut genres = Vec::new();
        let mut owned_ids = Vec::new();
        let mut owned_names = Vec::with_capacity(self.games.len());
        for game in &self.games {
            owned_names.push(game.name.clone());
            if let Some(id) = game.catalogue_game_id.as_deref().and_then(|value| value.parse::<u64>().ok()) {
                owned_ids.push(id);
            }
            if let Some(metadata) = game.catalogue_metadata.as_ref() {
                genres.extend(metadata.genres.iter().cloned());
            }
        }
        (genres, owned_ids, owned_names)
    }

    /// Replaces the library and recomputes every launch status once.
    fn set_games(&mut self, games: Vec<IpcGameEntry>) {
        self.statuses = status::statuses_of(&games);
        self.games = games;
        self.library_limit = self.library_limit.max(PAGE_SIZE);
        if self.selected_game.is_some_and(|index| index >= self.games.len()) {
            self.selected_game = None;
        }
    }

    fn start_page_fade(&mut self) {
        self.page_fade = Some(Instant::now());
    }

    // ------------------------------------------------------------------ render
    /// Pre-renders the current page into a fresh image (no state is touched).
    fn paint(&self, buf_h: u32) -> (Vec<Hit>, Framebuffer, i32) {
        let lay = self.lay;
        let t_alloc = Instant::now();
        let mut p = Painter::new(self.win_w as u32, buf_h, self.fonts).with_scale(lay.scale).with_covers(&*self.covers);
        let alloc_ms = t_alloc.elapsed().as_secs_f64() * 1000.0;
        let t_side = Instant::now();
        let info = SidebarInfo {
            core_ok: self.core_error.is_none() && !self.library_loading,
            core_loading: self.library_loading,
            core_refreshing: self.library_refreshing,
            game_count: self.games.len(),
        };
        let active = match self.page {
            Page::Store => Active::Store,
            Page::Library | Page::Product => Active::Library,
            Page::StoreProduct => Active::Store,
            Page::Settings => Active::Settings,
        };
        pages::sidebar(&mut p, &lay, active, &info);
        let side_ms = t_side.elapsed().as_secs_f64() * 1000.0;
        let t_page = Instant::now();
        let h = match self.page {
            Page::StoreProduct => pages::store_product(&mut p, &lay, self.store_detail.as_ref(), self.store_loading, self.store_error.as_deref()),
            Page::Store => pages::store(&mut p, &lay, &pages::StoreView {
                query: &self.store_query,
                focused: self.store_search_focused,
                results: &self.store_results,
                recommended: self.store_query.trim().is_empty() && self.has_recommendation_profile(),
                promotions: self.store_promotions,
                loading: self.store_loading,
                catalog_version: self.store_catalog_version.as_deref(),
                game_count: self.store_game_count,
                error: self.store_error.as_deref(),
                detail: self.store_detail.as_ref(),
            }),
            Page::Library => pages::library(
                &mut p,
                &lay,
                &LibraryView {
                    games: &self.games,
                    statuses: &self.statuses,
                    detected: &self.detected_games,
                    scanning: self.scan_in_progress,
                    loading: self.library_loading,
                    refreshing: self.library_refreshing,
                    mutating: self.mutation_in_progress,
                    progress: self.scan_progress.as_ref(),
                    scan_found: self.scan_found,
                    filter: self.st.lib_filter,
                    sort: self.library_sort,
                    query: &self.search_query,
                    search_focused: self.search_focused,
                    core_error: self.core_error.as_deref(),
                    limit: self.library_limit,
                },
            ),
            Page::Product => {
                let index = self.selected_game.unwrap_or(usize::MAX);
                pages::game_detail(
                    &mut p,
                    &lay,
                    &DetailView {
                        game: self.games.get(index),
                        index,
                        status: self.statuses.get(index),
                        executable_draft: &self.executable_draft,
                        editing_executable: self.editing_executable,
                    },
                )
            }
            Page::Settings => pages::settings(
                &mut p,
                &lay,
                &SettingsView {
                    core_error: self.core_error.as_deref(),
                    core_loading: self.library_loading,
                    core_refreshing: self.library_refreshing,
                    game_count: self.games.len(),
                    ready_count: self.statuses.iter().filter(|s| s.is_ready()).count(),
                    media_root: self.media_store.map(|store| store.root()),
                    library_sort: self.library_sort,
                    scanning: self.scan_in_progress,
                    progress: self.scan_progress.as_ref(),
                },
            ),
        };
        if let Some(index) = self.pending_remove {
            // A modal must be the only interactive layer while it is open.
            p.hits.clear();
            let name = self.games.get(index).map_or("ce jeu", |game| game.name.as_str());
            pages::remove_dialog(&mut p, &lay, name, (self.scroll.offset() as f32 / lay.scale).round() as i32);
        }
        if std::env::var_os("AMETHYS_TIMING").is_some() {
            eprintln!("  paint {:?}: buffer {}x{} alloc {:.1} ms, sidebar {:.1} ms, page {:.1} ms", self.page, self.win_w, buf_h, alloc_ms, side_ms, t_page.elapsed().as_secs_f64() * 1000.0);
        }
        (p.hits, p.fb, (h.max(lay.h) as f32 * lay.scale).ceil() as i32)
    }

    /// Updates only the store search field; artwork/cards stay in the cached framebuffer.
    fn redraw_store_search_only(&mut self) {
        let old = std::mem::replace(
            &mut self.content,
            Framebuffer::new(1, 1, gfx::BG).expect("temporary framebuffer"),
        );
        let mut painter = Painter::from_framebuffer(old, self.fonts).with_scale(self.lay.scale);
        pages::redraw_store_search(&mut painter, &self.lay, &self.store_query, self.store_search_focused);
        self.hits.retain(|hit| !matches!(hit.action, Action::FocusStoreSearch | Action::ClearStoreSearch));
        self.hits.append(&mut painter.hits);
        self.content = painter.fb;
    }

    /// Re-draws the current page (sidebar, content, hit regions).
    fn render(&mut self, reset_scroll: bool) {
        let t0 = Instant::now();
        // Preserve focus by action identity, not by its old vector index: a
        // filter, search result update, or responsive reflow can reorder hits.
        let focused_action = self.keyboard_focus
            .and_then(|index| self.hits.get(index))
            .map(|hit| hit.action);
        let mut buf_h = ((self.page_h.max(self.win_h) + 600) as u32).clamp(self.win_h as u32, 30_000);
        let (hits, fb, h) = loop {
            let (hits, fb, h) = self.paint(buf_h);
            // Painting beyond the buffer is clipped: grow it and paint again.
            if h as u32 + 8 > buf_h && buf_h < 30_000 {
                buf_h = (h as u32 + 600).min(30_000);
                continue;
            }
            break (hits, fb, h);
        };
        self.hits = hits;
        self.content = fb;
        self.keyboard_focus = focused_action
            .and_then(|action| focus_index_for_action(&self.hits, action));
        // Keep keyboard focus inside the modal and make the safe action the
        // default. Enter therefore dismisses the dialog instead of deleting.
        if self.pending_remove.is_some() && self.keyboard_focus.is_none() {
            self.keyboard_focus = self.hits.iter().position(|hit| hit.action == Action::CancelRemoveGame);
        }
        self.page_h = h;
        self.scroll.set_content_extent(h as u32);
        let max = self.scroll.max_offset() as f32;
        if reset_scroll {
            self.scroll_pos = 0.0;
            self.scroll_target = 0.0;
        } else {
            self.scroll_pos = self.scroll_pos.clamp(0.0, max);
            self.scroll_target = self.scroll_target.clamp(0.0, max);
        }
        self.scroll.set_offset(self.scroll_pos.round() as u32);
        if self.keyboard_focus.is_some_and(|index| index >= self.hits.len()) {
            self.keyboard_focus = None;
        }
        self.inter.clear();
        self.page_ms = t0.elapsed().as_secs_f64() * 1000.0;
    }

    /// Adapts the layout to a new window size.
    fn resize(&mut self, width: i32, height: i32) {
        self.win_w = width.max(MIN_W as i32);
        self.win_h = height.max(200);
        self.lay = Layout::new(self.win_w, self.win_h);
        self.scroll = ScrollViewport::new(
            Rect::new(self.side_px() as u32, 0, (self.win_w - self.side_px()).max(1) as u32, self.win_h as u32),
            LayoutDirection::Vertical,
            self.win_h as u32,
        );
        self.fades.clear();
        self.render(false);
    }

    // ------------------------------------------------------------------ actions
    fn side_px(&self) -> i32 { (self.lay.side_w as f32 * self.lay.scale).round() as i32 }

    fn selected_artwork(&self) -> Option<(String, String)> {
        self.selected_game.and_then(|index| self.games.get(index)).map(|game| {
            (game.catalogue_game_id.as_deref().unwrap_or(&game.id).to_owned(), game.name.clone())
        })
    }

    fn apply(&mut self, action: Action) {
        if !matches!(action, Action::FocusLibrarySearch) {
            self.search_focused = false;
        }
        if !matches!(action, Action::FocusStoreSearch) {
            self.store_search_focused = false;
        }
        if !matches!(action, Action::EditExecutable | Action::SaveExecutable | Action::CancelExecutableEdit) {
            self.editing_executable = false;
            self.executable_focused = false;
        }
        match action {
            Action::Go(page) => {
                self.page = page;
                self.store_search_focused = false;
                if page == Page::Store && self.store_catalog_version.is_none() && !self.store_loading {
                    self.start_store_task(true);
                }
                if page == Page::Library {
                    self.selected_game = None;
                }
                self.keyboard_focus = None;
                self.start_page_fade();
                self.render(true);
            }
            Action::FocusStoreSearch => {
                self.store_search_focused = true;
                self.search_focused = false;
                self.keyboard_focus = focus_index_for_action(&self.hits, Action::FocusStoreSearch);
                self.render(false);
            }
            Action::ClearStoreSearch => {
                self.store_query.clear();
                self.redraw_store_search_only();
            }
            Action::SetStoreMode(promotions) => {
                if self.store_promotions != promotions {
                    self.store_promotions = promotions;
                    self.store_search_focused = false;
                    self.keyboard_focus = None;
                    self.render(true);
                    if !promotions && self.store_results.is_empty() {
                        self.start_store_task(false);
                    }
                }
            }
            Action::SearchStore => self.start_store_task(false),
            Action::SyncMasterCatalog => self.start_store_task(true),
            Action::OpenCatalogStore(index) => {
                if let Some(url) = self.store_detail.as_ref()
                    .and_then(|detail| detail.stores.get(index))
                    .and_then(|store| store.url.as_deref())
                {
                    if let Err(error) = open_external_url(url) {
                        self.notify(ToastKind::Error, format!("Impossible d'ouvrir la boutique : {error}"));
                    }
                }
                self.render(false);
            }
            Action::SelectCatalogGame(amethys_id) => {
                if !self.store_loading {
                    let (sender, receiver) = std::sync::mpsc::channel();
                    std::thread::spawn(move || {
                        let result = crate::core_client::get_master_catalog_game(amethys_id);
                        let _ = sender.send(StoreTaskResult::Detail(result.map(Box::new)));
                    });
                    self.store_receiver = Some(receiver);
                    self.store_loading = true;
                    self.store_error = None;
                    self.store_detail = None;
                    self.page = Page::StoreProduct;
                    self.keyboard_focus = None;
                    self.start_page_fade();
                    self.render(true);
                }
            }
            Action::BackToStore => {
                self.page = Page::Store;
                self.keyboard_focus = None;
                self.start_page_fade();
                self.render(true);
            }
            Action::SelectGame(index) => {
                if let Some(game) = self.games.get(index) {
                    self.executable_draft = game.executable.clone().unwrap_or_default();
                    self.selected_game = Some(index);
                    self.page = Page::Product;
                    self.keyboard_focus = None;
                    self.start_page_fade();
                    self.render(true);
                }
            }
            Action::LibFilter(i) => {
                self.st.lib_filter = i;
                self.library_limit = PAGE_SIZE;
                self.render(false);
            }
            Action::SetLibrarySort(i) => {
                self.library_sort = i % 3;
                if let Some(store) = self.media_store {
                    let path = store.root().join(DESKTOP_PREFERENCES_FILE);
                    if let Err(error) = save_library_sort(&path, self.library_sort) {
                        self.notify(ToastKind::Error, format!("Préférence de tri non enregistrée : {error}"));
                    } else {
                        self.notify(ToastKind::Success, "Ordre de tri enregistré sur cet appareil.");
                    }
                } else {
                    self.notify(ToastKind::Error, "Stockage des préférences indisponible : le tri ne sera pas conservé.");
                }
                self.render(false);
            }
            Action::ShowMoreGames => {
                self.library_limit += PAGE_SIZE;
                self.render(false);
            }
            Action::FocusLibrarySearch => {
                self.search_focused = true;
                // The text field participates in the same Tab order as buttons.
                self.keyboard_focus = focus_index_for_action(&self.hits, Action::FocusLibrarySearch);
                self.render(false);
            }
            Action::ClearSearch => {
                self.search_query.clear();
                self.library_limit = PAGE_SIZE;
                self.render(false);
            }
            Action::ChangeGameCover => {
                match self.selected_artwork() {
                    Some((game_id, game_name)) => match crate::file_dialog::pick_image() {
                        Ok(Some(source)) => {
                            let result = self.covers.validate_image(&source).and_then(|()| {
                                let store = self.media_store.ok_or_else(|| {
                                    "Le dossier de données local d'Amethys n'est pas disponible.".to_owned()
                                })?;
                                let imported = store
                                    .import_library_asset(&game_id, amethys_media::MediaKind::Cover, &source)
                                    .map_err(|error| error.to_string())?;
                                self.covers.load_library_cover(&game_id, &imported)?;
                                Ok(())
                            });
                            match result {
                                Ok(()) => self.notify(ToastKind::Success, format!("Jaquette personnalisée enregistrée pour {game_name}.")),
                                Err(error) => self.notify(ToastKind::Error, format!("Impossible d'importer la jaquette : {error}")),
                            }
                        }
                        Ok(None) => {}
                        Err(error) => self.notify(ToastKind::Error, format!("Sélecteur d'image indisponible : {error}")),
                    },
                    None => self.notify(ToastKind::Error, "Aucun jeu n'est sélectionné."),
                }
                self.render(false);
            }
            Action::ResetGameCover => {
                let result = self
                    .selected_artwork()
                    .ok_or_else(|| "Aucun jeu n'est sélectionné.".to_owned())
                    .and_then(|(game_id, game_name)| {
                        let store = self.media_store.ok_or_else(|| {
                            "Le dossier de données local d'Amethys n'est pas disponible.".to_owned()
                        })?;
                        self.covers
                            .reset_library_cover(&game_id, &store.library_root())
                            .map(|removed| (game_name, removed))
                    });
                match result {
                    Ok((name, true)) => self.notify(ToastKind::Success, format!("Jaquette personnalisée supprimée pour {name}. L'image par défaut est rétablie.")),
                    Ok((name, false)) => self.notify(ToastKind::Info, format!("{name} utilise déjà son image par défaut.")),
                    Err(error) => self.notify(ToastKind::Error, format!("Impossible de rétablir l'image par défaut : {error}")),
                }
                self.render(false);
            }
            Action::EditExecutable => {
                self.editing_executable = true;
                self.executable_focused = true;
                self.render(false);
            }
            Action::SaveExecutable => {
                if self.mutation_in_progress {
                    self.notify(ToastKind::Info, "Attendez la fin de l'opération Core en cours.");
                    self.render(false);
                } else if self.executable_draft.trim().is_empty() {
                    self.editing_executable = true;
                    self.executable_focused = true;
                    self.notify(ToastKind::Error, "Indiquez le chemin de l'exécutable avant de l'enregistrer.");
                    self.render(false);
                } else if let Some(game) = self.selected_game.and_then(|index| self.games.get(index)) {
                    let id = game.id.clone();
                    let executable = self.executable_draft.trim().to_owned();
                    self.editing_executable = false;
                    self.executable_focused = false;
                    self.keyboard_focus = None;
                    self.start_core_mutation(CoreMutationTask::SetExecutable { id, executable });
                } else {
                    self.editing_executable = true;
                    self.executable_focused = true;
                    self.notify(ToastKind::Error, "Aucun jeu n'est sélectionné.");
                    self.render(false);
                }
            }
            Action::CancelExecutableEdit => {
                self.executable_draft = self
                    .selected_game
                    .and_then(|index| self.games.get(index))
                    .and_then(|game| game.executable.clone())
                    .unwrap_or_default();
                self.editing_executable = false;
                self.executable_focused = false;
                self.keyboard_focus = None;
                self.render(false);
            }
            Action::ScanGames => {
                if !self.scan_in_progress {
                    let (sender, receiver) = std::sync::mpsc::channel();
                    let cancel = Arc::new(AtomicBool::new(false));
                    let worker_cancel = Arc::clone(&cancel);
                    std::thread::spawn(move || {
                        let outcome = amethys_discovery::discover_all_games_with_progress(&worker_cancel, |progress| {
                            let _ = sender.send(ScanMessage::Progress(progress));
                        });
                        let _ = sender.send(ScanMessage::Finished(outcome));
                    });
                    self.scan_receiver = Some(receiver);
                    self.scan_cancel = Some(cancel);
                    self.scan_progress = None;
                    self.scan_in_progress = true;
                    self.scan_found = None;
                    self.detected_games.clear();
                    self.render(false);
                }
            }
            Action::CancelScan => {
                if let Some(cancel) = &self.scan_cancel {
                    cancel.store(true, Ordering::Relaxed);
                    self.notify(ToastKind::Info, "Annulation demandée : le fournisseur en cours termine son analyse avant l'arrêt.");
                    self.render(false);
                }
            }
            Action::ImportDetectedGame(index) => {
                if self.mutation_in_progress {
                    self.notify(ToastKind::Info, "Attendez la fin de l'opération Core en cours.");
                    self.render(false);
                    return;
                }
                let Some(detected) = self.detected_games.get(index).cloned() else {
                    self.notify(ToastKind::Error, "Cette installation détectée n'est plus disponible.");
                    self.render(false);
                    return;
                };
                let context = (detected.name.clone(), detected.platform.to_owned(), detected.install_path.clone());
                self.start_core_mutation(CoreMutationTask::Import { detected, context });
            }
            Action::RefreshLibrary => {
                self.start_library_refresh();
            }
            Action::LaunchGame(index) => {
                let result = match self.games.get(index) {
                    Some(game) => crate::core_client::launch_game(game)
                        .map(|pid| format!("Démarrage demandé : {} (processus {pid}).", game.name)),
                    None => Err("Ce jeu n'existe plus dans la bibliothèque chargée.".to_owned()),
                };
                match result {
                    Ok(message) => self.notify(ToastKind::Success, message),
                    Err(error) => self.notify(ToastKind::Error, format!("Échec du lancement : {error}")),
                }
                self.render(false);
            }
            Action::RemoveGame(index) => {
                if self.games.get(index).is_some() {
                    self.pending_remove = Some(index);
                    // A destructive dialog must never inherit focus from the page
                    // behind it. The dialog renderer will place focus on Cancel.
                    self.keyboard_focus = None;
                } else {
                    self.notify(ToastKind::Error, "Ce jeu n'existe plus dans la bibliothèque chargée.");
                }
                self.render(false);
            }
            Action::CancelRemoveGame => {
                self.pending_remove = None;
                self.keyboard_focus = None;
                self.render(false);
            }
            Action::ConfirmRemoveGame => {
                if self.mutation_in_progress {
                    self.notify(ToastKind::Info, "Attendez la fin de l'opération Core en cours.");
                    self.render(false);
                    return;
                }
                self.keyboard_focus = None;
                let Some(id) = self.pending_remove.take().and_then(|index| self.games.get(index).map(|game| game.id.clone())) else {
                    self.notify(ToastKind::Error, "Ce jeu n'existe plus dans la bibliothèque chargée.");
                    self.render(false);
                    return;
                };
                self.pending_remove = None;
                self.start_core_mutation(CoreMutationTask::Remove { id });
            }
        }
    }

    // ---------------------------------------------------------------- geometry
    /// Screen-space rectangle of a page-space region, clipped to its viewport.
    fn screen_rect(&self, rect: Rect, fixed: bool) -> Option<Rect> {
        let off = self.scroll.offset() as i32;
        let (x0, y0) = (rect.x as i32, rect.y as i32 - if fixed { 0 } else { off });
        let (x1, y1) = (x0 + rect.width as i32, y0 + rect.height as i32);
        let (cx0, cx1) = if fixed { (0, self.win_w) } else { (self.side_px(), self.win_w) };
        let (a, b, c, d) = (x0.max(cx0), y0.max(0), x1.min(cx1), y1.min(self.win_h));
        (c > a && d > b).then(|| Rect::new(a as u32, b as u32, (c - a) as u32, (d - b) as u32))
    }

    /// Screen-space, viewport-clipped bounds of every hit region.
    fn screen_bounds(&self) -> (Vec<Rect>, Vec<bool>) {
        let mut bounds = Vec::with_capacity(self.hits.len());
        let mut enabled = Vec::with_capacity(self.hits.len());
        for hit in &self.hits {
            match self.screen_rect(hit.rect, hit.fixed) {
                Some(rect) => {
                    bounds.push(rect);
                    enabled.push(true);
                }
                None => {
                    bounds.push(Rect::new(0, 0, 0, 0));
                    enabled.push(false);
                }
            }
        }
        (bounds, enabled)
    }

    /// Screen position of the first visible region whose action matches (used by the scripted replay).
    pub fn find_hit(&self, matches: impl Fn(Action) -> bool) -> Option<Point> {
        self.hits.iter().find_map(|hit| {
            if !matches(hit.action) {
                return None;
            }
            self.screen_rect(hit.rect, hit.fixed)
                .map(|r| Point::new((r.x + r.width / 2) as i32, (r.y + r.height / 2) as i32))
        })
    }

    /// Scrolls (smoothly) so a keyboard-focused region is comfortably in view.
    fn ensure_visible(&mut self, index: usize) {
        let Some(hit) = self.hits.get(index) else { return };
        if hit.fixed {
            return;
        }
        let (top, bottom) = (hit.rect.y as f32, (hit.rect.y + hit.rect.height) as f32);
        let (view_top, view_bottom) = (self.scroll_target, self.scroll_target + self.win_h as f32);
        if top < view_top + 24.0 || bottom > view_bottom - 24.0 {
            let max = self.scroll.max_offset() as f32;
            self.scroll_target = (top - self.win_h as f32 / 3.0).clamp(0.0, max);
        }
    }

    // ------------------------------------------------------------------ drawing
    fn blit(&self, fb: &mut Framebuffer) {
        let (cw, fw) = (self.content.width() as usize, fb.width() as usize);
        if cw != fw {
            return;
        }
        let rows = (fb.height() as usize).min(self.win_h as usize);
        let offset = self.scroll.offset() as usize;
        let sidebar_w = self.side_px() as usize;
        let source = self.content.pixels();
        let target = fb.pixels_mut();
        let available = self.content.height() as usize;
        for y in 0..rows {
            let dst = y * fw;
            target[dst..dst + sidebar_w].copy_from_slice(&source[y * cw..y * cw + sidebar_w]);
            let sy = (y + offset).min(available - 1);
            target[dst + sidebar_w..dst + fw].copy_from_slice(&source[sy * cw + sidebar_w..(sy + 1) * cw]);
        }
    }

    fn overlay_scaled(&self, fb: &mut Framebuffer, at: (i32, i32, i32, i32), opacity: f32, scale: f32, draw: impl FnOnce(&mut Painter)) {
        let (x, y, w, h) = at;
        if opacity <= 0.0 || w <= 0 || h <= 0 || x < 0 || y < 0 || x + w > fb.width() as i32 || y + h > fb.height() as i32 {
            return;
        }
        let fw = fb.width() as usize;
        let (uw, uh) = (w as usize, h as usize);
        let mut p = Painter::new(w as u32, h as u32, self.fonts).with_scale(scale);
        for row in 0..uh {
            let s = (y as usize + row) * fw + x as usize;
            p.fb.pixels_mut()[row * uw..(row + 1) * uw].copy_from_slice(&fb.pixels()[s..s + uw]);
        }
        draw(&mut p);
        let t = opacity.min(1.0);
        for row in 0..uh {
            let d = (y as usize + row) * fw + x as usize;
            let src = &p.fb.pixels()[row * uw..(row + 1) * uw];
            let dst = &mut fb.pixels_mut()[d..d + uw];
            if t >= 1.0 {
                dst.copy_from_slice(src);
            } else {
                for (o, n) in dst.iter_mut().zip(src) {
                    let mix = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t).round() as u8;
                    *o = Color::rgb(mix(o.r, n.r), mix(o.g, n.g), mix(o.b, n.b));
                }
            }
        }
    }

    /// Updates the hover fades from the current interaction state.
    fn update_fades(&mut self, dt: f32) {
        let current = self.inter.hovered().and_then(|i| self.hits.get(i)).map(|h| (h.action, h.rect));
        for fade in &mut self.fades {
            let is_current = current.is_some_and(|(action, rect)| action == fade.action && same_rect(rect, fade.rect));
            let (target, seconds) = if is_current { (1.0, HOVER_IN_S) } else { (0.0, HOVER_OUT_S) };
            fade.level = step_level(fade.level, target, dt, seconds);
        }
        if let (Some((action, rect)), Some(i)) = (current, self.inter.hovered()) {
            if !self.fades.iter().any(|f| f.action == action && same_rect(f.rect, rect)) {
                let hit = &self.hits[i];
                self.fades.push(Fade { action, rect, radius: hit.radius, fixed: hit.fixed, hover: hit.hover, level: 0.0 });
            }
        }
        let keep = current;
        self.fades.retain(|f| {
            f.level > 0.0 || keep.is_some_and(|(action, rect)| action == f.action && same_rect(rect, f.rect))
        });
    }

    fn fades_animating(&self) -> bool {
        let current = self.inter.hovered().and_then(|i| self.hits.get(i)).map(|h| (h.action, h.rect));
        self.fades.iter().any(|f| {
            let is_current = current.is_some_and(|(action, rect)| action == f.action && same_rect(rect, f.rect));
            if is_current { f.level < 1.0 } else { f.level > 0.0 }
        })
    }

    fn paint_fades(&self, fb: &mut Framebuffer, pressed: bool) {
        for fade in &self.fades {
            let level = ease_out(fade.level);
            if level <= 0.01 {
                continue;
            }
            let Some(r) = self.screen_rect(fade.rect, fade.fixed) else { continue };
            let scaled = |a: f32| (a * level).round().clamp(0.0, 255.0) as u8;
            let boost = if pressed && fade.level >= 0.99 { 1.9 } else { 1.0 };
            match fade.hover {
                Hover::Glow => {
                    fill_rr(fb, r, fade.radius, Color::rgba(185, 162, 255, scaled(26.0 * boost)));
                    stroke_rr(fb, r, fade.radius, Color::rgba(185, 162, 255, scaled(200.0)));
                }
                Hover::Card => {
                    fill_rr(fb, r, fade.radius, Color::rgba(255, 255, 255, scaled(10.0 * boost)));
                    stroke_rr(fb, r, fade.radius, Color::rgba(205, 188, 255, scaled(240.0)));
                    stroke_rr(fb, inset(r, 1), fade.radius.saturating_sub(1), Color::rgba(185, 162, 255, scaled(110.0)));
                }
                Hover::Soft => {
                    fill_rr(fb, r, fade.radius, Color::rgba(255, 255, 255, scaled(16.0 * boost)));
                }
                Hover::Danger => {
                    fill_rr(fb, r, fade.radius, Color::rgba(242, 107, 126, scaled(30.0 * boost)));
                    stroke_rr(fb, r, fade.radius, Color::rgba(242, 107, 126, scaled(210.0)));
                }
            }
        }
    }

    fn page_fade_alpha(&self) -> Option<u8> {
        let started = self.page_fade?;
        let t = started.elapsed().as_secs_f32() / PAGE_FADE_S;
        (t < 1.0).then(|| ((1.0 - ease_out(t)) * 235.0).round() as u8)
    }

    /// Pages, hover, scrollbar. `mask` hides the pointer from the page (used while it is over the overlays).
    fn draw_scene(&mut self, fb: &mut Framebuffer, real: &InputState, mask: bool, dt: f32) {
        let blank = InputState::default();
        let input = if mask { &blank } else { real };
        let main = Rect::new(self.side_px() as u32, 0, (self.win_w - self.side_px()).max(1) as u32, self.win_h as u32);
        let pointer = input.mouse_position;
        let in_main = pointer.is_some_and(|p| main.contains(p));

        // Smooth scrolling toward the wheel target (frozen while a dialog is open).
        if input.scroll_delta_y != 0 && in_main && self.pending_remove.is_none() {
            let max = self.scroll.max_offset() as f32;
            self.scroll_target = (self.scroll_target + input.scroll_delta_y as f32).clamp(0.0, max);
        }
        if (self.scroll_target - self.scroll_pos).abs() > 0.5 {
            let k = 1.0 - (-dt * SCROLL_RATE).exp();
            self.scroll_pos += (self.scroll_target - self.scroll_pos) * k;
        } else {
            self.scroll_pos = self.scroll_target;
        }
        self.scroll.set_offset(self.scroll_pos.round() as u32);

        // hover / press / click through GhostEngine's interaction model
        let (bounds, enabled) = self.screen_bounds();
        let events = self.inter.update(pointer, input.left_mouse_pressed, input.left_mouse_released, &enabled, &bounds);
        let clicked_index = events.clicked;
        let clicked = clicked_index.map(|index| self.hits[index].action);
        self.update_fades(dt);
        let pressed = self.inter.hovered().is_some_and(|i| self.inter.is_pressed(i));
        if let Some(index) = clicked_index {
            // A pointer activation also establishes the keyboard starting point,
            // so the next Tab continues from the control the user just used.
            self.keyboard_focus = Some(index);
        }
        if let Some(action) = clicked {
            self.apply(action);
        }

        self.blit(fb);
        if let Some(alpha_value) = self.page_fade_alpha() {
            fb_fill(fb, self.side_px(), 0, self.win_w - self.side_px(), self.win_h, Color::rgba(gfx::BG.r, gfx::BG.g, gfx::BG.b, alpha_value));
        }

        if clicked.is_none() {
            self.paint_fades(fb, pressed);
        }
        if let Some(index) = self.keyboard_focus {
            if let Some(hit) = self.hits.get(index) {
                if let Some(r) = self.screen_rect(hit.rect, hit.fixed) {
                    fill_rr(fb, r, hit.radius, Color::rgba(185, 162, 255, 22));
                    stroke_rr(fb, r, hit.radius, Color::rgba(222, 208, 255, 245));
                    stroke_rr(fb, inset(r, 1), hit.radius.saturating_sub(1), Color::rgba(185, 162, 255, 140));
                }
            }
        }

        // overlay scrollbar, visible while the pointer is over the content or the page is moving
        let moving = (self.scroll_target - self.scroll_pos).abs() > 0.5;
        if self.scroll.is_scrollable() && (in_main || moving) {
            let (vh, max) = (self.win_h as f32, self.scroll.max_offset() as f32);
            let thumb = ((vh * vh / (vh + max)) as u32).max(48);
            let travel = (self.win_h as u32).saturating_sub(thumb + 8);
            let y = 4 + (self.scroll.offset() as f32 / max.max(1.0) * travel as f32) as u32;
            fill_rr(fb, Rect::new(self.win_w as u32 - 10, y, 6, thumb), 3, Color::rgba(185, 162, 255, 120));
        }
    }

    fn draw_toast(&self, fb: &mut Framebuffer) {
        let Some(toast) = &self.toast else { return };
        let age = toast.born.elapsed().as_secs_f32();
        let opacity = toast_opacity(age, toast.life());
        if opacity <= 0.0 {
            return;
        }
        let mut probe = Painter::new(1, 1, self.fonts);
        let scale = self.lay.scale;
        let logical_w = (self.win_w as f32 / scale).round() as i32;
        let max_w = (logical_w - self.lay.side_w - 32).clamp(120, 540);
        let text_w = max_w - 72;
        let lines = probe.wrap(&toast.text, 13.0, W::Med, text_w).len().clamp(1, 3) as i32;
        let wide = lines > 1 || probe.width(&toast.text, 13.0, W::Med) > text_w as f32;
        let w = if wide { max_w } else { (probe.width(&toast.text, 13.0, W::Med).round() as i32 + 72).max(220) };
        let h = (lines * 20 + 28).max(56);
        let slide = ((1.0 - ease_out(age / 0.22)) * 14.0).round() as i32;
        let (x, y) = (self.lay.side_w + 12, (self.win_h as f32 / scale).round() as i32 - 12 - h + slide);
        let (accent, soft, icon) = match toast.kind {
            ToastKind::Success => (gfx::GREEN, gfx::GREEN_DARK, Icon::Check),
            ToastKind::Error => (gfx::RED, gfx::RED_DARK, Icon::Alert),
            ToastKind::Info => (gfx::VIO, gfx::ACT, Icon::Info),
        };
        let text = toast.text.clone();
        let px = |value: i32| (value as f32 * scale).round() as i32;
        self.overlay_scaled(fb, (px(x - 20), px(y - 12), px(w + 40), px(h + 40)), opacity, scale, move |p| {
            p.shadow(20, 12, w, h, 16, 14, 150);
            p.rrect(20, 12, w, h, 16, gfx::PANEL_HI);
            p.rrect_line(20, 12, w, h, 16, alpha(accent, 130));
            p.circle(20 + 34, 12 + h / 2, 15, soft);
            p.icon(icon, 20 + 34 - 8, 12 + h / 2 - 8, 16, accent, soft);
            p.paragraph(20 + 62, 12 + (h - lines * 20) / 2, text_w, 20, &text, 13.0, W::Med, gfx::TXT, 3);
        });
    }

    // ------------------------------------------------------------ stats overlay
    fn chip_rect(&self) -> Rect {
        let px = |value: i32| (value as f32 * self.lay.scale).round() as u32;
        Rect::new(self.win_w as u32 - px(12 + 250), self.win_h as u32 - px(12 + 34), px(250), px(34))
    }

    fn stats_panel_scale(&self) -> f32 {
        fit_stats_panel_scale(self.win_w.max(0) as u32, self.win_h.max(0) as u32, self.lay.scale)
    }

    fn panel_rect(&self) -> Rect {
        let scale = self.stats_panel_scale();
        let px = |value: i32| (value as f32 * scale).round() as u32;
        let x = (self.win_w as u32).saturating_sub(px(12 + PANEL_W));
        let y = (self.win_h as u32)
            .saturating_sub(px(12 + 34 + 8 + PANEL_H))
            .max(px(8));
        Rect::new(x, y, px(PANEL_W), px(PANEL_H))
    }

    fn bench_button(&self) -> Rect {
        let p = self.panel_rect();
        let px = |value: i32| (value as f32 * self.stats_panel_scale()).round() as u32;
        Rect::new(p.x + px(20), p.y + px(PANEL_H - 20 - 44 - 44), px(200), px(44))
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
            self.draw_scene(fb, &idle, false, 0.016);
        }
        let frame_avg = t.elapsed().as_secs_f64() * 1000.0 / f64::from(frames);
        self.bench = Some(Bench {
            page_avg: times.iter().sum::<f64>() / f64::from(n),
            page_min: times.iter().cloned().fold(f64::MAX, f64::min),
            page_max: times.iter().cloned().fold(0.0, f64::max),
            page_n: n,
            frame_avg,
            frame_n: frames,
        });
    }

    fn draw_overlay(&self, fb: &mut Framebuffer) {
        let avg = self.avg_frame_ms();
        let fps_text = if avg > 0.0 { format!("{:.0} img/s", 1000.0 / avg) } else { "au repos".to_owned() };
        let chip = self.chip_rect();
        let label = format!("GhostEngine · {:.1} ms · {fps_text}", self.work_ms);
        let scale = self.lay.scale;
        let px = |value: i32| (value as f32 * scale).round() as i32;
        self.overlay_scaled(fb, (chip.x as i32, chip.y as i32, px(250), px(34)), 1.0, scale, |c| {
            c.rrect(0, 0, 250, 34, 17, gfx::ACT);
            c.rrect_line(0, 0, 250, 34, 17, alpha(gfx::VIO, 200));
            c.circle(18, 17, 5, gfx::GREEN);
            c.text(32, 0, 34, &label, 13.0, W::Semi, gfx::ACT_TXT);
        });
        if !self.stats_open {
            return;
        }
        let stats = self.covers.stats();
        let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
        let fps = if avg > 0.0 { 1000.0 / avg } else { 0.0 };
        let rows = [
            ("Cadence en animation (moy. 60 frames)", if avg > 0.0 { format!("{avg:.2} ms · {fps:.0} img/s") } else { "aucune animation en cours".to_owned() }),
            ("Travail de la dernière frame", format!("{:.2} ms", self.work_ms)),
            ("Dernier rendu de page", format!("{:.0} ms", self.page_ms)),
            ("Framebuffer", format!("{}×{} RGBA · {:.1} Mo", self.win_w, self.win_h, f64::from(self.win_w * self.win_h * 4) / 1_048_576.0)),
            ("Images décodées", format!("{} fichiers · {:.0} ms", self.covers.files, self.covers.decode_ms)),
            ("Cache de textures", format!("{} entrées · {:.1} Mo", stats.entries, stats.used_bytes as f64 / 1_048_576.0)),
            ("Threads CPU disponibles", format!("{threads}")),
        ];
        let bench_label = self.bench;
        let panel = self.panel_rect();
        let panel_scale = self.stats_panel_scale();
        let panel_px = |value: i32| (value as f32 * panel_scale).round() as i32;
        let by = ((self.bench_button().y - panel.y) as f32 / panel_scale).round() as i32;
        self.overlay_scaled(fb, (panel.x as i32, panel.y as i32, panel_px(PANEL_W), panel_px(PANEL_H)), 1.0, panel_scale, |p| {
            p.shadow(0, 0, PANEL_W, PANEL_H, 18, 8, 60);
            p.rrect(0, 0, PANEL_W, PANEL_H, 18, gfx::PANEL);
            p.rrect_line(0, 0, PANEL_W, PANEL_H, 18, alpha(gfx::VIO, 200));
            p.text(20, 16, 28, "GhostEngine — mesures en direct", 17.0, W::Disp, gfx::TXT);
            let mut y = 56;
            for (k, v) in &rows {
                p.fill(20, y, PANEL_W - 40, 1, gfx::LINE);
                p.text(20, y + 1, 26, k, 12.5, W::Reg, gfx::MUT);
                p.text_right(PANEL_W - 20, y + 1, 26, v, 12.5, W::Med, gfx::TXT);
                y += 28;
            }
            y += 10;
            p.text(20, y, 18, "FONCTIONS DU MOTEUR UTILISÉES ICI", 11.0, W::Semi, gfx::MUT);
            y += 22;
            for line in [
                "Rectangles arrondis anti-aliasés, cercles, polygones",
                "Dégradés linéaires/radiaux, flou gaussien, ombres à alpha",
                "Décodage PNG/JPEG + TextureCache + mipmaps",
                "Texte OpenType (shaping) en 5 graisses",
                "ScrollViewport, UiInteraction, run_animated (patch additif)",
            ] {
                p.text(20, y, 18, line, 12.0, W::Reg, gfx::SOFT);
                y += 19;
            }
            p.rrect(20, by, 200, 44, 12, gfx::VIO);
            let label = "Lancer le benchmark";
            let tw = p.width(label, 14.0, W::Semi).round() as i32;
            p.text(20 + (200 - tw) / 2, by, 44, label, 14.0, W::Semi, gfx::VIO_DARK);
            let (l1, l2) = match bench_label {
                Some(b) => (
                    format!("Rendu de page : {:.0} ms (min {:.0}, max {:.0}, {} mesures)", b.page_avg, b.page_min, b.page_max, b.page_n),
                    format!("Frame complète : {:.2} ms soit {:.0} img/s ({} frames)", b.frame_avg, 1000.0 / b.frame_avg, b.frame_n),
                ),
                None => ("Mesure le rendu sur cette machine (≈ 1 à 3 s).".to_owned(), String::new()),
            };
            p.text(20, by + 52, 18, &l1, 12.0, W::Reg, gfx::SOFT);
            p.text(20, by + 70, 18, &l2, 12.0, W::Reg, gfx::SOFT);
        });
    }

    // -------------------------------------------------------------------- input
    fn handle_text_input(&mut self, input: &InputState) {
        if !self.search_focused && !self.store_search_focused && !self.executable_focused {
            return;
        }
        let mut changed = false;
        let searching = self.search_focused;
        let store_searching = self.store_search_focused;
        let target = if searching { &mut self.search_query } else if store_searching { &mut self.store_query } else { &mut self.executable_draft };
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
                    ghostengine::TextInputEvent::Insert(text) | ghostengine::TextInputEvent::CompositionCommit(text) => {
                        if !text.is_empty() {
                            target.push_str(text);
                            changed = true;
                        }
                    }
                    ghostengine::TextInputEvent::CompositionUpdate(_) | ghostengine::TextInputEvent::CompositionCancel => {}
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
        if searching && changed {
            self.library_limit = PAGE_SIZE;
        }
        let submitted = input.enter_pressed && (searching || store_searching);
        if submitted {
            self.search_focused = false;
            self.store_search_focused = false;
        }
        if changed || submitted {
            if submitted && store_searching {
                self.start_store_task(false);
            } else if changed && store_searching && self.page == Page::Store {
                // Typing must not repaint up to 48 game covers on every keypress.
                self.redraw_store_search_only();
            } else {
                self.render(false);
            }
        }
    }

    fn handle_tab(&mut self, input: &InputState) {
        self.keyboard_focus = if self.pending_remove.is_some() {
            next_modal_focus_index(&self.hits, self.keyboard_focus, input.shift_down)
        } else {
            next_focus_index(self.keyboard_focus, self.hits.len(), input.shift_down)
        };
        if let Some(next) = self.keyboard_focus {
            self.ensure_visible(next);
        }
    }

    /// Drains scan messages; renders at most once per call.
    fn poll_scan(&mut self) {
        let mut dirty = false;
        loop {
            let message = match self.scan_receiver.as_ref().map(|receiver| receiver.try_recv()) {
                Some(Ok(message)) => message,
                Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) => {
                    self.finish_scan_state();
                    self.notify(ToastKind::Error, "La détection des jeux s'est interrompue de manière inattendue.");
                    dirty = true;
                    break;
                }
                Some(Err(std::sync::mpsc::TryRecvError::Empty)) | None => break,
            };
            dirty = true;
            match message {
                ScanMessage::Progress(progress) => self.scan_progress = Some(progress),
                ScanMessage::Finished(outcome) => {
                    self.finish_scan_state();
                    // Deduplicate against the saved library and within this scan.
                    let mut known_platform_ids = std::collections::HashSet::new();
                    let mut known_install_paths = std::collections::HashSet::new();
                    for game in &self.games {
                        let platform = game.platform.to_ascii_lowercase();
                        if let Some(id) = game.platform_id.as_deref() {
                            known_platform_ids.insert((platform.clone(), id.to_ascii_lowercase()));
                        }
                        known_install_paths.insert(normalized_install_path(std::path::Path::new(&game.install_path)));
                    }
                    let mut detected = outcome.games;
                    detected.retain(|candidate| {
                        let platform = candidate.platform.to_ascii_lowercase();
                        let platform_id = candidate.platform_id.as_deref().map(str::to_ascii_lowercase);
                        let install_path = normalized_install_path(&candidate.install_path);
                        let known_id = platform_id.as_ref().is_some_and(|id| known_platform_ids.contains(&(platform.clone(), id.clone())));
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
                    self.scan_found = Some(count);
                    if outcome.cancelled {
                        self.notify(ToastKind::Info, format!("Détection annulée. {count} installation(s) trouvée(s) avant l'annulation."));
                    } else if count == 0 {
                        self.notify(ToastKind::Info, "Détection terminée : aucune nouvelle installation.");
                    } else {
                        self.notify(ToastKind::Success, format!("{count} installation(s) détectée(s) à importer."));
                    }
                    break;
                }
            }
        }
        if dirty {
            self.render(false);
        }
    }

    fn finish_scan_state(&mut self) {
        self.scan_receiver = None;
        self.scan_cancel = None;
        self.scan_in_progress = false;
        self.scan_progress = None;
    }

    /// Runs one frame: input -> state -> pixels. The result tells the window whether to
    /// close and whether another frame is needed right away (animation in progress).
    pub fn frame(&mut self, fb: &mut Framebuffer, input: &InputState) -> FrameOutcome {
        let start = Instant::now();
        let dt = self.last_frame.map_or(0.016, |last| start.duration_since(last).as_secs_f32().min(0.1));
        if let Some(last) = self.last_frame {
            let gap = start.duration_since(last).as_secs_f64() * 1000.0;
            // Gaps caused by waiting for input are not frame times.
            if gap < 100.0 {
                self.frame_ms.push_back(gap);
                if self.frame_ms.len() > 60 {
                    self.frame_ms.pop_front();
                }
            }
        }
        self.last_frame = Some(start);

        let (w, h) = (fb.width() as i32, fb.height() as i32);
        if w < MIN_W as i32 || h < 160 {
            return FrameOutcome::idle();
        }
        if (w, h) != (self.win_w, self.win_h) || self.content.width() != fb.width() {
            self.resize(w, h);
        }

        self.poll_library_load();
        self.poll_core_mutation();
        self.poll_store_task();
        self.poll_scan();

        // Core discovers missing executables asynchronously. Keep the visible
        // library synchronized so newly persisted paths become launchable
        // without requiring the user to close and reopen Amethys.
        if self.page == Page::Library
            && !self.library_loading
            && !self.mutation_in_progress
            && self.statuses.iter().any(|status| matches!(
                status.kind,
                status::StatusKind::NeedsExecutable | status::StatusKind::InvalidExecutable
            ))
            && start.duration_since(self.last_auto_library_refresh) >= std::time::Duration::from_secs(8)
        {
            self.last_auto_library_refresh = start;
            self.start_library_request(true);
            self.library_silent_refresh = true;
        }

        let escape = input.escape_down && !self.esc_was_down;
        self.esc_was_down = input.escape_down;
        if escape {
            if self.scan_in_progress {
                if let Some(cancel) = &self.scan_cancel {
                    cancel.store(true, Ordering::Relaxed);
                    self.notify(ToastKind::Info, "Annulation de la détection demandée…");
                }
            } else if self.pending_remove.is_some() {
                self.pending_remove = None;
                self.keyboard_focus = None;
                self.render(false);
            } else if self.editing_executable {
                // Escape discards the draft and restores the executable saved by Core.
                self.apply(Action::CancelExecutableEdit);
            } else if self.search_focused || self.store_search_focused {
                self.search_focused = false;
                self.store_search_focused = false;
                self.render(false);
            } else if self.stats_open {
                self.stats_open = false;
            } else if self.page == Page::StoreProduct {
                self.apply(Action::BackToStore);
            } else if self.page == Page::Product {
                self.apply(Action::Go(Page::Library));
            } else {
                return FrameOutcome::quit();
            }
        }

        let had_text_focus = self.search_focused || self.executable_focused;
        self.handle_text_input(input);
        if input.tab_pressed {
            if self.search_focused {
                // Leave the search field and move focus to the next actionable control
                // in the same key press, rather than requiring a second Tab.
                self.search_focused = false;
                self.render(false);
                self.handle_tab(input);
            } else if self.executable_focused {
                // Keep edit mode active while moving keyboard focus from the text field
                // to Save (Tab) or Cancel (Shift+Tab).
                self.executable_focused = false;
                self.render(false);
                let target = if input.shift_down { Action::CancelExecutableEdit } else { Action::SaveExecutable };
                if let Some(index) = self.hits.iter().position(|hit| hit.action == target) {
                    self.keyboard_focus = Some(index);
                    self.ensure_visible(index);
                }
            } else if !had_text_focus {
                self.handle_tab(input);
            }
        }
        if input.enter_pressed && self.executable_focused {
            // Enter submits the executable field just like the Save button.
            self.apply(Action::SaveExecutable);
        } else if input.enter_pressed && !had_text_focus && !self.search_focused && !self.executable_focused {
            if let Some(action) = self.keyboard_focus.and_then(|i| self.hits.get(i)).map(|h| h.action) {
                self.apply(action);
            }
        }

        let pointer = input.mouse_position;
        let over_chip = pointer.is_some_and(|p| self.chip_rect().contains(p));
        let over_panel = self.stats_open && pointer.is_some_and(|p| self.panel_rect().contains(p));
        let mut bench_now = false;
        if input.left_mouse_pressed {
            if over_chip {
                self.stats_open = !self.stats_open;
            } else if over_panel && pointer.is_some_and(|p| self.bench_button().contains(p)) {
                bench_now = true;
            }
        }
        self.draw_scene(fb, input, over_chip || over_panel, dt);
        if bench_now {
            self.run_bench(fb);
            self.draw_scene(fb, &InputState::default(), false, 0.016);
        }
        self.draw_toast(fb);
        if self.toast.as_ref().is_some_and(|t| t.born.elapsed().as_secs_f32() > t.life()) {
            self.toast = None;
        }
        self.draw_overlay(fb);
        if !bench_now {
            self.work_ms = start.elapsed().as_secs_f64() * 1000.0;
        }

        let animating = self.fades_animating()
            || (self.scroll_target - self.scroll_pos).abs() > 0.5
            || self.page_fade_alpha().is_some()
            || self.toast.is_some()
            || self.scan_in_progress
            || self.library_loading
            || self.mutation_in_progress;
        if animating {
            // The window loop does not wait for vsync: keep animation near 60 images/s.
            if let Some(rest) = FRAME_BUDGET.checked_sub(start.elapsed()) {
                std::thread::sleep(rest);
            }
            FrameOutcome::animating()
        } else {
            FrameOutcome::idle()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogue_store_urls_only_allow_http_and_https_hosts() {
        assert_eq!(validate_external_url("https://store.example/game").unwrap(), "https://store.example/game");
        assert_eq!(validate_external_url("http://store.example").unwrap(), "http://store.example");
        for url in [
            "javascript:alert(1)",
            "file:///etc/passwd",
            "https:///missing-host",
            "https://",
            "https://bad host/path",
            "https://bad\\host/path",
            "https://?query=only",
        ] {
            assert!(validate_external_url(url).is_err(), "unexpectedly accepted {url}");
        }
    }

    #[test]
    fn stats_panel_scale_fits_compact_and_large_windows() {
        let compact = fit_stats_panel_scale(360, 360, 0.96);
        assert!(compact * PANEL_W as f32 <= 340.0);
        assert!(compact * PANEL_H as f32 <= 298.0);
        assert!(compact >= 0.1);

        let regular = fit_stats_panel_scale(1440, 900, 1.08);
        assert!((regular - 1.08).abs() < f32::EPSILON);
    }

    #[test]
    fn desktop_preferences_default_and_reject_unknown_values() {
        let root = std::env::temp_dir().join(format!("amethys-prefs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let path = root.join("preferences");
        assert_eq!(load_library_sort(&path), 0);
        std::fs::create_dir_all(&root).expect("test directory");
        std::fs::write(&path, "schema=2\nlibrary_sort=2\n").expect("future schema");
        assert_eq!(load_library_sort(&path), 0);
        std::fs::write(&path, "schema=1\nlibrary_sort=9\n").expect("invalid sort");
        assert_eq!(load_library_sort(&path), 0);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn desktop_preferences_round_trip_atomically() {
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).expect("clock").as_nanos();
        let root = std::env::temp_dir().join(format!("amethys-prefs-{}-{nonce}", std::process::id()));
        let path = root.join("launcher-preferences-v1");
        save_library_sort(&path, 2).expect("preferences save");
        assert_eq!(load_library_sort(&path), 2);
        save_library_sort(&path, 1).expect("preferences replacement");
        assert_eq!(load_library_sort(&path), 1);
        assert!(!path.with_extension("bak").exists(), "successful replacement cleans up its backup");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn desktop_preferences_restore_backup_after_interrupted_replacement() {
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).expect("clock").as_nanos();
        let root = std::env::temp_dir().join(format!("amethys-prefs-recovery-{}-{nonce}", std::process::id()));
        std::fs::create_dir_all(&root).expect("test directory");
        let path = root.join("launcher-preferences-v1");
        std::fs::write(path.with_extension("bak"), "schema=1\nlibrary_sort=2\n").expect("backup");
        assert_eq!(load_library_sort(&path), 2);
        assert!(path.exists(), "the backup is restored to the primary preference path");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn keyboard_focus_is_restored_by_action_after_hits_are_reordered() {
        let hits = [
            Hit {
                rect: Rect::new(0, 0, 10, 10),
                radius: 0,
                action: Action::Go(Page::Settings),
                fixed: false,
                hover: Hover::Soft,
            },
            Hit {
                rect: Rect::new(0, 0, 10, 10),
                radius: 0,
                action: Action::FocusLibrarySearch,
                fixed: false,
                hover: Hover::Soft,
            },
        ];
        assert_eq!(focus_index_for_action(&hits, Action::FocusLibrarySearch), Some(1));
        assert_eq!(focus_index_for_action(&hits, Action::Go(Page::Settings)), Some(0));
        assert_eq!(focus_index_for_action(&hits, Action::ShowMoreGames), None);
    }

    #[test]
    fn keyboard_focus_cycles_forward_and_backward_without_losing_the_empty_state() {
        assert_eq!(next_focus_index(None, 0, false), None);
        assert_eq!(next_focus_index(None, 3, false), Some(0));
        assert_eq!(next_focus_index(None, 3, true), Some(2));
        assert_eq!(next_focus_index(Some(0), 3, false), Some(1));
        assert_eq!(next_focus_index(Some(2), 3, false), Some(0));
        assert_eq!(next_focus_index(Some(0), 3, true), Some(2));
        assert_eq!(next_focus_index(Some(2), 3, true), Some(1));
        assert_eq!(next_focus_index(Some(9), 3, false), Some(1));
    }

    #[test]
    fn modal_keyboard_focus_never_escapes_confirmation_actions() {
        let hit = |action, hover| Hit {
            rect: Rect::new(0, 0, 10, 10),
            radius: 0,
            action,
            fixed: false,
            hover,
        };
        let hits = [
            hit(Action::Go(Page::Library), Hover::Soft),
            hit(Action::CancelRemoveGame, Hover::Soft),
            hit(Action::ConfirmRemoveGame, Hover::Danger),
        ];
        assert_eq!(next_modal_focus_index(&hits, Some(1), false), Some(2));
        assert_eq!(next_modal_focus_index(&hits, Some(2), false), Some(1));
        assert_eq!(next_modal_focus_index(&hits, Some(1), true), Some(2));
        assert_eq!(next_modal_focus_index(&hits, Some(0), false), Some(1));
        assert_eq!(next_modal_focus_index(&hits, None, true), Some(2));
    }

    #[test]
    fn easing_is_monotonic_and_bounded() {
        assert_eq!(ease_out(0.0), 0.0);
        assert_eq!(ease_out(1.0), 1.0);
        assert_eq!(ease_out(5.0), 1.0);
        assert!(ease_out(0.25) < ease_out(0.5));
        assert!(ease_out(0.5) > 0.5, "ease-out starts fast");
    }

    #[test]
    fn hover_level_moves_at_constant_speed_and_never_overshoots() {
        let up = step_level(0.0, 1.0, 0.06, 0.12);
        assert!((up - 0.5).abs() < 1e-5);
        assert_eq!(step_level(0.95, 1.0, 0.5, 0.12), 1.0);
        assert_eq!(step_level(0.1, 0.0, 0.5, 0.22), 0.0);
        assert_eq!(step_level(0.4, 0.4, 0.1, 0.12), 0.4);
    }

    #[test]
    fn toast_fades_in_then_out_and_ends_transparent() {
        assert_eq!(toast_opacity(0.0, 4.0), 0.0);
        assert_eq!(toast_opacity(1.0, 4.0), 1.0);
        assert!(toast_opacity(3.9, 4.0) < 0.5);
        assert_eq!(toast_opacity(4.5, 4.0), 0.0);
    }

    #[test]
    fn errors_stay_visible_longer_than_confirmations() {
        let make = |kind| Toast { kind, text: String::new(), born: Instant::now() };
        assert!(make(ToastKind::Error).life() > make(ToastKind::Success).life());
    }

    #[test]
    fn inset_never_collapses_a_small_rectangle() {
        let tiny = Rect::new(4, 4, 2, 2);
        assert!(same_rect(inset(tiny, 1), tiny));
        let r = inset(Rect::new(10, 10, 40, 20), 1);
        assert_eq!((r.x, r.y, r.width, r.height), (11, 11, 38, 18));
    }

    #[cfg(unix)]
    #[test]
    fn unix_install_paths_preserve_case_and_backslashes() {
        use std::path::Path;
        assert_ne!(
            normalized_install_path(Path::new("/tmp/amethys-Game")),
            normalized_install_path(Path::new("/tmp/amethys-game")),
            "case-distinct Unix directories must remain distinct",
        );
        assert_ne!(
            normalized_install_path(Path::new("/tmp/amethys-Game\\Data")),
            normalized_install_path(Path::new("/tmp/amethys-Game/Data")),
            "a Unix backslash is a filename character, not a path separator",
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_install_paths_ignore_case_and_separator_style() {
        use std::path::Path;
        assert_eq!(
            normalized_install_path(Path::new(r"C:\Games\Amethys")),
            normalized_install_path(Path::new(r"c:/games/amethys/")),
        );
    }
}
