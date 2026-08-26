//! Codex-atlas pet loader (openai/skills hatch-pet): a directory with
//! `pet.json` + a spritesheet of 8 columns x 192x208 px cells, one row per
//! state (9 rows; taller v2 sheets carry extra rows we ignore).
use image::imageops::{self, FilterType};
use image::RgbaImage;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const CELL_W: u32 = 192;
pub const CELL_H: u32 = 208;
pub const ATLAS_COLS: u32 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    Idle,
    RunningRight,
    RunningLeft,
    Waving,
    Jumping,
    Failed,
    Waiting,
    Running,
    Review,
}

impl Row {
    pub const ALL: [Row; 9] = [
        Row::Idle,
        Row::RunningRight,
        Row::RunningLeft,
        Row::Waving,
        Row::Jumping,
        Row::Failed,
        Row::Waiting,
        Row::Running,
        Row::Review,
    ];

    pub fn index(self) -> usize {
        Row::ALL.iter().position(|r| *r == self).expect("row listed")
    }

    /// Per-frame durations in ms; the length is the used frame count. State rows
    /// follow hatch-pet's animation-rows.md (= Codex TUI `model.rs`); idle uses
    /// the calm 6.6s ambient loop Codex actually ships, not the doc's 1.1s one.
    pub fn durations(self) -> &'static [u64] {
        match self {
            Row::Idle => &[1680, 660, 660, 840, 840, 1920],
            Row::RunningRight | Row::RunningLeft => &[120, 120, 120, 120, 120, 120, 120, 220],
            Row::Waving => &[140, 140, 140, 280],
            Row::Jumping => &[140, 140, 140, 140, 280],
            Row::Failed => &[140, 140, 140, 140, 140, 140, 140, 240],
            Row::Waiting => &[150, 150, 150, 150, 150, 260],
            Row::Running => &[120, 120, 120, 120, 120, 220],
            Row::Review => &[150, 150, 150, 150, 150, 280],
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Row::Idle => "idle",
            Row::RunningRight => "running-right",
            Row::RunningLeft => "running-left",
            Row::Waving => "waving",
            Row::Jumping => "jumping",
            Row::Failed => "failed",
            Row::Waiting => "waiting",
            Row::Running => "running",
            Row::Review => "review",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub id: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub spritesheet_path: Option<String>,
}

impl Manifest {
    pub fn label(&self) -> &str {
        self.display_name.as_deref().unwrap_or(&self.id)
    }
}

/// One animation: PNG-encoded frames plus per-frame holds in ms.
pub struct Track {
    pub frames: Vec<Vec<u8>>,
    pub durations: Vec<u64>,
}

pub struct Pet {
    pub manifest: Manifest,
    /// Pixel size of every frame (downscaled to the display size at load).
    pub width: u32,
    pub height: u32,
    /// One track per `Row::ALL` entry (shared when rows reuse the same loop).
    tracks: Vec<Arc<Track>>,
}

impl Pet {
    pub fn frame(&self, row: Row, index: usize) -> &[u8] {
        let frames = &self.tracks[row.index()].frames;
        &frames[index % frames.len()]
    }
    pub fn durations(&self, row: Row) -> &[u64] {
        &self.tracks[row.index()].durations
    }
}

pub fn search_dirs() -> Vec<PathBuf> {
    let codex_home = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".codex"));
    vec![crate::config::plugin_root().join("pets"), codex_home.join("pets")]
}

/// Resolve a pet name (or a directory path) to its atlas directory.
pub fn resolve_pet(name: &str) -> Option<PathBuf> {
    if name.contains('/') {
        let dir = PathBuf::from(name);
        return dir.join("pet.json").is_file().then_some(dir);
    }
    search_dirs()
        .into_iter()
        .map(|base| base.join(name))
        .find(|dir| dir.join("pet.json").is_file())
}

