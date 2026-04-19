use std::{fs, str::FromStr};

use chrono::Utc;
use rusqlite::{params, OptionalExtension, Row};
use tauri::State;
use uuid::Uuid;

use crate::{
    dto::{
        normalize_profile_name, CreateProfileInput, DuplicateProfileInput, ProfileDetail,
        ProfileSummary, ProfileType,
    },
    error::{AppError, AppResult},
    profile_fs::{copy_profile_tree, ensure_profile_target, scaffold_profile_layout},
    state::AppState,
};

#[tauri::command]
pub fn list_profiles(state: State<'_, AppState>) -> Result<Vec<ProfileSummary>, String> {
    inner_list_profiles(&state).map_err(Into::into)
}

#[tauri::command]
pub fn get_profile_detail(
    state: State<'_, AppState>,
    profile_id: String,
) -> Result<ProfileDetail, String> {
    inner_get_profile_detail(&state, &profile_id).map_err(Into::into)
}

#[tauri::command]
pub fn create_profile(
    state: State<'_, AppState>,
    input: CreateProfileInput,
) -> Result<ProfileDetail, String> {
    inner_create_profile(&state, input).map_err(Into::into)
}

#[tauri::command]
pub fn duplicate_profile(
    state: State<'_, AppState>,
    input: DuplicateProfileInput,
) -> Result<ProfileDetail, String> {
    inner_duplicate_profile(&state, input).map_err(Into::into)
}

#[tauri::command]
pub fn delete_profile(state: State<'_, AppState>, profile_id: String) -> Result<(), String> {
    inner_delete_profile(&state, &profile_id).map_err(Into::into)
}

