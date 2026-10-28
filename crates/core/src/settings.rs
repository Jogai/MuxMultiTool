//! User settings the server keeps for its clients: the theme for now.
//!
//! The server stores what the client sends and gives it back; it does not
//! know the theme palettes, so the frontend validates the values and falls
//! back to its default for anything it does not recognize. The
//! [`SettingsStore`] keeps the settings in memory and, when given a folder,
//! in `settings.json` there, written atomically on every update.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tracing::{info, warn};

/// File name of the settings in the data folder.
pub const SETTINGS_FILE: &str = "settings.json";

/// Whether one theme variant is used, or a light and a dark one that follow
/// the OS setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
	/// One variant, whatever the OS says.
	#[default]
	Single,
	/// `light_variant` when the OS is light, `dark_variant` when it is dark.
	Auto,
}

/// The colour theme: a family and, within it, one variant or a light and a
/// dark one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ThemeSetting {
	/// Family id, e.g. `catppuccin`.
	pub family: String,
	/// One variant, or light and dark following the OS.
	pub mode: ThemeMode,
	/// Variant id used in [`ThemeMode::Single`], e.g. `mocha`.
	pub variant: String,
	/// Variant id used in [`ThemeMode::Auto`] when the OS is light.
	pub light_variant: String,
	/// Variant id used in [`ThemeMode::Auto`] when the OS is dark.
	pub dark_variant: String,
}

/// Every setting; `None` means the user never chose one. An update is also
/// a `Settings`, where `None` means "keep what is stored".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Settings {
	/// The colour theme.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub theme: Option<ThemeSetting>,
}

impl Settings {
	/// Takes over every field that is set in `patch`.
	pub fn merge(&mut self, patch: Settings) {
		if let Some(theme) = patch.theme {
			self.theme = Some(theme);
		}
	}
}

/// What can go wrong when the settings file is read or written.
#[derive(Debug, thiserror::Error)]
pub enum Error {
	/// The file or its folder could not be read or written.
	#[error("cannot access {path}: {source}")]
	Io {
		/// The file or folder.
		path: PathBuf,
		/// The underlying error.
		#[source]
		source: io::Error,
	},
	/// The file holds no valid settings.
	#[error("{path} holds no valid settings: {source}")]
	Format {
		/// The file.
		path: PathBuf,
		/// The underlying error.
		#[source]
		source: serde_json::Error,
	},
}

/// Keeps the settings, in memory and optionally in a file.
#[derive(Debug)]
pub struct SettingsStore {
	settings: Mutex<Settings>,
	/// The file, or `None` for a store that lives as long as the process.
	path: Option<PathBuf>,
}

impl SettingsStore {
	/// A store that forgets everything when the process ends. The frontend
	/// keeps its own copy, so a page reload still restores the theme.
	#[must_use]
	pub fn in_memory() -> Self {
		Self {
			settings: Mutex::new(Settings::default()),
			path: None,
		}
	}

	/// A store backed by `settings.json` in `data_dir`, with the file's
	/// contents when it exists. The folder is created on the first write.
	///
	/// # Errors
	/// Fails when the file exists but cannot be read or parsed; a missing
	/// file is an empty store.
	pub fn in_dir(data_dir: &Path) -> Result<Self, Error> {
		let path = data_dir.join(SETTINGS_FILE);
		let settings = match std::fs::read(&path) {
			Ok(bytes) => serde_json::from_slice(&bytes).map_err(|source| Error::Format {
				path: path.clone(),
				source,
			})?,
			Err(source) if source.kind() == io::ErrorKind::NotFound => Settings::default(),
			Err(source) => {
				return Err(Error::Io {
					path: path.clone(),
					source,
				});
			}
		};
		Ok(Self {
			settings: Mutex::new(settings),
			path: Some(path),
		})
	}

	/// [`Self::in_dir`] when a folder is configured, else [`Self::in_memory`].
	/// An unreadable file is logged and treated as empty: a broken settings
	/// file must not keep the server from starting.
	#[must_use]
	pub fn from_config(data_dir: Option<&Path>) -> Self {
		let Some(dir) = data_dir else {
			info!("no data folder configured; settings are kept in memory only");
			return Self::in_memory();
		};
		match Self::in_dir(dir) {
			Ok(store) => store,
			Err(error) => {
				warn!(%error, "settings file unreadable; starting with empty settings");
				Self {
					settings: Mutex::new(Settings::default()),
					path: Some(dir.join(SETTINGS_FILE)),
				}
			}
		}
	}

