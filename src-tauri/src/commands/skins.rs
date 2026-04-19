use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine};
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
        let local_file_path: String = row.get(1)?;
        let tags_json: String = row.get(4)?;
        Ok(build_skin_entry(
            row.get(0)?,
            local_file_path,
            row.get(2)?,
            row.get(3)?,
            serde_json::from_str(&tags_json).unwrap_or_default(),
            row.get(5)?,
            row.get(6)?,
            row.get(7)?,
        ))
    })?;

    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn inner_import_skin(state: &AppState, input: ImportSkinInput) -> AppResult<SkinEntry> {
    let source_path = input
        .source_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let source_file_name = input
        .file_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let source_bytes = input.source_bytes.filter(|bytes| !bytes.is_empty());

    let (bytes, inferred_display_name) = match (source_path.as_deref(), source_bytes) {
        (Some(source_path), None) => {
            if !source_path.exists() {
                return Err(AppError::NotFound(format!(
                    "skin source file not found: {}",
                    source_path.to_string_lossy()
                )));
            }

            if !has_png_extension(source_path) {
                return Err(AppError::Validation(
                    "skins must be imported from a .png file".to_string(),
                ));
            }

            let bytes = fs::read(source_path)?;
            let inferred_display_name = infer_skin_name_from_path(source_path);
            (bytes, inferred_display_name)
        }
        (None, Some(source_bytes)) => (
            source_bytes,
            infer_skin_name_from_file_name(source_file_name.as_deref()),
        ),
        (Some(_), Some(source_bytes)) => (
            source_bytes,
            infer_skin_name_from_file_name(source_file_name.as_deref()).or_else(|| {
                source_path
                    .as_deref()
                    .and_then(infer_skin_name_from_path)
            }),
        ),
        (None, None) => {
            return Err(AppError::Validation(
                "choose a skin PNG or provide a PNG path before importing".to_string(),
            ))
        }
    };

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
    fs::write(&destination, &bytes)?;

    let display_name = input
        .display_name
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| inferred_display_name.unwrap_or_else(|| "Imported Skin".to_string()));

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
                let local_file_path: String = row.get(1)?;
                let tags_json: String = row.get(4)?;
                Ok(build_skin_entry(
                    row.get(0)?,
                    local_file_path,
                    row.get(2)?,
                    row.get(3)?,
                    serde_json::from_str(&tags_json).unwrap_or_default(),
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| AppError::NotFound("imported skin could not be reloaded".to_string()))
}

fn inner_delete_skin(state: &AppState, skin_id: &str) -> AppResult<()> {
    let connection = state.db()?;
    let file_paths: Option<(String, Option<String>)> = connection
        .query_row(
            "SELECT local_file_path, thumbnail_path FROM skins WHERE id = ?1",
            params![skin_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;

    connection.execute(
        "UPDATE accounts SET current_skin_id = NULL WHERE current_skin_id = ?1",
        params![skin_id],
    )?;
    connection.execute("DELETE FROM skins WHERE id = ?1", params![skin_id])?;

    if let Some((file_path, thumbnail_path)) = file_paths {
        remove_if_exists(Path::new(&file_path))?;
        if let Some(thumbnail_path) = thumbnail_path {
            let thumbnail_path = PathBuf::from(thumbnail_path);
            if thumbnail_path != PathBuf::from(&file_path) {
                remove_if_exists(&thumbnail_path)?;
            }
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

fn build_skin_entry(
    id: String,
    local_file_path: String,
    display_name: String,
    model_variant: String,
    tags: Vec<String>,
    thumbnail_path: Option<String>,
    imported_at: String,
    updated_at: String,
) -> SkinEntry {
    let preview_data_url = skin_preview_data_url(Path::new(&local_file_path)).ok();

    SkinEntry {
        id,
        local_file_path,
        display_name,
        model_variant,
        tags,
        thumbnail_path,
        preview_data_url,
        imported_at,
        updated_at,
    }
}

fn skin_preview_data_url(path: &Path) -> AppResult<String> {
    let bytes = fs::read(path)?;
    Ok(format!(
        "data:image/png;base64,{}",
        BASE64_STANDARD.encode(bytes)
    ))
}

fn has_png_extension(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .map(|extension| extension.eq_ignore_ascii_case("png"))
        .unwrap_or(false)
}

fn infer_skin_name_from_path(path: &Path) -> Option<String> {
    path.file_stem()
        .and_then(OsStr::to_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn infer_skin_name_from_file_name(file_name: Option<&str>) -> Option<String> {
    let file_name = file_name?.trim();
    if file_name.is_empty() {
        return None;
    }

    Path::new(file_name)
        .file_stem()
        .and_then(OsStr::to_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn remove_if_exists(path: &Path) -> AppResult<()> {
    if path.exists() {
        fs::remove_file(path)?;
    }

    Ok(())
}
