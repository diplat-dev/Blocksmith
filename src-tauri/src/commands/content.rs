use std::{
    fs,
    path::{Component, Path, PathBuf},
    str::FromStr,
};

use chrono::Utc;
use rusqlite::{params, OptionalExtension, Row};
use sha2::{Digest, Sha256};
use tauri::State;
use uuid::Uuid;

use crate::{
    dto::{
        ApplyInstallPlanInput, ContentSearchResult, ContentType, CreateInstallPlanInput,
        DependencyWarning, ImportMrpackInput, InstallModpackInput, InstallPlan, InstallScope,
        InstalledContentRecord, ModrinthSearchInput, ProfileSummary, ToggleInstalledContentInput,
        UpdateCandidate,
    },
    error::{AppError, AppResult},
    modpack,
    modrinth::{content_type_from_modrinth, version_is_compatible, ModrinthClient, ProjectResponse, VersionDependency, VersionResponse},
    profile_fs::ensure_profile_target,
    state::AppState,
};

#[tauri::command]
pub async fn search_modrinth(
    state: State<'_, AppState>,
    input: ModrinthSearchInput,
) -> Result<Vec<ContentSearchResult>, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        inner_search_modrinth(&state, input).map_err(Into::into)
    })
    .await
    .map_err(|error| format!("background task failed: {error}"))?
}

#[tauri::command]
pub async fn create_install_plan(
    state: State<'_, AppState>,
    input: CreateInstallPlanInput,
) -> Result<InstallPlan, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        inner_create_install_plan(&state, input).map_err(Into::into)
    })
    .await
    .map_err(|error| format!("background task failed: {error}"))?
}

#[tauri::command]
pub async fn apply_install_plan(
    state: State<'_, AppState>,
    input: ApplyInstallPlanInput,
) -> Result<InstalledContentRecord, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        inner_apply_install_plan(&state, input).map_err(Into::into)
    })
    .await
    .map_err(|error| format!("background task failed: {error}"))?
}

#[tauri::command]
pub async fn import_mrpack(
    state: State<'_, AppState>,
    input: ImportMrpackInput,
) -> Result<crate::dto::ProfileDetail, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        modpack::import_mrpack(&state, input).map_err(Into::into)
    })
    .await
    .map_err(|error| format!("background task failed: {error}"))?
}

#[tauri::command]
pub async fn install_modrinth_modpack(
    state: State<'_, AppState>,
    input: InstallModpackInput,
) -> Result<crate::dto::ProfileDetail, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        modpack::install_modrinth_modpack(&state, input).map_err(Into::into)
    })
    .await
    .map_err(|error| format!("background task failed: {error}"))?
}

#[tauri::command]
pub fn list_installed_content(
    state: State<'_, AppState>,
    profile_id: Option<String>,
) -> Result<Vec<InstalledContentRecord>, String> {
    inner_list_installed_content(&state, profile_id.as_deref()).map_err(Into::into)
}

#[tauri::command]
pub fn toggle_installed_content(
    state: State<'_, AppState>,
    input: ToggleInstalledContentInput,
) -> Result<InstalledContentRecord, String> {
    inner_toggle_installed_content(&state, input).map_err(Into::into)
}

#[tauri::command]
pub fn remove_installed_content(
    state: State<'_, AppState>,
    installed_content_id: String,
) -> Result<(), String> {
    inner_remove_installed_content(&state, &installed_content_id).map_err(Into::into)
}

#[tauri::command]
pub async fn list_update_candidates(
    state: State<'_, AppState>,
    profile_id: Option<String>,
) -> Result<Vec<UpdateCandidate>, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        inner_list_update_candidates(&state, profile_id.as_deref()).map_err(Into::into)
    })
    .await
    .map_err(|error| format!("background task failed: {error}"))?
}

#[tauri::command]
pub async fn apply_update_candidate(
    state: State<'_, AppState>,
    installed_content_id: String,
    target_version_id: String,
) -> Result<InstalledContentRecord, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        inner_apply_update_candidate(&state, &installed_content_id, &target_version_id)
            .map_err(Into::into)
    })
    .await
    .map_err(|error| format!("background task failed: {error}"))?
}

