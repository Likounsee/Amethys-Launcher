//! Painting layer on top of GhostEngine's CPU primitives.
//! Only functions that GhostEngine exports are used here: no second renderer.
//!
//! This module owns the visual language of Amethys: colour tokens, the icon
//! set, shadows, gradients, text helpers and the hit regions that make a drawn
//! page interactive.

use crate::covers::CoverStore;
use ghostengine::rasterizer::fill_rect;
use ghostengine::{
    apply_gaussian_blur, draw_mipmapped_texture_clipped, draw_text, draw_texture_region_clipped,
    fill_circle_aa, fill_polygon_aa, fill_rect_linear_gradient_parallel,
    fill_rect_radial_gradient_parallel, Color, EffectScratch, FontFace, Framebuffer, Gradient,
    GradientStop, Point, Rect, TextureFilter, Vector,
};

// ---------------------------------------------------------------- colour tokens
/// Window background, the deepest layer.
pub const BG: Color = Color::rgb(0x0B, 0x0A, 0x11);
/// Sidebar surface.
pub const SIDE: Color = Color::rgb(0x0E, 0x0C, 0x15);
/// Cards and panels.
pub const PANEL: Color = Color::rgb(0x15, 0x12, 0x1E);
/// Elevated surface (inputs, raised rows).
pub const PANEL_HI: Color = Color::rgb(0x1D, 0x19, 0x29);
/// Hairline borders.
pub const LINE: Color = Color::rgb(0x27, 0x22, 0x36);
pub const TXT: Color = Color::rgb(0xF3, 0xF0, 0xFA);
pub const SOFT: Color = Color::rgb(0xCF, 0xC8, 0xE0);
pub const MUT: Color = Color::rgb(0x9A, 0x93, 0xAE);
pub const FAINT: Color = Color::rgb(0x6C, 0x66, 0x82);
/// Brand accent (amethyst).
pub const VIO: Color = Color::rgb(0xB5, 0x9B, 0xFF);
pub const VIO_HI: Color = Color::rgb(0xCD, 0xBC, 0xFF);
pub const VIO_LO: Color = Color::rgb(0x8E, 0x6C, 0xF0);
/// Ink drawn on top of the accent colour.
pub const VIO_DARK: Color = Color::rgb(0x14, 0x0B, 0x2C);
/// Selected navigation / tinted accent surface.
pub const ACT: Color = Color::rgb(0x2A, 0x21, 0x45);
pub const ACT_TXT: Color = Color::rgb(0xDC, 0xD0, 0xFF);
pub const GREEN: Color = Color::rgb(0x52, 0xDD, 0xA6);
pub const GREEN_DARK: Color = Color::rgb(0x0C, 0x2A, 0x20);
pub const AMBER: Color = Color::rgb(0xF5, 0xBA, 0x5C);
pub const AMBER_DARK: Color = Color::rgb(0x2B, 0x1D, 0x05);
pub const RED: Color = Color::rgb(0xF2, 0x6B, 0x7E);
pub const RED_DARK: Color = Color::rgb(0x31, 0x10, 0x18);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Store,
    Library,
    Product,
    StoreProduct,
    Settings,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Go(Page),
    LibFilter(usize),
    SetLibrarySort(usize),
    RefreshLibrary,
    ScanGames,
    CancelScan,
    ImportDetectedGame(usize),
    FocusLibrarySearch,
    ClearSearch,
    ShowMoreGames,
    EditExecutable,
    ChangeGameCover,
    ResetGameCover,
    SaveExecutable,
    CancelExecutableEdit,
    LaunchGame(usize),
    RemoveGame(usize),
    ConfirmRemoveGame,
    CancelRemoveGame,
    SelectGame(usize),
    FocusStoreSearch,
    SearchStore,
    SyncMasterCatalog,
    SelectCatalogGame(u64),
    BackToStore,
    OpenCatalogStore(usize),
}

/// How a hit region reacts when the pointer (or keyboard focus) is on it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hover {
    /// Accent outline and faint tint: buttons, pills, navigation.
    Glow,
    /// Stronger accent outline with a light wash: game cards.
    Card,
    /// Light wash only: rows and ghost buttons.
    Soft,
    /// Red tint: destructive actions.
    Danger,
}

/// A clickable region recorded while a page is drawn (page coordinates).
#[derive(Clone, Copy)]
pub struct Hit {
    pub rect: Rect,
    pub radius: u32,
    pub action: Action,
    /// Fixed to the window (sidebar) instead of scrolling with the page.
    pub fixed: bool,
    pub hover: Hover,
}

/// UI state that changes what the pages show.
#[derive(Clone, Copy, Default)]
pub struct State {
    pub lib_filter: usize,
}

