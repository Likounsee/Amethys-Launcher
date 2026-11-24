#![cfg_attr(windows, windows_subsystem = "windows")]

//! Amethys Launcher desktop application using the temporary GhostEngine window backend.
//!
//! Run with: cargo run --release --manifest-path prototypes/ghostengine-amethys-preview/Cargo.toml --bin amethys-launcher
//! Wheel = scroll; the sidebar switches pages. The Library page loads real Core entries,
//! refreshes the library, safely launches configured executables and removes local entries.
//! Tab / Shift+Tab / Enter navigate with the keyboard; Esc cancels, goes back, then quits.
//!
//! `--snapshot DIR` replays a scripted session and writes PNG captures without opening a
//! window. `--size WxH` sets the capture size (default 1440x900).

use amethyst_preview::app::{App, WIN_H, WIN_W};
use amethyst_preview::core_client;
use amethyst_preview::covers::CoverStore;
use amethyst_preview::gfx::{save_png, Action, Fonts, Page, BG};
use ghostengine::InputState;
use ghostengine::{Framebuffer, NativeWindowSurface, Point};
use std::path::PathBuf;

const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets");

type SnapshotResult = Result<(), Box<dyn std::error::Error>>;
type SnapshotShot<'a> = dyn FnMut(&mut App, &str, InputState) -> SnapshotResult + 'a;

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn parse_size(text: &str) -> Option<(u32, u32)> {
    let (w, h) = text.split_once(['x', 'X'])?;
    Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let snapshot = arg_value(&args, "--snapshot");
    let (width, height) = arg_value(&args, "--size").and_then(|s| parse_size(&s)).unwrap_or((WIN_W, WIN_H));
    let font_dir = args
        .iter()
        .enumerate()
        .filter(|(i, a)| {
            !a.starts_with("--")
                && args.get(i.wrapping_sub(1)).is_none_or(|p| p != "--snapshot" && p != "--size")
        })
        .map(|(_, a)| a.clone())
        .next()
        .unwrap_or_else(|| format!("{ASSETS}/fonts"));
    // The window closure must be 'static: leak the fonts for the process lifetime.
    let assets_dir = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.join("assets")))
        .filter(|path| path.is_dir())
        .unwrap_or_else(|| PathBuf::from(ASSETS));
    let fonts_dir = if font_dir != format!("{ASSETS}/fonts") {
        PathBuf::from(font_dir)
    } else {
        assets_dir.join("fonts")
    };
    let fonts_path = fonts_dir.to_string_lossy().into_owned();
    let fonts: &'static Fonts = Box::leak(Box::new(Fonts::load(&fonts_path)?));
    let media_store: Option<&'static amethys_media::MediaStore> = amethys_config::data_directory()
        .ok()
        .map(|root| Box::leak(Box::new(amethys_media::MediaStore::new(root))) as &'static amethys_media::MediaStore);
    if let Some(store) = media_store {
        if let Err(error) = store.prepare() {
            eprintln!("Could not prepare Amethys media directories: {error}");
        }
    }
    let media_library_dir = media_store.map(|store| store.library_root());
    let covers: &'static mut CoverStore = Box::leak(Box::new(CoverStore::load_with_library(
        &assets_dir.join("covers-disabled"), // pas de PNG fournis : jaquettes dessinées par le moteur
        media_library_dir.as_deref(),
    )));
    // Headless snapshots need a deterministic, fully loaded library before replay.
    // The interactive desktop starts immediately and retrieves Core data in the background.
    let mut app = if snapshot.is_some() {
        let library = core_client::list_games_or_start_agent();
        App::with_size(fonts, covers, media_store, library, width, height)
    } else {
        let mut app = App::with_size(fonts, covers, media_store, Ok(Vec::new()), width, height);
        app.start_library_load();
        app
    };

    if let Some(dir) = snapshot {
        let dir = PathBuf::from(dir);
        return match replay(&mut app, dir.clone(), width, height) {
            Ok(()) => Ok(()),
            Err(error) => {
                let _ = std::fs::create_dir_all(&dir);
                let _ = std::fs::write(dir.join("snapshot-error.txt"), error.to_string());
                Err(error)
            }
        };
    }

    NativeWindowSurface::run_animated("Amethys Launcher", width, height, move |fb, input, _clipboard| {
        app.frame(fb, &input)
    })?;
    Ok(())
}

fn at(p: Point) -> InputState {
    InputState { mouse_position: Some(p), ..Default::default() }
}

fn press(p: Point) -> InputState {
    InputState { left_mouse_down: true, left_mouse_pressed: true, ..at(p) }
}

fn release(p: Point) -> InputState {
    InputState { left_mouse_released: true, ..at(p) }
}

