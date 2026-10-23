//! Settings and on-disk locations.
//!
//! Stored as TOML in the platform config directory (`~/.config/territoria`
//! on Linux).

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

/// Platform directories used by Territoria.
#[derive(Debug, Clone)]
pub struct Paths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
}

impl Paths {
    pub fn detect() -> Result<Self> {
        let dirs = directories::ProjectDirs::from("com", "Ortic", "territoria")
            .context("could not determine a home directory")?;
        Ok(Self {
            config_dir: dirs.config_dir().to_owned(),
            data_dir: dirs.data_dir().to_owned(),
        })
    }

    pub fn settings_file(&self) -> PathBuf {
        self.config_dir.join("settings.toml")
    }

    pub fn log_dir(&self) -> PathBuf {
        self.data_dir.join("logs")
    }

    /// Autosaved copies of unsaved projects.
    pub fn recovery_dir(&self) -> PathBuf {
        self.data_dir.join("recovery")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeChoice {
    #[default]
    System,
    Dark,
    Light,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: ThemeChoice,
    /// UI zoom factor on top of the OS scale factor.
    pub ui_scale: f32,
    /// Most recent first.
    pub recent_projects: Vec<PathBuf>,
    /// Folder of the last file dialog.
    pub last_dir: Option<PathBuf>,
    /// Minutes between autosaves of unsaved changes (0 = off).
    pub autosave_minutes: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            theme: ThemeChoice::System,
            ui_scale: 1.0,
            recent_projects: Vec::new(),
            last_dir: None,
            autosave_minutes: 3,
        }
    }
}

impl Settings {
    pub fn add_recent(&mut self, path: &Path) {
        self.recent_projects.retain(|p| p != path);
        self.recent_projects.insert(0, path.to_owned());
        self.recent_projects.truncate(12);
    }
}

pub fn load_toml<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}

/// Writes through a temporary file so a crash never leaves half a file.
pub fn save_toml<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let text = toml::to_string_pretty(value)?;
    let tmp = path.with_extension("toml.tmp");
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_projects_are_unique_and_capped() {
        let mut s = Settings::default();
        for i in 0..20 {
            s.add_recent(Path::new(&format!("/p/{i}.territoria")));
        }
        s.add_recent(Path::new("/p/5.territoria"));
        assert_eq!(s.recent_projects.len(), 12);
        assert_eq!(s.recent_projects[0], Path::new("/p/5.territoria"));
    }

    #[test]
    fn settings_round_trip() {
        let dir =
            std::env::temp_dir().join(format!("territoria-config-test-{}", std::process::id()));
        let path = dir.join("settings.toml");
        let s = Settings {
            ui_scale: 1.25,
            ..Default::default()
        };
        save_toml(&path, &s).unwrap();
        let back: Settings = load_toml(&path).unwrap();
        assert_eq!(back, s);
        let _ = fs::remove_dir_all(dir);
    }
}