pub(crate) fn inner_list_profiles(state: &AppState) -> AppResult<Vec<ProfileSummary>> {
    let connection = state.db()?;
    let mut statement = connection.prepare(
        "
        SELECT
          id,
          name,
          profile_type,
          minecraft_version,
          loader_version,
          profile_dir,
          account_id,
          java_path,
          memory_min_mb,
          memory_max_mb,
          jvm_args,
          launch_args,
          notes,
          created_at,
          updated_at,
          last_played_at
        FROM profiles
        ORDER BY updated_at DESC, created_at DESC
        ",
    )?;

    let rows = statement.query_map([], profile_summary_from_row)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub(crate) fn inner_get_profile_detail(state: &AppState, profile_id: &str) -> AppResult<ProfileDetail> {
    let connection = state.db()?;
    fetch_profile_detail(&connection, profile_id)?
        .ok_or_else(|| AppError::NotFound(format!("profile not found: {profile_id}")))
}

pub(crate) fn inner_create_profile(state: &AppState, input: CreateProfileInput) -> AppResult<ProfileDetail> {
    let name = normalize_profile_name(&input.name)?;
    let minecraft_version = input.minecraft_version.trim().to_string();
    if minecraft_version.is_empty() {
        return Err(AppError::Validation(
            "minecraft version cannot be empty".to_string(),
        ));
    }

    let loader_version = match input.profile_type {
        ProfileType::Vanilla => None,
        ProfileType::Fabric => Some(
            input
                .loader_version
                .as_deref()
                .unwrap_or("latest")
                .trim()
                .to_string(),
        ),
    };

    let profile_id = format!("profile-{}", Uuid::new_v4().simple());
    let profile_root = state.paths.profile_root(&profile_id);
    ensure_profile_target(&state.paths.profiles_dir, &profile_root)?;
    scaffold_profile_layout(&profile_root)?;

    let created_at = Utc::now().to_rfc3339();
    let directory_path = profile_root.to_string_lossy().into_owned();

    let result = (|| -> AppResult<ProfileDetail> {
        let connection = state.db()?;
        connection.execute(
            "
            INSERT INTO profiles (
              id,
              name,
              profile_type,
              minecraft_version,
              loader_version,
              profile_dir,
              account_id,
              java_path,
              memory_min_mb,
              memory_max_mb,
              jvm_args,
              launch_args,
              notes,
              created_at,
              updated_at,
              last_played_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, NULL)
            ",
            params![
                profile_id,
                name,
                input.profile_type.as_str(),
                minecraft_version,
                loader_version,
                directory_path,
                input.account_id,
                input.java_path,
                input.memory_min_mb,
                input.memory_max_mb,
                input.jvm_args.unwrap_or_default(),
                input.launch_args.unwrap_or_default(),
                input.notes,
                created_at,
                created_at,
            ],
        )?;

        fetch_profile_detail(&connection, &profile_id)?
            .ok_or_else(|| AppError::NotFound(format!("profile not found after create: {profile_id}")))
    })();

    if result.is_err() {
        let _ = fs::remove_dir_all(&profile_root);
    }

    result
}

fn inner_duplicate_profile(
    state: &AppState,
    input: DuplicateProfileInput,
) -> AppResult<ProfileDetail> {
    let new_name = normalize_profile_name(&input.new_name)?;
    let source = inner_get_profile_detail(state, &input.source_profile_id)?;
    let source_root = std::path::PathBuf::from(&source.summary.directory_path);

    let profile_id = format!("profile-{}", Uuid::new_v4().simple());
    let profile_root = state.paths.profile_root(&profile_id);
    ensure_profile_target(&state.paths.profiles_dir, &profile_root)?;
    copy_profile_tree(&source_root, &profile_root)?;

    let created_at = Utc::now().to_rfc3339();
    let result = (|| -> AppResult<ProfileDetail> {
        let connection = state.db()?;
        connection.execute(
            "
            INSERT INTO profiles (
              id,
              name,
              profile_type,
              minecraft_version,
              loader_version,
              profile_dir,
              account_id,
              java_path,
              memory_min_mb,
              memory_max_mb,
              jvm_args,
              launch_args,
              notes,
              created_at,
              updated_at,
              last_played_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, NULL)
            ",
            params![
                profile_id,
                new_name,
                source.summary.profile_type.as_str(),
                source.summary.minecraft_version,
                source.summary.loader_version,
                profile_root.to_string_lossy().into_owned(),
                source.summary.account_id,
                source.summary.java_path,
                source.summary.memory_min_mb,
                source.summary.memory_max_mb,
                source.summary.jvm_args,
                source.summary.launch_args,
                source.summary.notes,
                created_at,
                created_at,
            ],
        )?;

        fetch_profile_detail(&connection, &profile_id)?
            .ok_or_else(|| AppError::NotFound(format!("profile not found after duplicate: {profile_id}")))
    })();

    if result.is_err() {
        let _ = fs::remove_dir_all(&profile_root);
    }

    result
}

fn inner_delete_profile(state: &AppState, profile_id: &str) -> AppResult<()> {
    let detail = inner_get_profile_detail(state, profile_id)?;
    let profile_root = std::path::PathBuf::from(&detail.summary.directory_path);
    ensure_profile_target(&state.paths.profiles_dir, &profile_root)?;

    if profile_root.exists() {
        fs::remove_dir_all(&profile_root)?;
    }

    let connection = state.db()?;
    connection.execute("DELETE FROM profiles WHERE id = ?1", params![profile_id])?;
    Ok(())
}

pub(crate) fn fetch_profile_detail(
    connection: &rusqlite::Connection,
    profile_id: &str,
) -> AppResult<Option<ProfileDetail>> {
    let summary = connection
        .query_row(
            "
            SELECT
              id,
              name,
              profile_type,
              minecraft_version,
              loader_version,
              profile_dir,
              account_id,
              java_path,
              memory_min_mb,
              memory_max_mb,
              jvm_args,
              launch_args,
              notes,
              created_at,
              updated_at,
              last_played_at
            FROM profiles
            WHERE id = ?1
            ",
            params![profile_id],
            profile_summary_from_row,
        )
        .optional()?;

    Ok(summary.map(ProfileDetail::from_summary))
}

pub(crate) fn profile_summary_from_row(row: &Row<'_>) -> rusqlite::Result<ProfileSummary> {
    let profile_type: String = row.get(2)?;

    Ok(ProfileSummary {
        id: row.get(0)?,
        name: row.get(1)?,
        profile_type: ProfileType::from_str(&profile_type).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                2,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        minecraft_version: row.get(3)?,
        loader_version: row.get(4)?,
        directory_path: row.get(5)?,
        account_id: row.get(6)?,
        java_path: row.get(7)?,
        memory_min_mb: row.get(8)?,
        memory_max_mb: row.get(9)?,
        jvm_args: row.get(10)?,
        launch_args: row.get(11)?,
        notes: row.get(12)?,
        created_at: row.get(13)?,
        updated_at: row.get(14)?,
        last_played_at: row.get(15)?,
    })
}