/// Scripted session using the same `App::frame` the window calls.
fn replay(app: &mut App, dir: PathBuf, width: u32, height: u32) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(&dir)?;
    let mut fb = Framebuffer::new(width, height, BG)?;
    // One frame with the real input, then idle frames at the same pointer position until
    // every animation (hover fade, page fade, toast slide) has settled.
    let mut shot = |app: &mut App, name: &str, input: InputState| -> SnapshotResult {
        let pointer = input.mouse_position;
        app.frame(&mut fb, &input);
        let rest = InputState { mouse_position: pointer, ..Default::default() };
        for _ in 0..40 {
            if !app.frame(&mut fb, &rest).redraw_soon {
                break;
            }
        }
        save_png(&fb, height, &dir.join(format!("{name}.png")))?;
        Ok(())
    };
    let click = |app: &mut App, shot: &mut SnapshotShot<'_>, name: &str, target: Option<Point>| -> Result<bool, Box<dyn std::error::Error>> {
        let Some(point) = target else { return Ok(false) };
        shot(app, &format!("{name}-press"), press(point))?;
        shot(app, name, release(point))?;
        Ok(true)
    };

    shot(app, "01-bibliotheque", at(Point::new(width as i32 / 2, 40)))?;
    let card = app.find_hit(|a| matches!(a, Action::SelectGame(_)));
    if let Some(p) = card {
        shot(app, "02-survol-carte", at(p))?;
    }
    shot(app, "03-defilement", InputState { scroll_delta_y: 600, ..at(Point::new(width as i32 / 2, height as i32 / 2)) })?;
    shot(app, "04-defilement-conserve", at(Point::new(width as i32 / 2, height as i32 / 2)))?;
    shot(app, "05-haut", InputState { scroll_delta_y: -4000, ..at(Point::new(width as i32 / 2, height as i32 / 2)) })?;

    let filter = app.find_hit(|a| matches!(a, Action::LibFilter(1)));
    click(app, &mut shot, "06-filtre-plateforme", filter)?;
    let sort = app.find_hit(|a| matches!(a, Action::SetLibrarySort(2)));
    click(app, &mut shot, "07-tri-etat", sort)?;
    let all = app.find_hit(|a| matches!(a, Action::LibFilter(0)));
    click(app, &mut shot, "08-filtre-tous", all)?;

    let search = app.find_hit(|a| matches!(a, Action::FocusLibrarySearch));
    if click(app, &mut shot, "09-recherche-focus", search)? {
        let typed = InputState { pressed_characters: "zzzz".chars().collect(), ..Default::default() };
        shot(app, "10-recherche-sans-resultat", typed)?;
        let clear = app.find_hit(|a| matches!(a, Action::ClearSearch));
        click(app, &mut shot, "11-recherche-effacee", clear)?;
    }

    let card = app.find_hit(|a| matches!(a, Action::SelectGame(_)));
    click(app, &mut shot, "12-fiche", card)?;
    let edit = app.find_hit(|a| matches!(a, Action::EditExecutable));
    click(app, &mut shot, "13-fiche-edition", edit)?;
    let cancel = app.find_hit(|a| matches!(a, Action::CancelExecutableEdit));
    click(app, &mut shot, "14-fiche-annuler", cancel)?;
    let remove = app.find_hit(|a| matches!(a, Action::RemoveGame(_)));
    click(app, &mut shot, "15-dialogue-retrait", remove)?;
    let keep = app.find_hit(|a| matches!(a, Action::CancelRemoveGame));
    click(app, &mut shot, "16-dialogue-annule", keep)?;
    let back = app.find_hit(|a| matches!(a, Action::Go(Page::Library)));
    click(app, &mut shot, "17-retour-bibliotheque", back)?;

    let settings = app.find_hit(|a| matches!(a, Action::Go(Page::Settings)));
    click(app, &mut shot, "18-parametres", settings)?;
    let store = app.find_hit(|a| matches!(a, Action::Go(Page::Store)));
    click(app, &mut shot, "19-boutique", store)?;
    let library = app.find_hit(|a| matches!(a, Action::Go(Page::Library)));
    click(app, &mut shot, "20-bibliotheque-retour", library)?;

    // Keyboard navigation: two Tab presses, then Enter on the focused element.
    shot(app, "21-clavier-tab", InputState { tab_pressed: true, ..Default::default() })?;
    shot(app, "22-clavier-tab-2", InputState { tab_pressed: true, ..Default::default() })?;

    // Stats overlay (the interactive benchmark is not run during captures).
    let chip = Point::new(width as i32 - 12 - 125, height as i32 - 12 - 17);
    click(app, &mut shot, "23-stats", Some(chip))?;
    Ok(())
}
