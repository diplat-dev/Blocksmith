use chrono::Utc;
use rusqlite::params;
use tauri::State;

use crate::{
    dto::SettingEntry,
    error::{AppError, AppResult},
    state::AppState,
};

#[tauri::command]
pub fn list_settings(state: State<'_, AppState>) -> Result<Vec<SettingEntry>, String> {
    inner_list_settings(&state).map_err(Into::into)
}

#[tauri::command]
pub fn upsert_setting(
    state: State<'_, AppState>,
    key: String,
    value: String,
    category: String,
) -> Result<SettingEntry, String> {
    inner_upsert_setting(&state, &key, &value, &category).map_err(Into::into)
}

fn inner_list_settings(state: &AppState) -> AppResult<Vec<SettingEntry>> {
    let connection = state.db()?;
    let mut statement = connection.prepare(
        "
        SELECT key, value, category, updated_at
        FROM settings
        ORDER BY category ASC, key ASC
        ",
    )?;

    let rows = statement.query_map([], |row| {
        Ok(SettingEntry {
            key: row.get(0)?,
            value: row.get(1)?,
            category: row.get(2)?,
            updated_at: row.get(3)?,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn inner_upsert_setting(
    state: &AppState,
    key: &str,
    value: &str,
    category: &str,
) -> AppResult<SettingEntry> {
    let trimmed_key = key.trim();
    if trimmed_key.is_empty() {
        return Err(AppError::Validation(
            "setting key cannot be empty".to_string(),
        ));
    }

    let trimmed_category = category.trim();
    if trimmed_category.is_empty() {
        return Err(AppError::Validation(
            "setting category cannot be empty".to_string(),
        ));
    }

    let updated_at = Utc::now().to_rfc3339();
    let connection = state.db()?;
    connection.execute(
        "
        INSERT INTO settings (key, value, category, updated_at)
        VALUES (?1, ?2, ?3, ?4)
        ON CONFLICT(key) DO UPDATE SET
          value = excluded.value,
          category = excluded.category,
          updated_at = excluded.updated_at
        ",
        params![trimmed_key, value, trimmed_category, updated_at],
    )?;

    Ok(SettingEntry {
        key: trimmed_key.to_string(),
        value: value.to_string(),
        category: trimmed_category.to_string(),
        updated_at,
    })
}

