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

    /// Projects kept as templates for new ones.
    pub fn templates_dir(&self) -> PathBuf {
        self.data_dir.join("templates")
    }
}

/// The interface language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    #[default]
    English,
    German,
}

impl Language {
    pub const ALL: [Language; 2] = [Language::English, Language::German];

    /// The language's own name.
    pub fn label(self) -> &'static str {
        match self {
            Language::English => "English",
            Language::German => "Deutsch",
        }
    }
}

/// Classes and colours of a map kept under a name, to use again.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClassSet {
    pub name: String,
    pub scale: territoria_analysis::ColorScale,
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
    /// The interface language.
    pub language: Language,
    /// UI zoom factor on top of the OS scale factor.
    pub ui_scale: f32,
    /// Most recent first.
    pub recent_projects: Vec<PathBuf>,
    /// Folder of the last file dialog.
    pub last_dir: Option<PathBuf>,
    /// Minutes between autosaves of unsaved changes (0 = off).
    pub autosave_minutes: u32,
    /// Background map of new maps, by preset name (`None`: plain).
    pub background: Option<String>,
    /// The service that finds coordinates for addresses.
    pub geocoder: territoria_io::geocode::GeocoderSettings,
    /// The service that tells travel times by road.
    pub routing: territoria_io::routing::RoutingSettings,
    /// Colours of new plans' territories, by palette name (`None`: pastels).
    pub territory_colors: Option<String>,
    /// Company name on exported maps and reports.
    pub company: String,
    /// Logo image (PNG, JPEG or SVG) on exported maps and reports.
    pub logo: Option<PathBuf>,
    /// Class sets kept for reuse: breaks, colours and labels by name.
    pub class_sets: Vec<ClassSet>,
    /// Keyboard shortcuts changed from the defaults: command → keys like
    /// `Ctrl+Shift+S` (empty: none).
    pub shortcuts: std::collections::BTreeMap<String, String>,
    /// For background maps that need one: in place of `{key}` in tile
    /// addresses.
    #[serde(skip)]
    pub tile_key: territoria_io::ApiKey,
    /// The services whose API key is in the system keychain, so the
    /// keychain is asked only when there is something to read.
    pub keys_in_keychain: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            theme: ThemeChoice::System,
            language: Language::English,
            ui_scale: 1.0,
            recent_projects: Vec::new(),
            last_dir: None,
            autosave_minutes: 3,
            background: None,
            geocoder: Default::default(),
            routing: Default::default(),
            territory_colors: None,
            company: String::new(),
            class_sets: Vec::new(),
            logo: None,
            shortcuts: Default::default(),
            tile_key: Default::default(),
            keys_in_keychain: Vec::new(),
        }
    }
}

