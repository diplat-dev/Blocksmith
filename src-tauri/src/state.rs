use std::sync::{Arc, Mutex, MutexGuard};

use rusqlite::Connection;

use crate::{
    db::open_database,
    error::{AppError, AppResult},
    paths::AppPaths,
};

#[derive(Clone)]
pub struct AppState {
    pub paths: AppPaths,
    db: Arc<Mutex<Connection>>,
}

impl AppState {
    pub fn bootstrap(paths: AppPaths) -> AppResult<Self> {
        let connection = open_database(&paths)?;
        Ok(Self {
            paths,
            db: Arc::new(Mutex::new(connection)),
        })
    }

    pub fn db(&self) -> AppResult<MutexGuard<'_, Connection>> {
        self.db
            .lock()
            .map_err(|_| AppError::Internal("database lock was poisoned".to_string()))
    }
}
