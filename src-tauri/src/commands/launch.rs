use std::{path::PathBuf, thread};

use chrono::Utc;
use rusqlite::params;
use tauri::State;
use uuid::Uuid;

use crate::{
    db::open_database,
    dto::{FabricLoaderSummary, LaunchHistoryEntry, LaunchPlan, MinecraftVersionSummary},
    error::AppResult,
    minecraft,
    state::AppState,
};

use super::profiles::inner_get_profile_detail;

#[tauri::command]
pub async fn list_minecraft_versions() -> Result<Vec<MinecraftVersionSummary>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        minecraft::list_minecraft_versions().map_err(Into::into)
    })
    .await
    .map_err(|error| format!("background task failed: {error}"))?
}

#[tauri::command]
pub async fn list_fabric_loader_versions(
    minecraft_version: String,
) -> Result<Vec<FabricLoaderSummary>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        minecraft::list_fabric_loader_versions(&minecraft_version).map_err(Into::into)
    })
    .await
    .map_err(|error| format!("background task failed: {error}"))?
}

#[tauri::command]
pub async fn resolve_launch_plan(
    state: State<'_, AppState>,
    profile_id: String,
) -> Result<LaunchPlan, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        inner_resolve_launch_plan(&state, &profile_id).map_err(Into::into)
    })
    .await
    .map_err(|error| format!("background task failed: {error}"))?
}

#[tauri::command]
pub async fn launch_profile(
    state: State<'_, AppState>,
    profile_id: String,
) -> Result<LaunchHistoryEntry, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        inner_launch_profile(&state, &profile_id).map_err(Into::into)
    })
    .await
    .map_err(|error| format!("background task failed: {error}"))?
}

#[tauri::command]
pub fn list_launch_history(
    state: State<'_, AppState>,
    profile_id: Option<String>,
) -> Result<Vec<LaunchHistoryEntry>, String> {
    inner_list_launch_history(&state, profile_id.as_deref()).map_err(Into::into)
}

fn inner_resolve_launch_plan(state: &AppState, profile_id: &str) -> AppResult<LaunchPlan> {
    let profile = inner_get_profile_detail(state, profile_id)?.summary;
    minecraft::resolve_launch_plan(state, &profile)
}

fn inner_launch_profile(state: &AppState, profile_id: &str) -> AppResult<LaunchHistoryEntry> {
    let profile = inner_get_profile_detail(state, profile_id)?.summary;
    let prepared = minecraft::prepare_launch(state, &profile)?;
    let launch_id = format!("launch-{}", Uuid::new_v4().simple());
    let started_at = Utc::now().to_rfc3339();
    let log_path = state.paths.logs_dir.join(format!("{launch_id}.log"));
    let working_directory = PathBuf::from(&prepared.plan.working_directory);

    let mut child = minecraft::launch_process(&prepared, &log_path, &working_directory)?;
    let connection = state.db()?;
    connection.execute(
        "
        INSERT INTO launch_history (id, profile_id, account_id, started_at, ended_at, status, log_path, exit_code)
        VALUES (?1, ?2, ?3, ?4, NULL, 'running', ?5, NULL)
        ",
        params![
            launch_id,
            profile.id,
            profile.account_id,
            started_at,
            log_path.to_string_lossy().into_owned()
        ],
    )?;
    connection.execute(
        "UPDATE profiles SET last_played_at = ?1, updated_at = ?1 WHERE id = ?2",
        params![started_at, profile.id],
    )?;
    drop(connection);

    let paths = state.paths.clone();
    let launch_id_for_thread = launch_id.clone();
    let profile_id_for_thread = profile.id.clone();
    thread::spawn(move || {
        let wait_result = child.wait();
        let ended_at = Utc::now().to_rfc3339();
        let (status, exit_code) = match wait_result {
            Ok(status) if status.success() => ("success".to_string(), status.code()),
            Ok(status) => ("failure".to_string(), status.code()),
            Err(_) => ("failure".to_string(), None),
        };

        if let Ok(connection) = open_database(&paths) {
            let _ = connection.execute(
                "
                UPDATE launch_history
                SET ended_at = ?1, status = ?2, exit_code = ?3
                WHERE id = ?4
                ",
                params![ended_at, status, exit_code, launch_id_for_thread],
            );

            let _ = connection.execute(
                "UPDATE profiles SET updated_at = ?1 WHERE id = ?2",
                params![Utc::now().to_rfc3339(), profile_id_for_thread],
            );
        }
    });

    Ok(LaunchHistoryEntry {
        id: launch_id,
        profile_id: profile.id,
        account_id: profile.account_id,
        started_at,
        ended_at: None,
        status: "running".to_string(),
        log_path: log_path.to_string_lossy().into_owned(),
        exit_code: None,
    })
}

fn inner_list_launch_history(
    state: &AppState,
    profile_id: Option<&str>,
) -> AppResult<Vec<LaunchHistoryEntry>> {
    let connection = state.db()?;
    let sql = if profile_id.is_some() {
        "
        SELECT id, profile_id, account_id, started_at, ended_at, status, log_path, exit_code
        FROM launch_history
        WHERE profile_id = ?1
        ORDER BY started_at DESC
        "
    } else {
        "
        SELECT id, profile_id, account_id, started_at, ended_at, status, log_path, exit_code
        FROM launch_history
        ORDER BY started_at DESC
        "
    };

    let mut statement = connection.prepare(sql)?;
    let rows = if let Some(profile_id) = profile_id {
        statement.query_map(params![profile_id], launch_history_from_row)?
    } else {
        statement.query_map([], launch_history_from_row)?
    };

    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn launch_history_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<LaunchHistoryEntry> {
    Ok(LaunchHistoryEntry {
        id: row.get(0)?,
        profile_id: row.get(1)?,
        account_id: row.get(2)?,
        started_at: row.get(3)?,
        ended_at: row.get(4)?,
        status: row.get(5)?,
        log_path: row.get(6)?,
        exit_code: row.get(7)?,
    })
}