fn inner_search_modrinth(state: &AppState, input: ModrinthSearchInput) -> AppResult<Vec<ContentSearchResult>> {
    let profile = match input.profile_id.as_deref() {
        Some(profile_id) => Some(fetch_profile_summary(state, profile_id)?),
        None => None,
    };

    let client = ModrinthClient::new()?;
    client.search_projects(&input.query, profile.as_ref(), input.content_type)
}

fn inner_create_install_plan(state: &AppState, input: CreateInstallPlanInput) -> AppResult<InstallPlan> {
    if input.content_type == ContentType::Modpack {
        return Err(AppError::Validation(
            "Modrinth modpack ingestion is not implemented in this slice yet".to_string(),
        ));
    }

    let profile = fetch_profile_summary(state, &input.profile_id)?;
    let client = ModrinthClient::new()?;
    let project = client.get_project(&input.project_id)?;
    let version = client.get_latest_compatible_version(&input.project_id, &profile, input.content_type)?;

    build_install_plan(
        state,
        &profile,
        &project,
        &version,
        input.content_type,
        input.install_scope,
        input.target_rel_path.as_deref(),
    )
}

fn inner_apply_install_plan(
    state: &AppState,
    input: ApplyInstallPlanInput,
) -> AppResult<InstalledContentRecord> {
    let plan = input.plan;
    let profile = fetch_profile_summary(state, &plan.profile_id)?;
    let existing = fetch_existing_content_record(state, &plan.profile_id, &plan.project_id, None)?;

    install_exact_version(
        state,
        &profile,
        &plan.project_id,
        &plan.version_id,
        plan.content_type,
        plan.install_scope,
        plan.target_rel_path.as_deref(),
        existing.as_ref().map(|record| record.id.as_str()),
        existing.as_ref().map(|record| record.enabled).unwrap_or(true),
    )
}