	/// Where the settings are written, if anywhere.
	#[must_use]
	pub fn path(&self) -> Option<&Path> {
		self.path.as_deref()
	}

	/// The current settings.
	pub async fn get(&self) -> Settings {
		self.settings.lock().await.clone()
	}

	/// Stores every field that is set in `patch` and returns the result.
	///
	/// # Errors
	/// Fails when the file cannot be written. The in-memory settings are
	/// updated anyway, so the client sees its change for this server run.
	pub async fn update(&self, patch: Settings) -> Result<Settings, Error> {
		let mut settings = self.settings.lock().await;
		settings.merge(patch);
		let current = settings.clone();
		if let Some(path) = &self.path {
			write_atomically(path, &current).await?;
		}
		Ok(current)
	}
}

/// Writes `settings` to a temporary file next to `path` and renames it, so a
/// crash mid-write leaves the old file intact.
async fn write_atomically(path: &Path, settings: &Settings) -> Result<(), Error> {
	let io = |source| Error::Io {
		path: path.to_path_buf(),
		source,
	};
	if let Some(dir) = path.parent() {
		tokio::fs::create_dir_all(dir).await.map_err(io)?;
	}
	let json = serde_json::to_vec_pretty(settings).map_err(|source| Error::Format {
		path: path.to_path_buf(),
		source,
	})?;
	let temp = path.with_extension("json.tmp");
	tokio::fs::write(&temp, json).await.map_err(io)?;
	tokio::fs::rename(&temp, path).await.map_err(io)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn theme(family: &str) -> ThemeSetting {
		ThemeSetting {
			family: family.into(),
			mode: ThemeMode::Auto,
			variant: String::new(),
			light_variant: "latte".into(),
			dark_variant: "mocha".into(),
		}
	}

	#[tokio::test]
	async fn in_memory_store_starts_empty_and_keeps_updates() {
		let store = SettingsStore::in_memory();
		assert_eq!(store.get().await, Settings::default());
		assert!(store.path().is_none());

		let stored = store
			.update(Settings {
				theme: Some(theme("catppuccin")),
			})
			.await
			.unwrap();
		assert_eq!(stored.theme, Some(theme("catppuccin")));
		assert_eq!(store.get().await, stored);
	}

	#[tokio::test]
	async fn an_empty_patch_keeps_the_stored_theme() {
		let store = SettingsStore::in_memory();
		store
			.update(Settings {
				theme: Some(theme("nord")),
			})
			.await
			.unwrap();
		let after = store.update(Settings::default()).await.unwrap();
		assert_eq!(after.theme, Some(theme("nord")));
	}

	#[tokio::test]
	async fn file_store_survives_a_reopen() {
		let dir = tempfile::tempdir().unwrap();
		let data_dir = dir.path().join("nested/data");
		let store = SettingsStore::in_dir(&data_dir).unwrap();
		assert_eq!(store.get().await, Settings::default());
		store
			.update(Settings {
				theme: Some(theme("dracula")),
			})
			.await
			.unwrap();
		assert!(data_dir.join(SETTINGS_FILE).is_file());
		assert!(!data_dir.join("settings.json.tmp").exists());

		let reopened = SettingsStore::in_dir(&data_dir).unwrap();
		assert_eq!(reopened.get().await.theme, Some(theme("dracula")));
	}

	#[test]
	fn a_broken_file_is_an_error_for_in_dir_but_not_for_from_config() {
		let dir = tempfile::tempdir().unwrap();
		std::fs::write(dir.path().join(SETTINGS_FILE), b"{ not json").unwrap();

		assert!(matches!(
			SettingsStore::in_dir(dir.path()),
			Err(Error::Format { .. })
		));
		let store = SettingsStore::from_config(Some(dir.path()));
		assert_eq!(store.path(), Some(dir.path().join(SETTINGS_FILE).as_path()));
	}

	#[test]
	fn the_json_names_are_stable() {
		let json = serde_json::to_value(Settings {
			theme: Some(theme("catppuccin")),
		})
		.unwrap();
		assert_eq!(
			json,
			serde_json::json!({
				"theme": {
					"family": "catppuccin",
					"mode": "auto",
					"variant": "",
					"light_variant": "latte",
					"dark_variant": "mocha"
				}
			})
		);
		assert_eq!(
			serde_json::from_str::<Settings>("{}").unwrap(),
			Settings::default()
		);
	}
}
