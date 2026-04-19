use std::{ffi::OsStr, fs, path::Path};

use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use tauri::State;
use uuid::Uuid;

use crate::{
    auth,
    dto::{ApplySkinInput, ImportSkinInput, SkinEntry},
    error::{AppError, AppResult},
    state::AppState,
};

const PNG_SIGNATURE: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];

#[tauri::command]
pub fn list_skins(state: State<'_, AppState>) -> Result<Vec<SkinEntry>, String> {
    inner_list_skins(&state).map_err(Into::into)
}

#[tauri::command]
pub fn import_skin(
    state: State<'_, AppState>,
    input: ImportSkinInput,
) -> Result<SkinEntry, String> {
    inner_import_skin(&state, input).map_err(Into::into)
}

#[tauri::command]
pub fn delete_skin(state: State<'_, AppState>, skin_id: String) -> Result<(), String> {
    inner_delete_skin(&state, &skin_id).map_err(Into::into)
}

#[tauri::command]
pub fn apply_skin_to_account(
    state: State<'_, AppState>,
    input: ApplySkinInput,
) -> Result<(), String> {
    inner_apply_skin_to_account(&state, input).map_err(Into::into)
}

fn inner_list_skins(state: &AppState) -> AppResult<Vec<SkinEntry>> {
    let connection = state.db()?;
    let mut statement = connection.prepare(
        "
        SELECT id, local_file_path, display_name, model_variant, tags_json, thumbnail_path, imported_at, updated_at
        FROM skins
        ORDER BY updated_at DESC, imported_at DESC
        ",
    )?;

    let rows = statement.query_map([], |row| {
        let tags_json: String = row.get(4)?;
        Ok(SkinEntry {
            id: row.get(0)?,
            local_file_path: row.get(1)?,
            display_name: row.get(2)?,
            model_variant: row.get(3)?,
            tags: serde_json::from_str(&tags_json).unwrap_or_default(),
            thumbnail_path: row.get(5)?,
            imported_at: row.get(6)?,
            updated_at: row.get(7)?,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn inner_import_skin(state: &AppState, input: ImportSkinInput) -> AppResult<SkinEntry> {
    let source_path = Path::new(&input.source_path);
    if !source_path.exists() {
        return Err(AppError::NotFound(format!(
            "skin source file not found: {}",
            source_path.to_string_lossy()
        )));
    }

    if source_path.extension() != Some(OsStr::new("png")) {
        return Err(AppError::Validation(
            "skins must be imported from a .png file".to_string(),
        ));
    }

    let bytes = fs::read(source_path)?;
    if bytes.len() < PNG_SIGNATURE.len() || bytes[..PNG_SIGNATURE.len()] != PNG_SIGNATURE {
        return Err(AppError::Validation(
            "skin file is not a valid PNG".to_string(),
        ));
    }

    let model_variant = match input.model_variant.trim() {
        "classic" | "slim" => input.model_variant.trim().to_string(),
        _ => {
            return Err(AppError::Validation(
                "skin model variant must be 'classic' or 'slim'".to_string(),
            ))
        }
    };

    let skin_id = format!("skin-{}", Uuid::new_v4().simple());
    let destination = state.paths.skins_dir.join(format!("{skin_id}.png"));
    fs::copy(source_path, &destination)?;

    let display_name = input
        .display_name
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            source_path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("Imported Skin")
                .to_string()
        });

    let imported_at = Utc::now().to_rfc3339();
    let tags_json = serde_json::to_string(&input.tags)?;

    let connection = state.db()?;
    connection.execute(
        "
        INSERT INTO skins (id, local_file_path, display_name, model_variant, tags_json, thumbnail_path, imported_at, updated_at)
        VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6, ?7)
        ",
        params![
            skin_id,
            destination.to_string_lossy().into_owned(),
            display_name,
            model_variant,
            tags_json,
            imported_at,
            imported_at,
        ],
    )?;

    connection
        .query_row(
            "
            SELECT id, local_file_path, display_name, model_variant, tags_json, thumbnail_path, imported_at, updated_at
            FROM skins
            WHERE id = ?1
            ",
            params![skin_id],
            |row| {
                let tags_json: String = row.get(4)?;
                Ok(SkinEntry {
                    id: row.get(0)?,
                    local_file_path: row.get(1)?,
                    display_name: row.get(2)?,
                    model_variant: row.get(3)?,
                    tags: serde_json::from_str(&tags_json).unwrap_or_default(),
                    thumbnail_path: row.get(5)?,
                    imported_at: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            },
        )
        .optional()?
        .ok_or_else(|| AppError::NotFound("imported skin could not be reloaded".to_string()))
}

fn inner_delete_skin(state: &AppState, skin_id: &str) -> AppResult<()> {
    let connection = state.db()?;
    let file_path: Option<String> = connection
        .query_row(
            "SELECT local_file_path FROM skins WHERE id = ?1",
            params![skin_id],
            |row| row.get(0),
        )
        .optional()?;

    connection.execute(
        "UPDATE accounts SET current_skin_id = NULL WHERE current_skin_id = ?1",
        params![skin_id],
    )?;
    connection.execute("DELETE FROM skins WHERE id = ?1", params![skin_id])?;

    if let Some(file_path) = file_path {
        let file_path = Path::new(&file_path);
        if file_path.exists() {
            fs::remove_file(file_path)?;
        }
    }

    Ok(())
}

fn inner_apply_skin_to_account(state: &AppState, input: ApplySkinInput) -> AppResult<()> {
    let connection = state.db()?;

    let skin_file_path: Option<(String, String)> = connection
        .query_row(
            "SELECT id, local_file_path, model_variant FROM skins WHERE id = ?1",
            params![input.skin_id],
            |row| Ok((row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let Some((skin_file_path, model_variant)) = skin_file_path else {
        return Err(AppError::NotFound(format!("skin not found: {}", input.skin_id)));
    };

    let account_provider: Option<String> = connection
        .query_row(
            "SELECT provider FROM accounts WHERE id = ?1",
            params![input.account_id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(account_provider) = account_provider else {
        return Err(AppError::NotFound(format!("account not found: {}", input.account_id)));
    };
    drop(connection);

    if account_provider.eq_ignore_ascii_case("microsoft") {
        auth::upload_skin_for_account(
            state,
            &input.account_id,
            Path::new(&skin_file_path),
            &model_variant,
        )?;
    }

    let connection = state.db()?;

    connection.execute(
        "UPDATE accounts SET current_skin_id = ?1, updated_at = ?2 WHERE id = ?3",
        params![input.skin_id, Utc::now().to_rfc3339(), input.account_id],
    )?;

    Ok(())
}