pub(crate) fn inner_list_installed_content(
    state: &AppState,
    profile_id: Option<&str>,
) -> AppResult<Vec<InstalledContentRecord>> {
    let connection = state.db()?;
    let sql = if profile_id.is_some() {
        "
        SELECT id, profile_id, content_type, install_scope, provider, project_id, version_id, slug, name, local_file_path, target_rel_path, file_hash, enabled, version_number, installed_at, updated_at
        FROM installed_content
        WHERE profile_id = ?1
        ORDER BY updated_at DESC, installed_at DESC
        "
    } else {
        "
        SELECT id, profile_id, content_type, install_scope, provider, project_id, version_id, slug, name, local_file_path, target_rel_path, file_hash, enabled, version_number, installed_at, updated_at
        FROM installed_content
        ORDER BY updated_at DESC, installed_at DESC
        "
    };

    let mut statement = connection.prepare(sql)?;
    let rows = if let Some(profile_id) = profile_id {
        statement.query_map(params![profile_id], installed_content_from_row)?
    } else {
        statement.query_map([], installed_content_from_row)?
    };

    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn inner_toggle_installed_content(
    state: &AppState,
    input: ToggleInstalledContentInput,
) -> AppResult<InstalledContentRecord> {
    let mut record = fetch_installed_content_by_id(state, &input.installed_content_id)?;
    let content_type = ContentType::from_str(&record.content_type)?;
    if content_type != ContentType::Mod {
        return Err(AppError::Validation(
            "only mods can be toggled with .disabled suffix semantics".to_string(),
        ));
    }

    let current_path = PathBuf::from(&record.local_file_path);
    if !current_path.exists() {
        return Err(AppError::NotFound(format!(
            "installed content file not found: {}",
            current_path.to_string_lossy()
        )));
    }

    let next_path = if input.enabled {
        remove_disabled_suffix(&current_path)?
    } else {
        append_disabled_suffix(&current_path)?
    };

    if current_path != next_path {
        fs::rename(&current_path, &next_path)?;
    }

    let updated_at = Utc::now().to_rfc3339();
    let connection = state.db()?;
    connection.execute(
        "
        UPDATE installed_content
        SET enabled = ?1, local_file_path = ?2, updated_at = ?3
        WHERE id = ?4
        ",
        params![
            if input.enabled { 1 } else { 0 },
            next_path.to_string_lossy().into_owned(),
            updated_at,
            input.installed_content_id,
        ],
    )?;

    record.enabled = input.enabled;
    record.local_file_path = next_path.to_string_lossy().into_owned();
    record.updated_at = updated_at;
    Ok(record)
}

fn inner_remove_installed_content(state: &AppState, installed_content_id: &str) -> AppResult<()> {
    let record = fetch_installed_content_by_id(state, installed_content_id)?;
    let file_path = Path::new(&record.local_file_path);
    if file_path.exists() {
        fs::remove_file(file_path)?;
    }

    let connection = state.db()?;
    connection.execute(
        "DELETE FROM content_dependencies WHERE installed_content_id = ?1",
        params![installed_content_id],
    )?;
    connection.execute(
        "DELETE FROM installed_content WHERE id = ?1",
        params![installed_content_id],
    )?;
    Ok(())
}

fn inner_list_update_candidates(
    state: &AppState,
    profile_id: Option<&str>,
) -> AppResult<Vec<UpdateCandidate>> {
    let installed_content = inner_list_installed_content(state, profile_id)?;
    let client = ModrinthClient::new()?;
    let mut updates = Vec::new();

    for record in installed_content {
        if !record.provider.eq_ignore_ascii_case("modrinth") {
            continue;
        }

        let profile = fetch_profile_summary(state, &record.profile_id)?;
        let content_type = ContentType::from_str(&record.content_type)?;
        if content_type == ContentType::Modpack {
            continue;
        }

        let latest = client.get_latest_compatible_version(&record.project_id, &profile, content_type)?;
        if latest.id != record.version_id {
            updates.push(UpdateCandidate {
                installed_content_id: record.id.clone(),
                profile_id: record.profile_id.clone(),
                project_id: record.project_id.clone(),
                current_version_id: record.version_id.clone(),
                target_version_id: latest.id,
                current_version_label: record.version_number.clone(),
                target_version_label: Some(latest.version_number),
                changelog: latest.changelog,
                compatibility_notes: Vec::new(),
            });
        }
    }

    Ok(updates)
}

fn inner_apply_update_candidate(
    state: &AppState,
    installed_content_id: &str,
    target_version_id: &str,
) -> AppResult<InstalledContentRecord> {
    let record = fetch_installed_content_by_id(state, installed_content_id)?;
    let profile = fetch_profile_summary(state, &record.profile_id)?;
    let content_type = ContentType::from_str(&record.content_type)?;

    install_exact_version(
        state,
        &profile,
        &record.project_id,
        target_version_id,
        content_type,
        InstallScope::from_str(&record.install_scope)?,
        record.target_rel_path.as_deref(),
        Some(installed_content_id),
        record.enabled,
    )
}

fn build_install_plan(
    state: &AppState,
    profile: &ProfileSummary,
    project: &ProjectResponse,
    version: &VersionResponse,
    content_type: ContentType,
    install_scope: Option<InstallScope>,
    target_rel_path: Option<&str>,
) -> AppResult<InstallPlan> {
    let file = version.primary_file()?;
    let install_scope = install_scope.unwrap_or(if content_type == ContentType::Datapack {
        InstallScope::World
    } else {
        InstallScope::Profile
    });

    let resolved_target_rel_path = resolve_target_rel_path(content_type, install_scope, target_rel_path, &file.filename)?;
    let target_path = resolve_profile_target_path(state, &profile.directory_path, &resolved_target_rel_path)?;
    let rollback_path = target_path.exists().then(|| format!("{}.bak", target_path.to_string_lossy()));
    let dependencies = version
        .dependencies
        .iter()
        .filter(|dependency| dependency.project_id.is_some())
        .map(map_dependency)
        .collect();

    let compatibility_warnings = if version_is_compatible(profile, content_type, version) {
        Vec::new()
    } else {
        vec!["Selected version is not fully compatible with the active profile.".to_string()]
    };

    Ok(InstallPlan {
        profile_id: profile.id.clone(),
        project_title: project.title.clone(),
        version_label: version.version_number.clone(),
        content_type,
        install_scope,
        project_id: project.id.clone(),
        version_id: version.id.clone(),
        target_rel_path: Some(resolved_target_rel_path),
        target_path: target_path.to_string_lossy().into_owned(),
        rollback_path,
        dependencies,
        compatibility_warnings,
    })
}

pub(crate) fn install_exact_version(
    state: &AppState,
    profile: &ProfileSummary,
    project_id: &str,
    version_id: &str,
    content_type: ContentType,
    install_scope: InstallScope,
    target_rel_path: Option<&str>,
    existing_record_id: Option<&str>,
    enabled: bool,
) -> AppResult<InstalledContentRecord> {
    if content_type == ContentType::Modpack {
        return Err(AppError::Validation(
            "Modrinth modpack ingestion is not implemented in this slice yet".to_string(),
        ));
    }

    let client = ModrinthClient::new()?;
    let project = client.get_project(project_id)?;
    let version = client.get_version(version_id)?;

    let version_content_type = content_type_from_modrinth(&project.project_type)?;
    if version_content_type != content_type {
        return Err(AppError::Validation(format!(
            "project {} is a {}, not a {}",
            project.title, version_content_type, content_type
        )));
    }

    let plan = build_install_plan(
        state,
        profile,
        &project,
        &version,
        content_type,
        Some(install_scope),
        target_rel_path,
    )?;

    let file = version.primary_file()?;
    let bytes = client.download_bytes(&file.url)?;
    let file_hash = sha256_hex(&bytes);
    let temp_file_path = state
        .paths
        .temp_dir
        .join(format!("{}-{}", Uuid::new_v4().simple(), file.filename));
    fs::write(&temp_file_path, &bytes)?;

    let mut final_path = PathBuf::from(&plan.target_path);
    if let Some(parent) = final_path.parent() {
        fs::create_dir_all(parent)?;
    }

    if final_path.exists() {
        let rollback_path = PathBuf::from(
            plan.rollback_path
                .clone()
                .unwrap_or_else(|| format!("{}.bak", final_path.to_string_lossy())),
        );
        if rollback_path.exists() {
            fs::remove_file(&rollback_path)?;
        }
        fs::rename(&final_path, &rollback_path)?;
    }

    fs::rename(&temp_file_path, &final_path)?;
    if !enabled && content_type == ContentType::Mod {
        let disabled_path = append_disabled_suffix(&final_path)?;
        fs::rename(&final_path, &disabled_path)?;
        final_path = disabled_path;
    }

    upsert_installed_content(
        state,
        profile,
        existing_record_id,
        &project,
        &version,
        content_type,
        install_scope,
        plan.target_rel_path.as_deref(),
        &final_path,
        &file_hash,
        enabled,
    )
}

fn upsert_installed_content(
    state: &AppState,
    profile: &ProfileSummary,
    existing_record_id: Option<&str>,
    project: &ProjectResponse,
    version: &VersionResponse,
    content_type: ContentType,
    install_scope: InstallScope,
    target_rel_path: Option<&str>,
    local_file_path: &Path,
    file_hash: &str,
    enabled: bool,
) -> AppResult<InstalledContentRecord> {
    let now = Utc::now().to_rfc3339();
    let record_id = match existing_record_id {
        Some(existing_record_id) => existing_record_id.to_string(),
        None => fetch_existing_content_record(state, &profile.id, &project.id, target_rel_path)?
            .map(|record| record.id)
            .unwrap_or_else(|| format!("content-{}", Uuid::new_v4().simple())),
    };

    {
        let connection = state.db()?;
        connection.execute(
            "
            INSERT INTO installed_content (
              id, profile_id, content_type, install_scope, provider, project_id, version_id, slug, name,
              local_file_path, target_rel_path, file_hash, enabled, version_number, installed_at, updated_at
            )
            VALUES (?1, ?2, ?3, ?4, 'modrinth', ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
            ON CONFLICT(id) DO UPDATE SET
              profile_id = excluded.profile_id,
              content_type = excluded.content_type,
              install_scope = excluded.install_scope,
              provider = excluded.provider,
              project_id = excluded.project_id,
              version_id = excluded.version_id,
              slug = excluded.slug,
              name = excluded.name,
              local_file_path = excluded.local_file_path,
              target_rel_path = excluded.target_rel_path,
              file_hash = excluded.file_hash,
              enabled = excluded.enabled,
              version_number = excluded.version_number,
              updated_at = excluded.updated_at
            ",
            params![
                record_id,
                profile.id,
                content_type.as_str(),
                install_scope.as_str(),
                project.id,
                version.id,
                project.slug,
                project.title,
                local_file_path.to_string_lossy().into_owned(),
                target_rel_path,
                file_hash,
                if enabled { 1 } else { 0 },
                version.version_number,
                now,
                now,
            ],
        )?;

        connection.execute(
            "DELETE FROM content_dependencies WHERE installed_content_id = ?1",
            params![record_id],
        )?;

        for dependency in version.dependencies.iter().filter(|dependency| dependency.project_id.is_some()) {
            connection.execute(
                "
                INSERT INTO content_dependencies (id, installed_content_id, dependency_project_id, dependency_version_id, dependency_kind)
                VALUES (?1, ?2, ?3, ?4, ?5)
                ",
                params![
                    format!("dep-{}", Uuid::new_v4().simple()),
                    record_id,
                    dependency.project_id,
                    dependency.version_id,
                    dependency.dependency_type,
                ],
            )?;
        }
    }

    fetch_installed_content_by_id(state, &record_id)
}

fn resolve_target_rel_path(
    content_type: ContentType,
    install_scope: InstallScope,
    target_rel_path: Option<&str>,
    filename: &str,
) -> AppResult<String> {
    match content_type {
        ContentType::Mod => Ok(format!("minecraft/mods/{filename}")),
        ContentType::ResourcePack => Ok(format!("minecraft/resourcepacks/{filename}")),
        ContentType::ShaderPack => Ok(format!("minecraft/shaderpacks/{filename}")),
        ContentType::Datapack => {
            if install_scope != InstallScope::World {
                return Err(AppError::Validation(
                    "datapacks must be installed with world scope".to_string(),
                ));
            }

            let raw_target = target_rel_path.ok_or_else(|| {
                AppError::Validation(
                    "datapack installs require a target world name or relative path".to_string(),
                )
            })?;
            let normalized = normalize_datapack_target(raw_target)?;
            Ok(format!("{normalized}/{filename}"))
        }
        ContentType::Modpack => Err(AppError::Validation(
            "modpack install path resolution is not implemented yet".to_string(),
        )),
    }
}

fn normalize_datapack_target(value: &str) -> AppResult<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(AppError::Validation(
            "datapack target cannot be empty".to_string(),
        ));
    }

    if trimmed.contains('/') || trimmed.contains('\\') {
        let normalized = normalize_relative_path(trimmed)?;
        if !normalized.starts_with("minecraft/saves/") || !normalized.ends_with("datapacks") {
            return Err(AppError::Validation(
                "datapack target paths must stay under minecraft/saves/<world>/datapacks".to_string(),
            ));
        }
        return Ok(normalized);
    }

    Ok(format!("minecraft/saves/{trimmed}/datapacks"))
}