/// (name, dir, label) for every pet under the search dirs.
pub fn list_pets() -> Vec<(String, PathBuf, String)> {
    let mut out = Vec::new();
    for base in search_dirs() {
        let Ok(entries) = std::fs::read_dir(&base) else { continue };
        for entry in entries.flatten() {
            let dir = entry.path();
            if !dir.join("pet.json").is_file() {
                continue;
            }
            let Some(name) = dir.file_name().and_then(|n| n.to_str()).map(str::to_owned) else { continue };
            let label = std::fs::read_to_string(dir.join("pet.json"))
                .ok()
                .and_then(|t| serde_json::from_str::<Manifest>(&t).ok())
                .map(|m| m.label().to_owned())
                .unwrap_or_default();
            out.push((name, dir, label));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Display size for a wanted height: cap at the native size (never upscale).
fn fit(native_w: u32, native_h: u32, height_px: Option<u32>) -> (u32, u32) {
    let native_h = native_h.max(1);
    let height = height_px.unwrap_or(native_h).min(native_h).max(1);
    let width = ((native_w.max(1) as f64 * height as f64) / native_h as f64).round().max(1.0) as u32;
    (width, height)
}

/// Resize to the display box and PNG-encode. With `quantize`, frames become
/// 8-bit indexed PNGs (palette + tRNS): pixels decoded from lossy WebP barely
/// deflate as RGBA (~40% of raw), while a 256-colour palette is visually
/// lossless for sprite art and ~4x smaller.
fn encode_png(img: &RgbaImage, width: u32, height: u32, quantize: bool) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let resized;
    let img = if img.dimensions() == (width, height) {
        img
    } else {
        resized = imageops::resize(img, width, height, FilterType::Lanczos3);
        &resized
    };
    if !quantize {
        let mut out = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)?;
        return Ok(out);
    }
    // Entry 0 is reserved for fully transparent; quantise the visible pixels into the other 255.
    let visible: Vec<u8> = img
        .pixels()
        .filter(|p| p[3] >= 8)
        .flat_map(|p| p.0)
        .collect();
    let mut palette = vec![0u8, 0, 0];
    let mut trns = vec![0u8];
    let mut indices = Vec::with_capacity((width * height) as usize);
    if visible.is_empty() {
        indices.resize((width * height) as usize, 0);
    } else {
        let quant = color_quant::NeuQuant::new(10, 255, &visible);
        let map = quant.color_map_rgba();
        let (entries, _) = map.as_chunks::<4>();
        for entry in entries {
            palette.extend_from_slice(&entry[..3]);
            trns.push(entry[3]);
        }
        for p in img.pixels() {
            indices.push(if p[3] < 8 { 0 } else { quant.index_of(&p.0) as u8 + 1 });
        }
    }
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, width, height);
        enc.set_color(png::ColorType::Indexed);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_palette(palette);
        enc.set_trns(trns);
        enc.set_compression(png::Compression::Default);
        enc.write_header()?.write_image_data(&indices)?;
    }
    Ok(out)
}