// ------------------------------------------------------- fast rounded shapes
// GhostEngine's anti-aliased rounded-rectangle primitives evaluate coverage for
// every pixel of the rectangle (about 20 ms for a 1100x230 panel). Interiors
// are plain rectangles; only the four r x r corner quads need coverage maths.

/// Blends `c` over a rectangle, clipped to the framebuffer (any sign of x / y).
pub fn fb_fill(fb: &mut Framebuffer, x: i32, y: i32, w: i32, h: i32, c: Color) {
    let (fw, fh) = (fb.width() as i32, fb.height() as i32);
    let (x0, y0, x1, y1) = (x.max(0), y.max(0), (x + w).min(fw), (y + h).min(fh));
    if x1 <= x0 || y1 <= y0 || c.a == 0 {
        return;
    }
    let r = Rect::new(x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32);
    if c.a == 255 {
        fill_rect(fb, r, c);
    } else {
        ghostengine::blend_rect_clipped(fb, r, r, c);
    }
}

fn fb_blend_px(fb: &mut Framebuffer, x: i32, y: i32, c: Color, coverage: f32) {
    if x < 0 || y < 0 || coverage <= 0.0 {
        return;
    }
    let a = (f32::from(c.a) * coverage.min(1.0)).round() as u8;
    if a == 0 {
        return;
    }
    if let Some(cur) = fb.get_pixel(x as u32, y as u32) {
        fb.set_pixel(x as u32, y as u32, Color::rgba(c.r, c.g, c.b, a).blend_over(cur));
    }
}

/// Visits each pixel of the four corner quads with the distance of its centre
/// from the corner circle's centre.
fn for_corner_pixels(x: i32, y: i32, w: i32, h: i32, r: i32, mut visit: impl FnMut(i32, i32, f32)) {
    let rf = r as f32;
    for (ox, oy, cx, cy) in [
        (x, y, x + r, y + r),
        (x + w - r, y, x + w - r, y + r),
        (x, y + h - r, x + r, y + h - r),
        (x + w - r, y + h - r, x + w - r, y + h - r),
    ] {
        for j in 0..r {
            for i in 0..r {
                let (px, py) = (ox + i, oy + j);
                let (dx, dy) = (cx as f32 - (px as f32 + 0.5), cy as f32 - (py as f32 + 0.5));
                visit(px, py, (dx * dx + dy * dy).sqrt());
            }
        }
        let _ = rf;
    }
}

/// Anti-aliased filled rounded rectangle.
pub fn fb_fill_rrect(fb: &mut Framebuffer, x: i32, y: i32, w: i32, h: i32, r: i32, c: Color) {
    if w <= 0 || h <= 0 || c.a == 0 {
        return;
    }
    let r = r.max(0).min(w / 2).min(h / 2);
    if r == 0 {
        fb_fill(fb, x, y, w, h, c);
        return;
    }
    fb_fill(fb, x + r, y, w - 2 * r, h, c);
    fb_fill(fb, x, y + r, r, h - 2 * r, c);
    fb_fill(fb, x + w - r, y + r, r, h - 2 * r, c);
    let rf = r as f32;
    for_corner_pixels(x, y, w, h, r, |px, py, d| {
        fb_blend_px(fb, px, py, c, (rf - d + 0.5).clamp(0.0, 1.0));
    });
}

/// Anti-aliased one-pixel outline of a rounded rectangle.
pub fn fb_stroke_rrect(fb: &mut Framebuffer, x: i32, y: i32, w: i32, h: i32, r: i32, c: Color) {
    if w <= 1 || h <= 1 || c.a == 0 {
        return;
    }
    let r = r.max(0).min(w / 2).min(h / 2);
    fb_fill(fb, x + r, y, w - 2 * r, 1, c);
    fb_fill(fb, x + r, y + h - 1, w - 2 * r, 1, c);
    if r == 0 {
        fb_fill(fb, x, y + 1, 1, h - 2, c);
        fb_fill(fb, x + w - 1, y + 1, 1, h - 2, c);
        return;
    }
    fb_fill(fb, x, y + r, 1, h - 2 * r, c);
    fb_fill(fb, x + w - 1, y + r, 1, h - 2 * r, c);
    let rf = r as f32;
    for_corner_pixels(x, y, w, h, r, |px, py, d| {
        let outer = (rf - d + 0.5).clamp(0.0, 1.0);
        let inner = (d - (rf - 1.0) + 0.5).clamp(0.0, 1.0);
        fb_blend_px(fb, px, py, c, outer * inner);
    });
}