fn normalize_relative_path(value: &str) -> AppResult<String> {
    let path = Path::new(value);
    if path.is_absolute() {
        return Err(AppError::Path(
            "absolute paths are not allowed for relative profile targets".to_string(),
        ));
    }

    let mut normalized = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => normalized.push(part.to_string_lossy().into_owned()),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(AppError::Path(
                    "relative target paths cannot traverse outside the profile".to_string(),
                ))
            }
        }
    }

    if normalized.is_empty() {
        return Err(AppError::Validation(
            "relative target path cannot be empty".to_string(),
        ));
    }

    Ok(normalized.join("/"))
}

fn resolve_profile_target_path(
    state: &AppState,
    profile_root: &str,
    target_rel_path: &str,
) -> AppResult<PathBuf> {
    let profile_root = PathBuf::from(profile_root);
    let normalized = normalize_relative_path(target_rel_path)?;
    let joined = normalized
        .split('/')
        .fold(profile_root.clone(), |path, part| path.join(part));
    ensure_profile_target(&state.paths.profiles_dir, &joined)?;
    Ok(joined)
}

fn fetch_profile_summary(state: &AppState, profile_id: &str) -> AppResult<ProfileSummary> {
    let connection = state.db()?;
    connection
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
        .optional()?
        .ok_or_else(|| AppError::NotFound(format!("profile not found: {profile_id}")))
}