/// The API keys of the online services, by the name they are kept under
/// in the keychain.
pub type Keys = [(&'static str, String); 3];

impl Settings {
    pub fn keys(&self) -> Keys {
        [
            ("geocoding", self.geocoder.key.0.clone()),
            ("routing", self.routing.key.0.clone()),
            ("tiles", self.tile_key.0.clone()),
        ]
    }

    fn key_mut(&mut self, name: &str) -> Option<&mut String> {
        match name {
            "geocoding" => Some(&mut self.geocoder.key.0),
            "routing" => Some(&mut self.routing.key.0),
            "tiles" => Some(&mut self.tile_key.0),
            _ => None,
        }
    }

    /// Reads the keys kept in the keychain; the problems, if any.
    pub fn load_keys(&mut self, keychain: &dyn Keychain) -> Vec<String> {
        let mut problems = Vec::new();
        for name in self.keys_in_keychain.clone() {
            match keychain.read(&name) {
                Ok(Some(key)) => {
                    if let Some(k) = self.key_mut(&name) {
                        *k = key;
                    }
                }
                Ok(None) => {}
                Err(e) => problems.push(format!("{name}: {e}")),
            }
        }
        problems
    }

    /// Writes the keys that differ from `before` to the keychain (an empty
    /// key is removed) and notes which are kept there; the problems, if any.
    pub fn store_keys(&mut self, before: &Keys, keychain: &dyn Keychain) -> Vec<String> {
        let mut problems = Vec::new();
        for ((name, key), (_, old)) in self.keys().iter().zip(before) {
            if key == old {
                continue;
            }
            let key = key.trim();
            match keychain.write(name, key) {
                Ok(()) => {
                    self.keys_in_keychain.retain(|n| n != name);
                    if !key.is_empty() {
                        self.keys_in_keychain.push((*name).to_owned());
                    }
                }
                Err(e) => problems.push(format!("{name}: {e}")),
            }
        }
        problems
    }

    pub fn add_recent(&mut self, path: &Path) {
        self.recent_projects.retain(|p| p != path);
        self.recent_projects.insert(0, path.to_owned());
        self.recent_projects.truncate(12);
    }
}

/// Where API keys are kept: the system keychain, or memory in tests.
pub trait Keychain {
    fn read(&self, name: &str) -> Result<Option<String>, String>;
    /// Keeps `value` under `name`; an empty value removes it.
    fn write(&self, name: &str, value: &str) -> Result<(), String>;
}

/// The system keychain: macOS Keychain, Windows Credential Manager, the
/// Secret Service (GNOME Keyring, KWallet) on Linux.
pub struct SystemKeychain;

impl SystemKeychain {
    const SERVICE: &'static str = "Territoria";
}

impl Keychain for SystemKeychain {
    fn read(&self, name: &str) -> Result<Option<String>, String> {
        let entry = keyring::Entry::new(Self::SERVICE, name).map_err(|e| e.to_string())?;
        match entry.get_password() {
            Ok(p) => Ok(Some(p)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    fn write(&self, name: &str, value: &str) -> Result<(), String> {
        let entry = keyring::Entry::new(Self::SERVICE, name).map_err(|e| e.to_string())?;
        if value.is_empty() {
            match entry.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(e) => Err(e.to_string()),
            }
        } else {
            entry.set_password(value).map_err(|e| e.to_string())
        }
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

    /// A keychain in memory.
    #[derive(Default)]
    struct Memory(std::cell::RefCell<std::collections::HashMap<String, String>>);

    impl Keychain for Memory {
        fn read(&self, name: &str) -> Result<Option<String>, String> {
            Ok(self.0.borrow().get(name).cloned())
        }
        fn write(&self, name: &str, value: &str) -> Result<(), String> {
            if value.is_empty() {
                self.0.borrow_mut().remove(name);
            } else {
                self.0.borrow_mut().insert(name.into(), value.into());
            }
            Ok(())
        }
    }

    #[test]
    fn keys_live_in_the_keychain_not_the_file() {
        let keychain = Memory::default();
        let mut s = Settings::default();
        let before = s.keys();
        s.routing.key.0 = "secret-r".into();
        s.tile_key.0 = " secret-t ".into();
        assert!(s.store_keys(&before, &keychain).is_empty());
        assert_eq!(s.keys_in_keychain, ["routing", "tiles"]);
        assert_eq!(keychain.read("tiles").unwrap().as_deref(), Some("secret-t"));
        let text = toml::to_string_pretty(&s).unwrap();
        assert!(!text.contains("secret"), "{text}");
        // Read back on the next start.
        let mut back: Settings = toml::from_str(&text).unwrap();
        assert!(back.load_keys(&keychain).is_empty());
        assert_eq!(back.routing.key.0, "secret-r");
        // Emptied keys leave the keychain.
        let before = back.keys();
        back.routing.key.0.clear();
        back.store_keys(&before, &keychain);
        assert_eq!(back.keys_in_keychain, ["tiles"]);
        assert_eq!(keychain.read("routing").unwrap(), None);
    }

    /// The real keychain; needs one unlocked (`cargo test -- --ignored`).
    #[test]
    #[ignore]
    fn system_keychain_round_trip() {
        let k = SystemKeychain;
        let name = format!("test-{}", std::process::id());
        k.write(&name, "s3cret").unwrap();
        assert_eq!(k.read(&name).unwrap().as_deref(), Some("s3cret"));
        k.write(&name, "").unwrap();
        assert_eq!(k.read(&name).unwrap(), None);
    }

    #[test]
    fn settings_round_trip() {
        let dir =
            std::env::temp_dir().join(format!("territoria-config-test-{}", std::process::id()));
        let path = dir.join("settings.toml");
        let mut shortcuts = std::collections::BTreeMap::new();
        shortcuts.insert("Find".to_owned(), "Ctrl+K".to_owned());
        let s = Settings {
            ui_scale: 1.25,
            shortcuts,
            class_sets: vec![ClassSet {
                name: "Revenue bands".into(),
                scale: territoria_analysis::ColorScale {
                    method: territoria_analysis::Method::Manual,
                    manual: vec![1000.0, 5000.0],
                    labels: vec!["low".into(), "mid".into(), "high".into()],
                    no_data: Some(territoria_analysis::Rgba::rgb(1, 2, 3)),
                    ..Default::default()
                },
            }],
            ..Default::default()
        };
        save_toml(&path, &s).unwrap();
        let back: Settings = load_toml(&path).unwrap();
        assert_eq!(back, s);
        let _ = fs::remove_dir_all(dir);
    }
}