pub fn hex(s: &str) -> Color {
    let s = s.trim_start_matches('#');
    let v = u32::from_str_radix(s, 16).expect("hex color");
    Color::rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// Same colour with a different alpha.
pub fn alpha(c: Color, a: u8) -> Color {
    Color::rgba(c.r, c.g, c.b, a)
}

#[derive(Clone, Copy)]
pub enum W {
    Reg,
    Med,
    Semi,
    Bold,
    Disp,
}

pub struct Fonts {
    pub reg: FontFace,
    pub med: FontFace,
    pub semi: FontFace,
    pub bold: FontFace,
    pub disp: FontFace,
}

impl Fonts {
    pub fn load(dir: &str) -> Result<Self, String> {
        let load = |name: &str| -> Result<FontFace, String> {
            let path = format!("{dir}/{name}");
            let bytes = std::fs::read(&path).map_err(|e| format!("{path}: {e}"))?;
            FontFace::from_bytes(&bytes).map_err(|e| format!("{path}: {e}"))
        };
        Ok(Self {
            reg: load("Inter-Regular.otf")?,
            med: load("Inter-Medium.otf")?,
            semi: load("Inter-SemiBold.otf")?,
            bold: load("Inter-Bold.otf")?,
            disp: load("InterDisplay-Bold.otf")?,
        })
    }
    fn get(&self, w: W) -> &FontFace {
        match w {
            W::Reg => &self.reg,
            W::Med => &self.med,
            W::Semi => &self.semi,
            W::Bold => &self.bold,
            W::Disp => &self.disp,
        }
    }
}

pub fn rect(x: i32, y: i32, w: i32, h: i32) -> Rect {
    Rect::new(x.max(0) as u32, y.max(0) as u32, w.max(0) as u32, h.max(0) as u32)
}

// ------------------------------------------------------------------- icons
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    Library,
    Gem,
    Gear,
    Search,
    Refresh,
    Play,
    Back,
    Folder,
    Check,
    Alert,
    Close,
    Trash,
    Image,
    Edit,
    Info,
}

// -------------------------------------------------------------- placeholders
/// Deterministic, original placeholder colours derived from a game name.
/// No artwork is reproduced: the same name always gives the same gradient.
pub fn placeholder_colors(name: &str) -> (Color, Color) {
    let mut hash: u32 = 0x811C_9DC5;
    for byte in name.trim().to_lowercase().bytes() {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    let hue = (hash % 360) as f32;
    (hsl(hue, 0.50, 0.30), hsl((hue + 38.0) % 360.0, 0.55, 0.09))
}

fn hsl(h: f32, s: f32, l: f32) -> Color {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match h as u32 / 60 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let to_u8 = |v: f32| ((v + m).clamp(0.0, 1.0) * 255.0).round() as u8;
    Color::rgb(to_u8(r), to_u8(g), to_u8(b))
}

pub struct Painter<'a> {
    pub fb: Framebuffer,
    pub hits: Vec<Hit>,
    fonts: &'a Fonts,
    covers: Option<&'a CoverStore>,
    scratch: Framebuffer,
    scale: f32,
}

impl<'a> Painter<'a> {
    pub fn new(width: u32, height: u32, fonts: &'a Fonts) -> Self {
        Self {
            fb: Framebuffer::new(width.max(1), height.max(1), BG).expect("framebuffer"),
            hits: Vec::new(),
            fonts,
            covers: None,
            scratch: Framebuffer::new(1, 1, BG).expect("scratch"),
            scale: 1.0,
        }
    }

    /// Scales logical UI coordinates and typography into physical pixels.
    pub fn with_scale(mut self, scale: f32) -> Self {
        self.scale = if scale.is_finite() { scale.clamp(0.75, 2.0) } else { 1.0 };
        self
    }

    fn px(&self, value: i32) -> i32 { (value as f32 * self.scale).round() as i32 }
    fn pf(&self, value: f32) -> f32 { value * self.scale }

    pub fn with_covers(mut self, covers: &'a CoverStore) -> Self {
        self.covers = Some(covers);
        self
    }

    pub fn width_px(&self) -> i32 {
        self.fb.width() as i32
    }

    // ---- artwork -------------------------------------------------------
    /// Draws a decoded cover image into the rectangle (mipmapped when the aspect
    /// matches, otherwise centre-cropped with bilinear sampling). False if no image.
    #[allow(clippy::too_many_arguments)] // Explicit drawing geometry is intentional in this low-level painter API.
    pub fn draw_cover_for_game(
        &mut self,
        catalogue_game_id: &str,
        installation_id: &str,
        title: &str,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
    ) -> bool {
        let Some(store) = self.covers else { return false };
        let Some(mips) = store
            .get_for_game(catalogue_game_id, installation_id, title)
            .or_else(|| store.get(title))
        else {
            return false;
        };
        let base = &mips.levels()[0];
        let (bw, bh) = (base.width() as i64, base.height() as i64);
        let dest = rect(self.px(x), self.px(y), self.px(w), self.px(h));
        if ((w as i64 * bh) - (h as i64 * bw)).abs() * 20 <= w as i64 * bh {
            draw_mipmapped_texture_clipped(&mut self.fb, mips, dest, dest);
        } else {
            let (sw, sh) = if (bw * h as i64) > (bh * w as i64) {
                (bh * w as i64 / h as i64, bh)
            } else {
                (bw, bw * h as i64 / w as i64)
            };
            let src = Rect::new(((bw - sw) / 2) as u32, ((bh - sh) / 2) as u32, sw as u32, sh as u32);
            draw_texture_region_clipped(&mut self.fb, base, src, dest, TextureFilter::Bilinear, dest);
        }
        true
    }

