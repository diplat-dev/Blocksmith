use std::{fs, path::PathBuf};

use serde::Serialize;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppPaths {
    pub root_dir: PathBuf,
    pub db_path: PathBuf,
    pub cache_dir: PathBuf,
    pub logs_dir: PathBuf,
    pub profiles_dir: PathBuf,
    pub skins_dir: PathBuf,
    pub exports_dir: PathBuf,
    pub runtimes_dir: PathBuf,
    pub temp_dir: PathBuf,
}

impl AppPaths {
    pub fn discover() -> AppResult<Self> {
        let local_data_dir = dirs::data_local_dir().ok_or_else(|| {
            AppError::Path("could not resolve %LocalAppData% for Blocksmith".to_string())
        })?;

        let root_dir = local_data_dir.join("Blocksmith");
        let paths = Self {
            db_path: root_dir.join("db.sqlite"),
            cache_dir: root_dir.join("cache"),
            logs_dir: root_dir.join("logs"),
            profiles_dir: root_dir.join("profiles"),
            skins_dir: root_dir.join("skins"),
            exports_dir: root_dir.join("exports"),
            runtimes_dir: root_dir.join("runtimes"),
            temp_dir: root_dir.join("temp"),
            root_dir,
        };

        paths.ensure_layout()?;
        Ok(paths)
    }

    pub fn ensure_layout(&self) -> AppResult<()> {
        for path in [
            &self.root_dir,
            &self.cache_dir,
            &self.logs_dir,
            &self.profiles_dir,
            &self.skins_dir,
            &self.exports_dir,
            &self.runtimes_dir,
            &self.temp_dir,
        ] {
            fs::create_dir_all(path)?;
        }

        Ok(())
    }

    pub fn profile_root(&self, profile_id: &str) -> PathBuf {
        self.profiles_dir.join(profile_id)
    }
}

