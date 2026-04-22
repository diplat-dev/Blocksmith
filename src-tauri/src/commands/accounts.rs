use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use tauri::State;
use uuid::Uuid;

use crate::{
    auth,
    dto::{AccountSummary, CreateLocalAccountInput},
    error::{AppError, AppResult},
    state::AppState,
};

#[tauri::command]
pub fn list_accounts(state: State<'_, AppState>) -> Result<Vec<AccountSummary>, String> {
    inner_list_accounts(&state).map_err(Into::into)
}

#[tauri::command]
pub fn create_local_account(
    state: State<'_, AppState>,
    input: CreateLocalAccountInput,
) -> Result<AccountSummary, String> {
    inner_create_local_account(&state, input).map_err(Into::into)
}

#[tauri::command]
pub fn sign_in_microsoft(state: State<'_, AppState>) -> Result<AccountSummary, String> {
    auth::sign_in_with_microsoft(&state).map_err(Into::into)
}

#[tauri::command]
pub fn delete_account(state: State<'_, AppState>, account_id: String) -> Result<(), String> {
    inner_delete_account(&state, &account_id).map_err(Into::into)
}

#[tauri::command]
pub fn bind_profile_account(
    state: State<'_, AppState>,
    profile_id: String,
    account_id: Option<String>,
) -> Result<(), String> {
    inner_bind_profile_account(&state, &profile_id, account_id.as_deref()).map_err(Into::into)
}

fn inner_list_accounts(state: &AppState) -> AppResult<Vec<AccountSummary>> {
    let connection = state.db()?;
    let mut statement = connection.prepare(
        "
        SELECT
          accounts.id,
          accounts.username,
          accounts.uuid,
          accounts.provider,
          accounts.avatar_url,
          accounts.current_skin_id,
          accounts.owns_minecraft,
          accounts.ownership_verified_at,
          accounts.created_at,
          accounts.updated_at,
          CASE WHEN account_tokens.account_id IS NULL THEN 0 ELSE 1 END
        FROM accounts
        LEFT JOIN account_tokens ON account_tokens.account_id = accounts.id
        ORDER BY accounts.updated_at DESC, accounts.created_at DESC
        ",
    )?;

    let rows = statement.query_map([], |row| {
        Ok(AccountSummary {
            id: row.get(0)?,
            username: row.get(1)?,
            uuid: row.get(2)?,
            provider: row.get(3)?,
            avatar_url: row.get(4)?,
            current_skin_id: row.get(5)?,
            owns_minecraft: row.get::<_, i64>(6)? != 0,
            ownership_verified_at: row.get(7)?,
            created_at: row.get(8)?,
            updated_at: row.get(9)?,
            is_authenticated: row.get::<_, i64>(10)? != 0,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn inner_create_local_account(state: &AppState, input: CreateLocalAccountInput) -> AppResult<AccountSummary> {
    let username = input.username.trim();
    if username.is_empty() {
        return Err(AppError::Validation("account username cannot be empty".to_string()));
    }

    let now = Utc::now().to_rfc3339();
    let account_id = format!("account-{}", Uuid::new_v4().simple());
    let uuid = input
        .uuid
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let provider = input
        .provider
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "manual".to_string());

    let connection = state.db()?;
    connection.execute(
        "
        INSERT INTO accounts (
          id,
          username,
          uuid,
          provider,
          avatar_url,
          current_skin_id,
          owns_minecraft,
          ownership_verified_at,
          created_at,
          updated_at
        )
        VALUES (?1, ?2, ?3, ?4, NULL, NULL, 0, NULL, ?5, ?6)
        ",
        params![account_id, username, uuid, provider, now, now],
    )?;

    connection
        .query_row(
            "
            SELECT
              accounts.id,
              accounts.username,
              accounts.uuid,
              accounts.provider,
              accounts.avatar_url,
              accounts.current_skin_id,
              accounts.owns_minecraft,
              accounts.ownership_verified_at,
              accounts.created_at,
              accounts.updated_at,
              CASE WHEN account_tokens.account_id IS NULL THEN 0 ELSE 1 END
            FROM accounts
            LEFT JOIN account_tokens ON account_tokens.account_id = accounts.id
            WHERE accounts.id = ?1
            ",
            params![account_id],
            |row| {
                Ok(AccountSummary {
                    id: row.get(0)?,
                    username: row.get(1)?,
                    uuid: row.get(2)?,
                    provider: row.get(3)?,
                    avatar_url: row.get(4)?,
                    current_skin_id: row.get(5)?,
                    owns_minecraft: row.get::<_, i64>(6)? != 0,
                    ownership_verified_at: row.get(7)?,
                    created_at: row.get(8)?,
                    updated_at: row.get(9)?,
                    is_authenticated: row.get::<_, i64>(10)? != 0,
                })
            },
        )
        .optional()?
        .ok_or_else(|| AppError::NotFound("created account could not be reloaded".to_string()))
}

fn inner_delete_account(state: &AppState, account_id: &str) -> AppResult<()> {
    auth::delete_persisted_account_token(state, account_id)?;
    let connection = state.db()?;
    connection.execute("UPDATE profiles SET account_id = NULL WHERE account_id = ?1", params![account_id])?;
    connection.execute("DELETE FROM account_tokens WHERE account_id = ?1", params![account_id])?;
    connection.execute("DELETE FROM accounts WHERE id = ?1", params![account_id])?;
    Ok(())
}

fn inner_bind_profile_account(
    state: &AppState,
    profile_id: &str,
    account_id: Option<&str>,
) -> AppResult<()> {
    let connection = state.db()?;

    if let Some(account_id) = account_id {
        let exists: Option<String> = connection
            .query_row(
                "SELECT id FROM accounts WHERE id = ?1",
                params![account_id],
                |row| row.get(0),
            )
            .optional()?;

        if exists.is_none() {
            return Err(AppError::NotFound(format!("account not found: {account_id}")));
        }
    }

    connection.execute(
        "UPDATE profiles SET account_id = ?1, updated_at = ?2 WHERE id = ?3",
        params![account_id, Utc::now().to_rfc3339(), profile_id],
    )?;

    Ok(())
}