fn fetch_existing_content_record(
    state: &AppState,
    profile_id: &str,
    project_id: &str,
    target_rel_path: Option<&str>,
) -> AppResult<Option<InstalledContentRecord>> {
    let connection = state.db()?;
    let existing_id: Option<String> = if let Some(target_rel_path) = target_rel_path {
        connection
            .query_row(
                "
                SELECT id
                FROM installed_content
                WHERE profile_id = ?1 AND project_id = ?2 AND target_rel_path = ?3
                LIMIT 1
                ",
                params![profile_id, project_id, target_rel_path],
                |row| row.get(0),
            )
            .optional()?
    } else {
        connection
            .query_row(
                "
                SELECT id
                FROM installed_content
                WHERE profile_id = ?1 AND project_id = ?2
                ORDER BY updated_at DESC
                LIMIT 1
                ",
                params![profile_id, project_id],
                |row| row.get(0),
            )
            .optional()?
    };

    existing_id
        .as_deref()
        .map(|id| fetch_installed_content_by_id(state, id))
        .transpose()
}

fn fetch_installed_content_by_id(
    state: &AppState,
    installed_content_id: &str,
) -> AppResult<InstalledContentRecord> {
    let connection = state.db()?;
    connection
        .query_row(
            "
            SELECT id, profile_id, content_type, install_scope, provider, project_id, version_id, slug, name,
                   local_file_path, target_rel_path, file_hash, enabled, version_number, installed_at, updated_at
            FROM installed_content
            WHERE id = ?1
            ",
            params![installed_content_id],
            installed_content_from_row,
        )
        .optional()?
        .ok_or_else(|| AppError::NotFound(format!("installed content not found: {installed_content_id}")))
}