/// Decode the atlas once, slice the used cells per row, downscale each to
/// `height_px` (capped at the native 208) and PNG-encode.
pub fn load_pet(dir: &Path, height_px: Option<u32>, quantize: bool) -> Result<Pet, Box<dyn std::error::Error>> {
    let manifest: Manifest = serde_json::from_str(&std::fs::read_to_string(dir.join("pet.json"))?)?;
    let sheet_path = dir.join(manifest.spritesheet_path.as_deref().unwrap_or("spritesheet.webp"));
    let sheet: RgbaImage = image::open(&sheet_path)
        .map_err(|e| format!("{}: {e}", sheet_path.display()))?
        .to_rgba8();
    // v1 sheets are 8x9 cells (1536x1872); v2 sheets add rows below (e.g. 11 rows,
    // 1536x2288, for the app's look-around animations). The nine standard rows
    // always come first, so accept any taller multiple and read only those.
    let (expect_w, min_h) = (CELL_W * ATLAS_COLS, CELL_H * Row::ALL.len() as u32);
    if sheet.width() != expect_w || sheet.height() < min_h || !sheet.height().is_multiple_of(CELL_H) {
        return Err(format!(
            "{}: expected a {expect_w}x{min_h}+ Codex atlas (8 columns of 192x208 cells), got {}x{}",
            sheet_path.display(),
            sheet.width(),
            sheet.height()
        )
        .into());
    }
    let (width, height) = fit(CELL_W, CELL_H, height_px);
    let mut tracks = Vec::with_capacity(Row::ALL.len());
    for row in Row::ALL {
        let durations = row.durations().to_vec();
        let mut frames = Vec::with_capacity(durations.len());
        for col in 0..durations.len() as u32 {
            let cell = imageops::crop_imm(&sheet, col * CELL_W, row.index() as u32 * CELL_H, CELL_W, CELL_H).to_image();
            frames.push(encode_png(&cell, width, height, quantize)?);
        }
        tracks.push(Arc::new(Track { frames, durations }));
    }
    Ok(Pet { manifest, width, height, tracks })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::test_support::{temp_dir, ENV_LOCK};

    #[test]
    fn fit_clamps_and_keeps_aspect() {
        assert_eq!(fit(192, 208, Some(102)), (94, 102));
        // Tiny pets must not panic (clamp(8, native_h) used to when native_h < 8).
        assert_eq!(fit(20, 4, Some(102)), (20, 4));
        assert_eq!(fit(0, 0, None), (1, 1));
        // Never upscale past native height.
        assert_eq!(fit(192, 208, Some(9999)), (192, 208));
    }

    #[test]
    fn rows_are_ordered_and_timed_like_codex() {
        assert_eq!(Row::ALL.map(|r| r.index()), core::array::from_fn::<_, 9, _>(|i| i));
        let counts: Vec<usize> = Row::ALL.iter().map(|r| r.durations().len()).collect();
        assert_eq!(counts, [6, 8, 8, 4, 5, 8, 6, 6, 6]);
        // The calm ambient idle from Codex's model.rs, not the hatch-pet doc's fast one.
        assert_eq!(Row::Idle.durations(), [1680, 660, 660, 840, 840, 1920]);
    }

    fn write_atlas(dir: &std::path::Path, rows: u32) {
        let (w, h) = (CELL_W * ATLAS_COLS, CELL_H * rows);
        let mut img = RgbaImage::new(w, h);
        for (x, y, p) in img.enumerate_pixels_mut() {
            // Opaque, varied colours so quantisation has real work to do.
            *p = image::Rgba([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8, 255]);
        }
        std::fs::create_dir_all(dir).unwrap();
        img.save(dir.join("sheet.png")).unwrap();
        std::fs::write(
            dir.join("pet.json"),
            r#"{"id":"testpet","displayName":"Test Pet","spritesheetPath":"sheet.png"}"#,
        )
        .unwrap();
    }

    #[test]
    fn loads_v1_and_v2_atlases_and_rejects_wrong_sizes() {
        let base = temp_dir("atlas");
        for (name, rows, ok) in [("v1", 9u32, true), ("v2", 11, true), ("short", 5, false)] {
            let dir = base.join(name);
            write_atlas(&dir, rows);
            let result = load_pet(&dir, Some(102), true);
            assert_eq!(result.is_ok(), ok, "{name}");
            if let Ok(pet) = result {
                assert_eq!((pet.width, pet.height), (94, 102));
                assert_eq!(pet.manifest.label(), "Test Pet");
                let frame = image::load_from_memory(pet.frame(Row::Idle, 0)).unwrap();
                assert_eq!((frame.width(), frame.height()), (94, 102));
                // Out-of-range frame indices wrap instead of panicking.
                let _ = pet.frame(Row::Waving, 99);
            }
        }
    }

    #[test]
    fn indexed_png_round_trips_transparency() {
        let mut img = RgbaImage::new(40, 30);
        for (x, y, p) in img.enumerate_pixels_mut() {
            *p = if x < 20 {
                image::Rgba([0, 0, 0, 0])
            } else {
                image::Rgba([(x * 6) as u8, (y * 8) as u8, 200, 255])
            };
        }
        for quantize in [true, false] {
            let png = encode_png(&img, 40, 30, quantize).unwrap();
            let back = image::load_from_memory(&png).unwrap().to_rgba8();
            assert_eq!(back.dimensions(), (40, 30));
            assert_eq!(back.get_pixel(0, 0)[3], 0, "transparent stays transparent");
            assert_eq!(back.get_pixel(39, 0)[3], 255, "opaque stays opaque");
        }
    }

    #[test]
    fn resolves_by_name_and_path_and_lists_dotted_names() {
        let _guard = ENV_LOCK.lock().unwrap();
        let codex = temp_dir("resolve-codex");
        let root = temp_dir("resolve-root");
        std::env::set_var("CODEX_HOME", &codex);
        std::env::set_var("HERDR_PLUGIN_ROOT", &root);
        let pets = codex.join("pets");
        write_atlas(&pets.join("pikachu.v2"), 9);
        std::fs::create_dir_all(pets.join("not-a-pet")).unwrap();

        assert_eq!(resolve_pet("pikachu.v2"), Some(pets.join("pikachu.v2")));
        assert_eq!(resolve_pet("missing"), None);
        let by_path = pets.join("pikachu.v2");
        assert_eq!(resolve_pet(by_path.to_str().unwrap()), Some(by_path));

        let names: Vec<String> = list_pets().into_iter().map(|(n, _, _)| n).collect();
        assert!(names.contains(&"pikachu.v2".to_string()), "{names:?}");
        assert!(!names.contains(&"not-a-pet".to_string()));
    }
}
