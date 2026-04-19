use rusqlite::OptionalExtension;
use tauri::State;

use crate::{
    dto::DashboardSnapshot,
    error::AppResult,
    state::AppState,
};

#[tauri::command]
pub fn get_dashboard_snapshot(state: State<'_, AppState>) -> Result<DashboardSnapshot, String> {
    inner_dashboard_snapshot(&state).map_err(Into::into)
}

fn inner_dashboard_snapshot(state: &AppState) -> AppResult<DashboardSnapshot> {
    let connection = state.db()?;

    let profile_count = connection.query_row("SELECT COUNT(*) FROM profiles", [], |row| row.get(0))?;
    let vanilla_profile_count = connection.query_row(
        "SELECT COUNT(*) FROM profiles WHERE profile_type = 'vanilla'",
        [],
        |row| row.get(0),
    )?;
    let fabric_profile_count = connection.query_row(
        "SELECT COUNT(*) FROM profiles WHERE profile_type = 'fabric'",
        [],
        |row| row.get(0),
    )?;
    let latest_profile_name = connection
        .query_row(
            "SELECT name FROM profiles ORDER BY updated_at DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let signed_in_account_count =
        connection.query_row("SELECT COUNT(*) FROM accounts", [], |row| row.get(0))?;
    let local_skin_count =
        connection.query_row("SELECT COUNT(*) FROM skins", [], |row| row.get(0))?;
    let pending_update_count = 0;

    Ok(DashboardSnapshot {
        profile_count,
        vanilla_profile_count,
        fabric_profile_count,
        latest_profile_name,
        signed_in_account_count,
        local_skin_count,
        pending_update_count,
    })
}