    /// Original generated artwork used when a game has no cover of its own.
    pub fn placeholder_cover(&mut self, title: &str, x: i32, y: i32, w: i32, h: i32) {
        let (top, bottom) = placeholder_colors(title);
        self.gradient(x, y, w, h, 160.0, &[(0.0, top), (1.0, bottom)]);
        self.glow(x + w / 4, y + h / 5, (w.max(h) as f32 * 0.85) as i32, Color::rgba(255, 255, 255, 46), x, y, w, h);
        let initial = title
            .chars()
            .find(|c| c.is_alphanumeric())
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_default();
        if !initial.is_empty() {
            let size = (h as f32 * 0.42).max(24.0);
            let tw = self.width(&initial, size, W::Disp).round() as i32;
            self.text(x + (w - tw) / 2, y + (h - (size * 1.2) as i32) / 2 - h / 14, (size * 1.2) as i32, &initial, size, W::Disp, Color::rgba(255, 255, 255, 44));
        }
    }

    /// Real cover when available, original placeholder otherwise. True if a real cover was drawn.
    #[allow(clippy::too_many_arguments)] // Explicit drawing geometry is intentional in this low-level painter API.
    pub fn cover_or_placeholder(
        &mut self,
        catalogue_game_id: &str,
        installation_id: &str,
        title: &str,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
    ) -> bool {
        if self.draw_cover_for_game(catalogue_game_id, installation_id, title, x, y, w, h) {
            true
        } else {
            self.placeholder_cover(title, x, y, w, h);
            false
        }
    }

    /// Smooth dark band over the lower part of artwork so white titles stay readable.
    pub fn scrim(&mut self, x: i32, y: i32, w: i32, h: i32, from: f32, strength: u8) {
        let band = (h as f32 * from) as i32;
        self.gradient(
            x,
            y + h - band,
            w,
            band,
            180.0,
            &[(0.0, Color::rgba(8, 6, 14, 0)), (1.0, Color::rgba(8, 6, 14, strength))],
        );
    }

    /// Blurs a copy of the artwork into a banner background.
    #[allow(clippy::too_many_arguments)] // Explicit drawing geometry is intentional in this low-level painter API.
    pub fn blurred_cover_banner(
        &mut self,
        catalogue_game_id: &str,
        installation_id: &str,
        title: &str,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
    ) {
        let mut tile = Painter::new(self.px(w).max(1) as u32, self.px(h).max(1) as u32, self.fonts).with_scale(self.scale);
        tile.covers = self.covers;
        if !tile.draw_cover_for_game(catalogue_game_id, installation_id, title, 0, 0, w, h) {
            tile.placeholder_cover(title, 0, 0, w, h);
        }
        let mut scratch = EffectScratch::default();
        apply_gaussian_blur(&mut tile.fb, 28, &mut scratch);
        self.blit(&tile.fb, x, y);
    }

    /// Copies an opaque framebuffer into the page at (x, y).
    pub fn blit(&mut self, src: &Framebuffer, x: i32, y: i32) {
        let (sw, sh) = (src.width() as i32, src.height() as i32);
        let (x, y) = (self.px(x), self.px(y));
        let (fw, fh) = (self.fb.width() as i32, self.fb.height() as i32);
        for row in 0..sh {
            let dy = y + row;
            if dy < 0 || dy >= fh {
                continue;
            }
            let (x0, x1) = (x.max(0), (x + sw).min(fw));
            if x1 <= x0 {
                continue;
            }
            let s0 = (row * sw + (x0 - x)) as usize;
            let len = (x1 - x0) as usize;
            let d0 = (dy * fw + x0) as usize;
            self.fb.pixels_mut()[d0..d0 + len].copy_from_slice(&src.pixels()[s0..s0 + len]);
        }
    }

