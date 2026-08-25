//! Settings popover: a small crossterm TUI run inside a herdr popup pane.
//! ↑/↓ pick a setting, ←/→ change it (applied live), Esc/q/Enter close.
use crate::config::{DragSetting, Mode, PetConfig};
use crate::sprites;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::style::{Attribute, Print, SetAttribute};
use crossterm::terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{cursor, execute, queue};
use std::io::{self, Write};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Row {
    Enabled,
    Pet,
    Mode,
    Size,
    Speed,
    Transitions,
    Quantize,
    /// Only listed in ROWS on macOS, where drag exists.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Drag,
    WarmPanes,
}

// Position is not a setting: drag the pet (macOS) or `herdr-pet corner|nudge`;
// the dropped position is remembered, like Codex's and Orca's overlays.
const ROWS: &[Row] = &[
    Row::Enabled,
    Row::Pet,
    Row::Mode,
    Row::Size,
    Row::Speed,
    Row::Transitions,
    Row::Quantize,
    #[cfg(target_os = "macos")]
    Row::Drag,
    Row::WarmPanes,
];

impl Row {
    fn label(self) -> &'static str {
        match self {
            Row::Enabled => "Enabled",
            Row::Pet => "Pet",
            Row::Mode => "Mode",
            Row::Size => "Size",
            Row::Speed => "Speed",
            Row::Transitions => "Transitions",
            Row::Quantize => "Quantize",
            Row::Drag => "Drag",
            Row::WarmPanes => "Warm panes",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Row::Enabled => "show the pet (also prefix+shift+p)",
            Row::Pet => "which pet to show (pets/ or ~/.codex/pets)",
            Row::Mode => "all: follows the focused pane · agents: one per agent pane",
            Row::Size => "height in terminal rows",
            Row::Speed => "playback speed multiplier",
            Row::Transitions => "wave on blocked, jump on done",
            Row::Quantize => "256-colour frames: ~4x less data per frame, fine for sprite art",
            Row::Drag => "hold these keys and drag anywhere to move the pet (position is remembered)",
            Row::WarmPanes => "recently focused panes that keep their pet ready",
        }
    }

    fn value(self, cfg: &PetConfig) -> String {
        match self {
            Row::Enabled => if cfg.enabled { "on" } else { "off" }.into(),
            Row::Pet => cfg.pet.clone(),
            Row::Mode => cfg.mode.as_str().into(),
            Row::Size => format!("{} rows", cfg.size),
            Row::Speed => format!("{:.2}x", cfg.speed),
            Row::Transitions => if cfg.transitions { "on" } else { "off" }.into(),
            Row::Quantize => if cfg.quantize { "on" } else { "off" }.into(),
            Row::Drag => cfg.drag.modifiers().unwrap_or("off").into(),
            Row::WarmPanes => cfg.warm_panes.to_string(),
        }
    }

    /// Apply a step (+1 / -1). Returns true when something changed.
    fn step(self, cfg: &mut PetConfig, dir: i32, pets: &[String]) -> bool {
        let before = cfg.clone();
        match self {
            Row::Enabled => cfg.enabled = !cfg.enabled,
            Row::Pet => {
                if !pets.is_empty() {
                    let idx = pets.iter().position(|n| *n == cfg.pet).unwrap_or(0) as i32;
                    cfg.pet = pets[(idx + dir).rem_euclid(pets.len() as i32) as usize].clone();
                }
            }
            Row::Mode => cfg.mode = if cfg.mode == Mode::All { Mode::Agents } else { Mode::All },
            Row::Size => cfg.size = (cfg.size as i32 + dir).clamp(3, 24) as u32,
            Row::Speed => cfg.speed = if dir > 0 { (cfg.speed * 1.25).min(4.0) } else { (cfg.speed / 1.25).max(0.25) },
            Row::Transitions => cfg.transitions = !cfg.transitions,
            Row::Quantize => cfg.quantize = !cfg.quantize,
            Row::Drag => {
                cfg.drag = if cfg.drag.modifiers().is_some() {
                    DragSetting::Enabled(false)
                } else {
                    DragSetting::Modifiers("control+option".into())
                }
            }
            Row::WarmPanes => cfg.warm_panes = (cfg.warm_panes as i32 + dir).clamp(1, 16) as usize,
        }
        *cfg != before
    }
}

fn draw(out: &mut impl Write, cfg: &PetConfig, selected: usize) -> io::Result<()> {
    let (cols, _) = terminal::size().unwrap_or((60, 20));
    let width = cols.max(30) as usize;
    queue!(out, cursor::MoveTo(0, 0), Clear(ClearType::All))?;
    queue!(out, SetAttribute(Attribute::Bold), Print(" herdr-pet settings"), SetAttribute(Attribute::Reset))?;
    for (i, row) in ROWS.iter().enumerate() {
        let y = 2 + i as u16;
        let marker = if i == selected { "›" } else { " " };
        let line = format!("{marker} {:<12} ‹ {} ›", row.label(), row.value(cfg));
        queue!(out, cursor::MoveTo(0, y))?;
        if i == selected {
            queue!(out, SetAttribute(Attribute::Reverse), Print(format!("{line:<width$}")), SetAttribute(Attribute::Reset))?;
        } else {
            queue!(out, Print(line))?;
        }
    }
    let hint_y = 2 + ROWS.len() as u16 + 1;
    queue!(out, cursor::MoveTo(0, hint_y), SetAttribute(Attribute::Dim), Print(format!(" {}", ROWS[selected].hint())))?;
    queue!(out, cursor::MoveTo(0, hint_y + 1), Print(" ↑↓ select   ←→ change   esc close"), SetAttribute(Attribute::Reset))?;
    out.flush()
}

/// Runs the settings UI on the current terminal. `apply` persists + signals the daemon.
pub fn run(apply: &dyn Fn(&PetConfig)) -> io::Result<()> {
    let pets: Vec<String> = sprites::list_pets().into_iter().map(|(name, _, _)| name).collect();
    let mut cfg = PetConfig::load().map_err(io::Error::other)?;
    let mut selected = 0usize;
    let mut out = io::stdout();
    terminal::enable_raw_mode()?;
    execute!(out, EnterAlternateScreen, cursor::Hide)?;
    let result = (|| -> io::Result<()> {
        loop {
            draw(&mut out, &cfg, selected)?;
            let Event::Key(KeyEvent { code, modifiers, kind, .. }) = event::read()? else { continue };
            if kind == KeyEventKind::Release {
                continue;
            }
            let dir = match code {
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => break,
                KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => break,
                KeyCode::Up | KeyCode::Char('k') => {
                    selected = (selected + ROWS.len() - 1) % ROWS.len();
                    continue;
                }
                KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                    selected = (selected + 1) % ROWS.len();
                    continue;
                }
                KeyCode::Left | KeyCode::Char('h') | KeyCode::Char('-') => -1,
                KeyCode::Right | KeyCode::Char('l') | KeyCode::Char('+') | KeyCode::Char(' ') => 1,
                _ => continue,
            };
            // Change the file's current contents, not our snapshot: the daemon
            // may have written a dragged `position` since the popover opened.
            let Ok(mut fresh) = PetConfig::load() else { continue };
            if ROWS[selected].step(&mut fresh, dir, &pets) {
                apply(&fresh);
            }
            cfg = fresh;
        }
        Ok(())
    })();
    execute!(out, cursor::Show, LeaveAlternateScreen)?;
    terminal::disable_raw_mode()?;
    result
}