fn installed_content_from_row(row: &Row<'_>) -> rusqlite::Result<InstalledContentRecord> {
    Ok(InstalledContentRecord {
        id: row.get(0)?,
        profile_id: row.get(1)?,
        content_type: row.get(2)?,
        install_scope: row.get(3)?,
        provider: row.get(4)?,
        project_id: row.get(5)?,
        version_id: row.get(6)?,
        slug: row.get(7)?,
        name: row.get(8)?,
        local_file_path: row.get(9)?,
        target_rel_path: row.get(10)?,
        file_hash: row.get(11)?,
        enabled: row.get::<_, i64>(12)? != 0,
        version_number: row.get(13)?,
        installed_at: row.get(14)?,
        updated_at: row.get(15)?,
    })
}

fn profile_summary_from_row(row: &Row<'_>) -> rusqlite::Result<ProfileSummary> {
    let profile_type: String = row.get(2)?;

    Ok(ProfileSummary {
        id: row.get(0)?,
        name: row.get(1)?,
        profile_type: crate::dto::ProfileType::from_str(&profile_type).map_err(|error| {
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

fn map_dependency(dependency: &VersionDependency) -> DependencyWarning {
    DependencyWarning {
        project_id: dependency.project_id.clone().unwrap_or_default(),
        version_id: dependency.version_id.clone(),
        kind: dependency.dependency_type.clone(),
        reason: match dependency.dependency_type.as_str() {
            "required" => "Required dependency".to_string(),
            "optional" => "Optional dependency".to_string(),
            "incompatible" => "Known incompatible project".to_string(),
            other => format!("Dependency relationship: {other}"),
        },
    }
}

fn append_disabled_suffix(path: &Path) -> AppResult<PathBuf> {
    let file_name = path.file_name().and_then(|name| name.to_str()).ok_or_else(|| {
        AppError::Path("could not resolve installed content filename".to_string())
    })?;
    Ok(path.with_file_name(format!("{file_name}.disabled")))
}

fn remove_disabled_suffix(path: &Path) -> AppResult<PathBuf> {
    let file_name = path.file_name().and_then(|name| name.to_str()).ok_or_else(|| {
        AppError::Path("could not resolve installed content filename".to_string())
    })?;
    let restored = file_name.strip_suffix(".disabled").ok_or_else(|| {
        AppError::Validation("mod is already enabled".to_string())
    })?;
    Ok(path.with_file_name(restored))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let hash = hasher.finalize();
    format!("{hash:x}")
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use super::{
        inner_apply_install_plan, inner_create_install_plan, inner_search_modrinth,
        normalize_datapack_target, normalize_relative_path,
    };
    use crate::{
        commands::profiles::inner_create_profile,
        dto::{
            ApplyInstallPlanInput, ContentType, CreateInstallPlanInput, CreateProfileInput,
            ModrinthSearchInput, ProfileSummary, ProfileType,
        },
        minecraft,
        modrinth::ModrinthClient,
        paths::AppPaths,
        state::AppState,
    };
    use uuid::Uuid;

    struct TestRoot {
        root: PathBuf,
    }

    impl TestRoot {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir()
                .join("blocksmith-live-tests")
                .join(format!("{name}-{}", Uuid::new_v4().simple()));
            Self { root }
        }

        fn paths(&self) -> AppPaths {
            AppPaths {
                root_dir: self.root.clone(),
                db_path: self.root.join("db.sqlite"),
                cache_dir: self.root.join("cache"),
                logs_dir: self.root.join("logs"),
                profiles_dir: self.root.join("profiles"),
                skins_dir: self.root.join("skins"),
                exports_dir: self.root.join("exports"),
                runtimes_dir: self.root.join("runtimes"),
                temp_dir: self.root.join("temp"),
            }
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn probe_profile(version: &str) -> ProfileSummary {
        ProfileSummary {
            id: "probe".to_string(),
            name: "Probe".to_string(),
            profile_type: ProfileType::Fabric,
            minecraft_version: version.to_string(),
            loader_version: Some("latest".to_string()),
            directory_path: "C:\\probe".to_string(),
            account_id: None,
            java_path: None,
            memory_min_mb: None,
            memory_max_mb: None,
            jvm_args: String::new(),
            launch_args: String::new(),
            notes: None,
            created_at: String::new(),
            updated_at: String::new(),
            last_played_at: None,
        }
    }

    #[test]
    fn normalize_relative_path_rejects_parent_dirs() {
        assert!(normalize_relative_path("../outside").is_err());
        assert!(normalize_relative_path("minecraft/mods").is_ok());
    }

    #[test]
    fn normalize_datapack_target_supports_world_name_shortcut() {
        let path = normalize_datapack_target("WorldOne").expect("should normalize");
        assert_eq!(path, "minecraft/saves/WorldOne/datapacks");
    }

    #[test]
    #[ignore = "live network smoke test"]
    fn live_can_create_fabric_profile_and_install_sodium() {
        let started_at = std::time::Instant::now();
        let test_root = TestRoot::new("fabric-sodium");
        let paths = test_root.paths();
        paths.ensure_layout().expect("should create test app layout");
        let state = AppState::bootstrap(paths).expect("should bootstrap test app state");
        eprintln!("[live-smoke] bootstrapped isolated state in {:?}", started_at.elapsed());

        let sodium_search = inner_search_modrinth(
            &state,
            ModrinthSearchInput {
                query: "sodium".to_string(),
                profile_id: None,
                content_type: Some(ContentType::Mod),
            },
        )
        .expect("global sodium search should succeed");
        eprintln!(
            "[live-smoke] Modrinth search returned {} results in {:?}",
            sodium_search.len(),
            started_at.elapsed()
        );
        let sodium = sodium_search
            .iter()
            .find(|result| result.slug == "sodium")
            .unwrap_or_else(|| panic!("expected sodium search result, got {} results", sodium_search.len()));
        eprintln!(
            "[live-smoke] using project {} ({})",
            sodium.title,
            sodium.project_id
        );

        let release_versions = minecraft::list_minecraft_versions()
            .expect("should load Mojang version manifest")
            .into_iter()
            .filter(|entry| entry.kind == "release")
            .map(|entry| entry.id)
            .collect::<Vec<_>>();
        eprintln!(
            "[live-smoke] loaded {} release versions in {:?}",
            release_versions.len(),
            started_at.elapsed()
        );

        let chosen_version = release_versions
            .iter()
            .find(|version| sodium.supported_versions.iter().any(|candidate| candidate == *version))
            .cloned()
            .or_else(|| {
                let client = ModrinthClient::new().ok()?;
                release_versions
                    .iter()
                    .find(|version| {
                        client
                            .get_latest_compatible_version(
                                &sodium.project_id,
                                &probe_profile(version),
                                ContentType::Mod,
                            )
                            .is_ok()
                    })
                    .cloned()
            })
            .expect("should find a release version compatible with Sodium");
        eprintln!(
            "[live-smoke] selected Minecraft {} in {:?}",
            chosen_version,
            started_at.elapsed()
        );

        let profile = inner_create_profile(
            &state,
            CreateProfileInput {
                name: "Live Fabric Sodium".to_string(),
                profile_type: ProfileType::Fabric,
                minecraft_version: chosen_version.clone(),
                loader_version: Some("latest".to_string()),
                account_id: None,
                java_path: None,
                memory_min_mb: None,
                memory_max_mb: None,
                jvm_args: Some(String::new()),
                launch_args: Some(String::new()),
                notes: Some("live smoke test".to_string()),
            },
        )
        .expect("should create a Fabric profile");
        eprintln!(
            "[live-smoke] created profile {} at {} in {:?}",
            profile.summary.id,
            profile.summary.directory_path,
            started_at.elapsed()
        );

        let profile_search = inner_search_modrinth(
            &state,
            ModrinthSearchInput {
                query: "sodium".to_string(),
                profile_id: Some(profile.summary.id.clone()),
                content_type: Some(ContentType::Mod),
            },
        )
        .expect("profile-aware sodium search should succeed");
        assert!(
            profile_search.iter().any(|result| result.slug == "sodium"),
            "expected profile-aware search to include sodium"
        );
        eprintln!(
            "[live-smoke] profile-aware search succeeded in {:?}",
            started_at.elapsed()
        );

        let plan = inner_create_install_plan(
            &state,
            CreateInstallPlanInput {
                profile_id: profile.summary.id.clone(),
                project_id: sodium.project_id.clone(),
                content_type: ContentType::Mod,
                install_scope: None,
                target_rel_path: None,
            },
        )
        .expect("should create an install plan for Sodium");
        eprintln!(
            "[live-smoke] install plan targets {} in {:?}",
            plan.target_path,
            started_at.elapsed()
        );

        let installed = inner_apply_install_plan(
            &state,
            ApplyInstallPlanInput { plan },
        )
        .expect("should install Sodium successfully");
        eprintln!(
            "[live-smoke] install completed at {} in {:?}",
            installed.local_file_path,
            started_at.elapsed()
        );

        assert_eq!(installed.project_id, sodium.project_id);
        assert_eq!(installed.content_type, "mod");
        assert!(
            installed.local_file_path.contains("minecraft"),
            "installed mod path should stay inside the profile"
        );
        assert!(
            installed.local_file_path.ends_with(".jar"),
            "installed file should be a jar"
        );
        assert!(
            std::path::Path::new(&installed.local_file_path).exists(),
            "installed Sodium jar should exist on disk"
        );
    }
}