    #[allow(clippy::too_many_arguments)] // Explicit drawing geometry is intentional in this low-level painter API.
    pub fn hit(&mut self, x: i32, y: i32, w: i32, h: i32, radius: i32, action: Action, fixed: bool) {
        self.hit_with(x, y, w, h, radius, action, fixed, Hover::Glow);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn hit_with(&mut self, x: i32, y: i32, w: i32, h: i32, radius: i32, action: Action, fixed: bool, hover: Hover) {
        let scaled_rect = rect(self.px(x), self.px(y), self.px(w), self.px(h));
        let scaled_radius = self.px(radius).max(0) as u32;
        self.hits.push(Hit { rect: scaled_rect, radius: scaled_radius, action, fixed, hover });
    }

    // ---- shapes -------------------------------------------------------
    pub fn fill(&mut self, x: i32, y: i32, w: i32, h: i32, c: Color) {
        let (x, y, w, h) = (self.px(x), self.px(y), self.px(w), self.px(h));
        fb_fill(&mut self.fb, x, y, w, h, c);
    }

    pub fn rrect(&mut self, x: i32, y: i32, w: i32, h: i32, r: i32, c: Color) {
        let (x, y, w, h, r) = (self.px(x), self.px(y), self.px(w), self.px(h), self.px(r));
        fb_fill_rrect(&mut self.fb, x, y, w, h, r, c);
    }

    pub fn rrect_line(&mut self, x: i32, y: i32, w: i32, h: i32, r: i32, c: Color) {
        let (x, y, w, h, r) = (self.px(x), self.px(y), self.px(w), self.px(h), self.px(r));
        fb_stroke_rrect(&mut self.fb, x, y, w, h, r, c);
    }

    pub fn circle(&mut self, cx: i32, cy: i32, r: i32, c: Color) {
        let (cx, cy, r) = (self.px(cx), self.px(cy), self.px(r).max(0) as u32);
        fill_circle_aa(&mut self.fb, Point::new(cx, cy), r, c);
    }

    pub fn poly(&mut self, pts: &[(f32, f32)], c: Color) {
        let v: Vec<Point> = pts
            .iter()
            .map(|&(x, y)| Point::new(self.pf(x).round() as i32, self.pf(y).round() as i32))
            .collect();
        fill_polygon_aa(&mut self.fb, &v, c);
    }

    /// Soft drop shadow under a rounded rectangle (drawn before the shape itself).
    #[allow(clippy::too_many_arguments)] // Explicit drawing geometry is intentional in this low-level painter API.
    pub fn shadow(&mut self, x: i32, y: i32, w: i32, h: i32, r: i32, spread: i32, strength: u8) {
        for i in 0..spread {
            let t = 1.0 - i as f32 / spread as f32;
            let a = (f32::from(strength) * t * t / (spread as f32 * 0.55)).round().clamp(0.0, 255.0) as u8;
            if a == 0 {
                continue;
            }
            self.rrect(x - i, y - i + spread / 2, w + 2 * i, h + 2 * i, r + i, Color::rgba(0, 0, 0, a));
        }
    }

    /// CSS-like linear gradient (`angle` in degrees, 180 = top to bottom).
    pub fn gradient(&mut self, x: i32, y: i32, w: i32, h: i32, angle: f32, stops: &[(f32, Color)]) {
        if w <= 0 || h <= 0 {
            return;
        }
        let (x, y, w, h) = (self.px(x), self.px(y), self.px(w), self.px(h));
        let a = angle.to_radians();
        let (dx, dy) = (a.sin(), -a.cos());
        let len = (w as f32 * dx).abs() + (h as f32 * dy).abs();
        let (cx, cy) = (x as f32 + w as f32 / 2.0, y as f32 + h as f32 / 2.0);
        let start = Vector { x: cx - dx * len / 2.0, y: cy - dy * len / 2.0 };
        let end = Vector { x: cx + dx * len / 2.0, y: cy + dy * len / 2.0 };
        let stops = stops
            .iter()
            .map(|&(position, color)| GradientStop { position, color })
            .collect();
        let g = Gradient::new(stops).expect("gradient");
        fill_rect_linear_gradient_parallel(&mut self.fb, rect(x, y, w, h), start, end, &g);
    }

    /// Radial light: `color` at the centre fading to transparent, clipped to a rectangle.
    #[allow(clippy::too_many_arguments)]
    pub fn glow(&mut self, cx: i32, cy: i32, radius: i32, color: Color, x: i32, y: i32, w: i32, h: i32) {
        if w <= 0 || h <= 0 || radius <= 0 {
            return;
        }
        let (cx, cy, radius, x, y, w, h) = (self.px(cx), self.px(cy), self.px(radius), self.px(x), self.px(y), self.px(w), self.px(h));
        let g = Gradient::new(vec![
            GradientStop { position: 0.0, color },
            GradientStop { position: 1.0, color: alpha(color, 0) },
        ])
        .expect("gradient");
        fill_rect_radial_gradient_parallel(
            &mut self.fb,
            rect(x, y, w, h),
            Vector { x: cx as f32, y: cy as f32 },
            radius as f32,
            &g,
        );
    }

    /// Gradient inside a rounded rectangle; `bg` is the colour behind it.
    #[allow(clippy::too_many_arguments)]
    pub fn gradient_rrect(&mut self, x: i32, y: i32, w: i32, h: i32, r: i32, angle: f32, stops: &[(f32, Color)], bg: Color) {
        self.gradient(x, y, w, h, angle, stops);
        self.round_corners(x, y, w, h, r, bg);
    }

    /// Rounds the four corners of an already painted rectangle by blending
    /// the surrounding background colour back over the outside of the arc.
    pub fn round_corners(&mut self, x: i32, y: i32, w: i32, h: i32, r: i32, bg: Color) {
        let (x, y, w, h, r) = (self.px(x), self.px(y), self.px(w), self.px(h), self.px(r));
        let r = r.min(w / 2).min(h / 2).max(0);
        if r == 0 {
            return;
        }
        let rf = r as f32;
        for corner in 0..4 {
            let (ox, oy, cx, cy) = match corner {
                0 => (x, y, x + r, y + r),
                1 => (x + w - r, y, x + w - r, y + r),
                2 => (x, y + h - r, x + r, y + h - r),
                _ => (x + w - r, y + h - r, x + w - r, y + h - r),
            };
            for py in oy..oy + r {
                for px in ox..ox + r {
                    let mut inside = 0;
                    for sy in 0..4 {
                        for sx in 0..4 {
                            let fx = px as f32 + (sx as f32 + 0.5) / 4.0 - cx as f32;
                            let fy = py as f32 + (sy as f32 + 0.5) / 4.0 - cy as f32;
                            if fx * fx + fy * fy <= rf * rf {
                                inside += 1;
                            }
                        }
                    }
                    let outside = 16 - inside;
                    if outside == 0 || px < 0 || py < 0 {
                        continue;
                    }
                    if let Some(cur) = self.fb.get_pixel(px as u32, py as u32) {
                        let over = Color::rgba(bg.r, bg.g, bg.b, (outside * 255 / 16) as u8);
                        self.fb.set_pixel(px as u32, py as u32, over.blend_over(cur));
                    }
                }
            }
        }
    }

    // ---- icons ----------------------------------------------------------
    /// Draws an icon inside an `s`×`s` box. `bg` is the surface behind it (used for cut-outs).
    pub fn icon(&mut self, ic: Icon, x: i32, y: i32, s: i32, c: Color, bg: Color) {
        let k = s as f32 / 20.0;
        let (xf, yf) = (x as f32, y as f32);
        let px = |a: f32| xf + a * k;
        let py = |b: f32| yf + b * k;
        let pt = |a: f32, b: f32| (px(a), py(b));
        let ix = |a: f32| (xf + a * k).round() as i32;
        let iy = |b: f32| (yf + b * k).round() as i32;
        let iw = |v: f32| ((v * k).round() as i32).max(1);
        match ic {
            Icon::Library => {
                for (a, b) in [(2.0, 2.0), (11.0, 2.0), (2.0, 11.0), (11.0, 11.0)] {
                    self.rrect(ix(a), iy(b), iw(7.0), iw(7.0), iw(2.0), c);
                }
            }
            Icon::Gem => {
                self.poly(&[pt(5.0, 2.5), pt(15.0, 2.5), pt(17.5, 7.5), pt(10.0, 17.5), pt(2.5, 7.5)], c);
                self.poly(&[pt(5.0, 2.5), pt(10.0, 7.5), pt(2.5, 7.5)], alpha(Color::WHITE, 70));
                self.poly(&[pt(15.0, 2.5), pt(10.0, 7.5), pt(17.5, 7.5)], alpha(Color::BLACK, 40));
            }
            Icon::Gear => {
                self.rrect(ix(8.0), iy(0.5), iw(4.0), iw(3.5), iw(1.0), c);
                self.rrect(ix(8.0), iy(16.0), iw(4.0), iw(3.5), iw(1.0), c);
                self.rrect(ix(0.5), iy(8.0), iw(3.5), iw(4.0), iw(1.0), c);
                self.rrect(ix(16.0), iy(8.0), iw(3.5), iw(4.0), iw(1.0), c);
                self.circle(ix(10.0), iy(10.0), iw(6.5), c);
                self.circle(ix(10.0), iy(10.0), iw(3.0), bg);
            }
            Icon::Search => {
                self.circle(ix(8.5), iy(8.5), iw(6.5), c);
                self.circle(ix(8.5), iy(8.5), iw(4.6), bg);
                self.poly(&[pt(12.6, 14.0), pt(14.0, 12.6), pt(18.6, 17.2), pt(17.2, 18.6)], c);
            }
            Icon::Refresh => {
                self.circle(ix(10.0), iy(10.0), iw(7.0), c);
                self.circle(ix(10.0), iy(10.0), iw(5.0), bg);
                self.poly(&[pt(10.0, 10.0), pt(19.0, 2.5), pt(19.0, 10.0)], bg);
                self.poly(&[pt(11.5, 1.5), pt(18.5, 1.5), pt(18.5, 8.5)], c);
            }
            Icon::Play => {
                self.poly(&[pt(6.0, 3.0), pt(16.8, 10.0), pt(6.0, 17.0)], c);
            }
            Icon::Back => {
                self.poly(&[pt(12.5, 3.0), pt(14.2, 4.7), pt(8.9, 10.0), pt(14.2, 15.3), pt(12.5, 17.0), pt(5.5, 10.0)], c);
            }
            Icon::Folder => {
                self.rrect(ix(2.0), iy(3.5), iw(7.5), iw(5.0), iw(1.5), c);
                self.rrect(ix(2.0), iy(6.0), iw(16.0), iw(11.0), iw(2.0), c);
            }
            Icon::Check => {
                self.poly(&[pt(3.0, 10.5), pt(5.0, 8.5), pt(8.2, 11.7), pt(15.0, 4.8), pt(17.0, 6.8), pt(8.2, 15.7)], c);
            }
            Icon::Alert => {
                self.poly(&[pt(10.0, 2.0), pt(18.8, 17.2), pt(1.2, 17.2)], c);
                self.rrect(ix(9.0), iy(7.2), iw(2.0), iw(5.2), iw(1.0), bg);
                self.circle(ix(10.0), iy(14.5), iw(1.2), bg);
            }
            Icon::Close => {
                self.poly(&[pt(4.0, 5.4), pt(5.4, 4.0), pt(16.0, 14.6), pt(14.6, 16.0)], c);
                self.poly(&[pt(14.6, 4.0), pt(16.0, 5.4), pt(5.4, 16.0), pt(4.0, 14.6)], c);
            }
            Icon::Trash => {
                self.rrect(ix(7.5), iy(2.0), iw(5.0), iw(2.5), iw(1.0), c);
                self.rrect(ix(3.0), iy(4.0), iw(14.0), iw(2.2), iw(1.0), c);
                self.rrect(ix(4.8), iy(7.0), iw(10.4), iw(10.5), iw(2.0), c);
                self.rrect(ix(7.6), iy(9.2), iw(1.4), iw(5.6), iw(0.7), bg);
                self.rrect(ix(11.0), iy(9.2), iw(1.4), iw(5.6), iw(0.7), bg);
            }
            Icon::Image => {
                self.rrect(ix(1.5), iy(3.0), iw(17.0), iw(14.0), iw(2.5), c);
                self.rrect(ix(3.2), iy(4.7), iw(13.6), iw(10.6), iw(1.2), bg);
                self.circle(ix(7.0), iy(8.4), iw(1.7), c);
                self.poly(&[pt(3.6, 15.0), pt(8.6, 10.4), pt(11.6, 13.2), pt(13.6, 11.4), pt(16.4, 15.0)], c);
            }
            Icon::Edit => {
                self.poly(&[pt(2.5, 17.5), pt(3.4, 13.2), pt(13.6, 3.0), pt(17.0, 6.4), pt(6.8, 16.6)], c);
            }
            Icon::Info => {
                self.circle(ix(10.0), iy(10.0), iw(8.0), c);
                self.circle(ix(10.0), iy(6.4), iw(1.3), bg);
                self.rrect(ix(9.0), iy(9.0), iw(2.0), iw(6.0), iw(1.0), bg);
            }
        }
    }

    // ---- text ---------------------------------------------------------
    pub fn width(&mut self, s: &str, size: f32, w: W) -> f32 {
        let font = self.fonts.get(w);
        draw_text(&mut self.scratch, font, s, Point::new(0, 0), size, Color::WHITE).width
    }

    /// Draws one line inside a line box of height `lh` starting at `y`.
    #[allow(clippy::too_many_arguments)] // Explicit drawing geometry is intentional in this low-level painter API.
    pub fn text(&mut self, x: i32, y: i32, lh: i32, s: &str, size: f32, w: W, c: Color) -> f32 {
        let baseline = y as f32 + (lh as f32 - 1.21 * size) / 2.0 + 0.969 * size;
        let (x, baseline, size) = (self.px(x), self.px(baseline.round() as i32), self.pf(size));
        let font = self.fonts.get(w);
        draw_text(&mut self.fb, font, s, Point::new(x, baseline), size, c).width / self.scale
    }

    #[allow(clippy::too_many_arguments)] // Explicit drawing geometry is intentional in this low-level painter API.
    pub fn text_right(&mut self, right: i32, y: i32, lh: i32, s: &str, size: f32, w: W, c: Color) {
        let tw = self.width(s, size, w);
        self.text(right - tw.round() as i32, y, lh, s, size, w, c);
    }

    /// Centres one line horizontally inside [x, x + w).
    #[allow(clippy::too_many_arguments)] // Explicit drawing geometry is intentional in this low-level painter API.
    pub fn text_center(&mut self, x: i32, w: i32, y: i32, lh: i32, s: &str, size: f32, wt: W, c: Color) {
        let tw = self.width(s, size, wt).round() as i32;
        self.text(x + (w - tw) / 2, y, lh, s, size, wt, c);
    }

    /// Shortens `s` with an ellipsis so it fits in `max_w` pixels.
    pub fn ellipsize(&mut self, s: &str, size: f32, w: W, max_w: i32) -> String {
        if self.width(s, size, w) <= max_w as f32 {
            return s.to_owned();
        }
        let chars: Vec<char> = s.chars().collect();
        let (mut lo, mut hi) = (0usize, chars.len());
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            let candidate: String = chars[..mid].iter().collect::<String>() + "…";
            if self.width(&candidate, size, w) <= max_w as f32 {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        chars[..lo].iter().collect::<String>().trim_end().to_owned() + "…"
    }

    /// Shortens the middle of a path so both its root and file name stay visible.
    pub fn ellipsize_path(&mut self, s: &str, size: f32, w: W, max_w: i32) -> String {
        if self.width(s, size, w) <= max_w as f32 {
            return s.to_owned();
        }
        let chars: Vec<char> = s.chars().collect();
        let n = chars.len();
        let (mut keep, mut best) = (2usize, String::from("…"));
        while keep < n {
            let tail = keep * 2 / 3;
            let head = keep - tail;
            let candidate = chars[..head].iter().collect::<String>() + "…" + &chars[n - tail..].iter().collect::<String>();
            if self.width(&candidate, size, w) > max_w as f32 {
                break;
            }
            best = candidate;
            keep += 2;
        }
        best
    }

    /// One line, ellipsized to `max_w`. Returns the drawn width.
    #[allow(clippy::too_many_arguments)]
    pub fn text_fit(&mut self, x: i32, y: i32, lh: i32, s: &str, size: f32, w: W, c: Color, max_w: i32) -> f32 {
        let fitted = self.ellipsize(s, size, w, max_w);
        self.text(x, y, lh, &fitted, size, w, c)
    }

    pub fn wrap(&mut self, s: &str, size: f32, w: W, max_w: i32) -> Vec<String> {
        let mut lines = Vec::new();
        let mut cur = String::new();
        for word in s.split_whitespace() {
            let trial = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
            if !cur.is_empty() && self.width(&trial, size, w) > max_w as f32 {
                lines.push(std::mem::take(&mut cur));
                cur = word.to_string();
            } else {
                cur = trial;
            }
        }
        if !cur.is_empty() {
            lines.push(cur);
        }
        lines
    }

    /// Draws wrapped text and returns its height. The last visible line is
    /// ellipsized when the text does not fit in `max_lines`.
    #[allow(clippy::too_many_arguments)]
    pub fn paragraph(&mut self, x: i32, y: i32, max_w: i32, lh: i32, s: &str, size: f32, w: W, c: Color, max_lines: usize) -> i32 {
        let lines = self.wrap(s, size, w, max_w);
        let n = lines.len().min(max_lines);
        for (i, line) in lines.iter().take(n).enumerate() {
            let is_last_cut = i + 1 == n && lines.len() > n;
            if is_last_cut {
                let tail = format!("{line} …");
                let fitted = self.ellipsize(&tail, size, w, max_w);
                self.text(x, y + i as i32 * lh, lh, &fitted, size, w, c);
            } else {
                self.text(x, y + i as i32 * lh, lh, line, size, w, c);
            }
        }
        n as i32 * lh
    }

    /// Height `paragraph` would use, without drawing.
    pub fn paragraph_height(&mut self, max_w: i32, lh: i32, s: &str, size: f32, w: W, max_lines: usize) -> i32 {
        self.wrap(s, size, w, max_w).len().min(max_lines) as i32 * lh
    }
}

/// Writes the top `height` rows of a framebuffer as an RGB PNG.
pub fn save_png(fb: &Framebuffer, height: u32, path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let (w, h) = (fb.width(), height.min(fb.height()));
    let mut rgb = Vec::with_capacity((w * h * 3) as usize);
    for px in fb.pixels().iter().take((w * h) as usize) {
        rgb.extend_from_slice(&[px.r, px.g, px.b]);
    }
    let mut enc = png::Encoder::new(std::io::BufWriter::new(std::fs::File::create(path)?), w, h);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(&rgb)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_colours_are_stable_and_name_dependent() {
        assert_eq!(placeholder_colors("Hades"), placeholder_colors("  hades "));
        assert_ne!(placeholder_colors("Hades"), placeholder_colors("Celeste"));
    }

    #[test]
    fn hsl_primary_hues_round_trip() {
        let red = hsl(0.0, 1.0, 0.5);
        assert_eq!((red.r, red.g, red.b), (255, 0, 0));
        let green = hsl(120.0, 1.0, 0.5);
        assert_eq!((green.r, green.g, green.b), (0, 255, 0));
    }
}
