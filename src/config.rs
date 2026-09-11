//! pet.toml: user-editable settings under HERDR_PLUGIN_CONFIG_DIR. Missing
//! keys take their defaults; unknown keys are ignored.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// One pet, on the focused pane, whatever it runs.
    All,
    /// One pet on every pane with a detected agent.
    Agents,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::All => "all",
            Mode::Agents => "agents",
        }
    }
}

/// `drag = "control+option"` enables the macOS drag with those modifiers;
/// `drag = false` disables it (`true` means the default modifiers).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DragSetting {
    Enabled(bool),
    Modifiers(String),
}

impl DragSetting {
    pub fn modifiers(&self) -> Option<&str> {
        match self {
            DragSetting::Enabled(false) => None,
            DragSetting::Enabled(true) => Some("control+option"),
            DragSetting::Modifiers(m) => Some(m.as_str()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PetConfig {
    pub enabled: bool,
    /// Pet name under <plugin>/pets or ~/.codex/pets, or a directory/file path.
    pub pet: String,
    pub mode: Mode,
    /// Pet height in terminal rows; width follows the pet's aspect ratio.
    pub size: u32,
    /// `[col, row]` of the pet's top-left cell inside the pane; written by
    /// dragging. Absent = bottom-right corner.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<[i32; 2]>,
    /// Play waving/jumping one-shots on status changes.
    pub transitions: bool,
    /// Playback speed multiplier over the pet's frame timings (1.0 = as authored).
    pub speed: f64,
    /// Encode frames as 256-colour indexed PNGs (~4x smaller, visually lossless
    /// for sprite art) instead of full-colour RGBA.
    pub quantize: bool,
    pub drag: DragSetting,
    /// Recently focused panes (in other tabs) that keep their pet ready so
    /// switching back shows it instantly.
    pub warm_panes: usize,
}

impl Default for PetConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            pet: String::new(),
            mode: Mode::All,
            size: 6,
            position: None,
            transitions: true,
            speed: 1.0,
            quantize: true,
            drag: if cfg!(target_os = "macos") {
                DragSetting::Modifiers("control+option".into())
            } else {
                DragSetting::Enabled(false)
            },
            warm_panes: 4,
        }
    }
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

pub fn plugin_root() -> PathBuf {
    if let Some(root) = std::env::var_os("HERDR_PLUGIN_ROOT") {
        return PathBuf::from(root);
    }
    // Dev fallback: the binary lives in <root>/target/<profile>/herdr-pet. Resolve
    // symlinks first (the CLI is usually invoked via ~/.local/bin/herdr-pet).
    std::env::current_exe()
        .and_then(std::fs::canonicalize)
        .ok()
        .and_then(|exe| exe.ancestors().nth(3).map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn config_dir() -> PathBuf {
    std::env::var_os("HERDR_PLUGIN_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config/herdr/plugins/config/pet"))
}

pub fn state_dir() -> PathBuf {
    std::env::var_os("HERDR_PLUGIN_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".local/state/herdr/plugins/pet"))
}

pub fn config_path() -> PathBuf {
    config_dir().join("pet.toml")
}

impl PetConfig {
    /// A missing file is the defaults; a file that fails to parse is an error so
    /// callers never overwrite a hand-edited file with defaults.
    pub fn load() -> Result<PetConfig, String> {
        let path = config_path();
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(PetConfig::default()),
            Err(e) => return Err(format!("cannot read {}: {e}", path.display())),
        };
        let mut cfg: PetConfig = toml::from_str(&text).map_err(|e| format!("invalid {}: {e}", path.display()))?;
        cfg.clamp();
        Ok(cfg)
    }

    /// Keep hand-edited values inside the ranges the rest of the code assumes.
    pub fn clamp(&mut self) {
        self.size = self.size.clamp(3, 24);
        self.warm_panes = self.warm_panes.clamp(1, 16);
        self.speed = if self.speed.is_finite() { self.speed.clamp(0.25, 4.0) } else { 1.0 };
        // `position` is deliberately not clamped: negative cells count from the
        // right/bottom edge (see daemon::apply_placement), and the placement step
        // clamps the resolved cell to the pane.
    }

    /// Atomic: write a temp file in the same directory and rename it into place,
    /// so a reader (the daemon on SIGUSR1) never sees a half-written file.
    pub fn save(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(config_dir())?;
        let body = toml::to_string(self).map_err(std::io::Error::other)?;
        let text = format!(
            "# herdr-pet settings. Edited by the settings popover / CLI (applied live);\n# hand edits take effect on `herdr-pet restart`.\n{body}"
        );
        let tmp = config_dir().join(format!(".pet.toml.{}", std::process::id()));
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, config_path())
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::path::PathBuf;
    use std::sync::Mutex;

    /// Serialises tests that touch process env vars (config/state dirs, CODEX_HOME).
    pub static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// Fresh unique directory under the OS temp dir.
    pub fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("herdr-pet-test-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{temp_dir, ENV_LOCK};
    use super::*;

    #[test]
    fn save_load_round_trip() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = temp_dir("cfg-roundtrip");
        std::env::set_var("HERDR_PLUGIN_CONFIG_DIR", &dir);
        let cfg = PetConfig {
            enabled: false,
            pet: "rook".into(),
            mode: Mode::Agents,
            size: 12,
            position: Some([7, 3]),
            transitions: false,
            speed: 2.0,
            quantize: false,
            drag: DragSetting::Enabled(false),
            warm_panes: 2,
        };
        cfg.save().unwrap();
        assert_eq!(PetConfig::load().unwrap(), cfg);
    }

    #[test]
    fn load_clamps_out_of_range_values() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = temp_dir("cfg-clamp");
        std::env::set_var("HERDR_PLUGIN_CONFIG_DIR", &dir);
        std::fs::write(
            dir.join("pet.toml"),
            "size = 200000\nwarm_panes = 0\nspeed = 99.0\nposition = [-5, -9]\n",
        )
        .unwrap();
        let cfg = PetConfig::load().unwrap();
        assert_eq!(cfg.size, 24);
        assert_eq!(cfg.warm_panes, 1);
        assert_eq!(cfg.speed, 4.0);
        // Negative cells survive the load: they are edge-relative, not garbage.
        assert_eq!(cfg.position, Some([-5, -9]));
    }

    #[test]
    fn broken_toml_is_an_error_and_untouched() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = temp_dir("cfg-broken");
        std::env::set_var("HERDR_PLUGIN_CONFIG_DIR", &dir);
        let body = "speed = 1,5\n";
        std::fs::write(dir.join("pet.toml"), body).unwrap();
        assert!(PetConfig::load().is_err());
        assert_eq!(std::fs::read_to_string(dir.join("pet.toml")).unwrap(), body);
    }

    #[test]
    fn missing_file_is_defaults_and_unknown_keys_ignored() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = temp_dir("cfg-missing");
        std::env::set_var("HERDR_PLUGIN_CONFIG_DIR", &dir);
        assert_eq!(PetConfig::load().unwrap(), PetConfig::default());
        std::fs::write(dir.join("pet.toml"), "pet = \"dewey\"\nfuture_key = true\n").unwrap();
        assert_eq!(PetConfig::load().unwrap().pet, "dewey");
    }
}
