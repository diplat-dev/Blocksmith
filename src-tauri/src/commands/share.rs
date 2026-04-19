use std::{fs, io::Cursor, str::FromStr};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::Utc;
use rusqlite::params;
use tauri::State;
use uuid::Uuid;

use crate::{
    commands::profiles::inner_create_profile,
    dto::{
        ContentType, CreateProfileInput, ImportShareFileInput, ImportShareInput, ProfileDetail,
        ShareExportResult, SharedContentReference, SharedProfileManifest,
    },
    error::{AppError, AppResult},
    state::AppState,
};

use super::content::{inner_list_installed_content, install_exact_version};

#[tauri::command]
pub fn export_profile_share(
    state: State<'_, AppState>,
    profile_id: String,
) -> Result<ShareExportResult, String> {
    inner_export_profile_share(&state, &profile_id).map_err(Into::into)
}

#[tauri::command]
pub fn import_profile_share(
    state: State<'_, AppState>,
    input: ImportShareInput,
) -> Result<ProfileDetail, String> {
    inner_import_profile_share(&state, input).map_err(Into::into)
}

#[tauri::command]
pub fn import_profile_share_file(
    state: State<'_, AppState>,
    input: ImportShareFileInput,
) -> Result<ProfileDetail, String> {
    inner_import_profile_share_file(&state, input).map_err(Into::into)
}

fn inner_export_profile_share(state: &AppState, profile_id: &str) -> AppResult<ShareExportResult> {
    let detail = crate::commands::profiles::inner_get_profile_detail(state, profile_id)?;
    let installed_content = inner_list_installed_content(state, Some(profile_id))?;

    let mut content = installed_content
        .into_iter()
        .map(|record| SharedContentReference {
            project_id: record.project_id,
            version_id: record.version_id,
            content_type: record.content_type,
            install_scope: record.install_scope,
            target_rel_path: record.target_rel_path,
        })
        .collect::<Vec<_>>();
    content.sort_by(|left, right| {
        (&left.content_type, &left.project_id, &left.version_id, &left.target_rel_path).cmp(
            &(&right.content_type, &right.project_id, &right.version_id, &right.target_rel_path),
        )
    });

    let manifest = SharedProfileManifest {
        export_version: 1,
        profile_name: detail.summary.name.clone(),
        profile_type: detail.summary.profile_type,
        minecraft_version: detail.summary.minecraft_version.clone(),
        loader_version: detail.summary.loader_version.clone(),
        java_path: detail.summary.java_path.clone(),
        memory_min_mb: detail.summary.memory_min_mb,
        memory_max_mb: detail.summary.memory_max_mb,
        jvm_args: detail.summary.jvm_args.clone(),
        launch_args: detail.summary.launch_args.clone(),
        notes: detail.summary.notes.clone(),
        content,
    };

    let manifest_json = serde_json::to_vec(&manifest)?;
    let share_code = URL_SAFE_NO_PAD.encode(zstd::stream::encode_all(Cursor::new(&manifest_json), 6)?);
    let export_path = state
        .paths
        .exports_dir
        .join(format!("{}-{}.blocksmith.json", profile_id, Uuid::new_v4().simple()));
    fs::write(&export_path, serde_json::to_vec_pretty(&manifest)?)?;

    let now = Utc::now().to_rfc3339();
    let connection = state.db()?;
    connection.execute(
        "
        INSERT INTO profile_exports (id, profile_id, export_version, manifest_json, created_at)
        VALUES (?1, ?2, ?3, ?4, ?5)
        ",
        params![
            format!("export-{}", Uuid::new_v4().simple()),
            profile_id,
            manifest.export_version,
            String::from_utf8_lossy(&manifest_json).into_owned(),
            now,
        ],
    )?;

    Ok(ShareExportResult {
        share_code,
        export_path: export_path.to_string_lossy().into_owned(),
        manifest,
    })
}

fn inner_import_profile_share(state: &AppState, input: ImportShareInput) -> AppResult<ProfileDetail> {
    let decoded = URL_SAFE_NO_PAD
        .decode(input.share_code.trim())
        .map_err(|error| AppError::Validation(format!("invalid Blocksmith share code: {error}")))?;
    let manifest_bytes = zstd::stream::decode_all(Cursor::new(decoded))
        .map_err(|error| AppError::Validation(format!("invalid compressed share payload: {error}")))?;
    let manifest: SharedProfileManifest = serde_json::from_slice(&manifest_bytes)?;
    import_manifest(state, manifest, input.new_name)
}

fn inner_import_profile_share_file(
    state: &AppState,
    input: ImportShareFileInput,
) -> AppResult<ProfileDetail> {
    let source_path = input.source_path.trim();
    if source_path.is_empty() {
        return Err(AppError::Validation(
            "share manifest source path cannot be empty".to_string(),
        ));
    }

    let manifest_bytes = fs::read(source_path)?;
    let manifest: SharedProfileManifest = serde_json::from_slice(&manifest_bytes)?;
    import_manifest(
        state,
        manifest,
        input.new_name,
    )
}

fn import_manifest(
    state: &AppState,
    manifest: SharedProfileManifest,
    override_name: Option<String>,
) -> AppResult<ProfileDetail> {
    let new_profile_name = override_name
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| manifest.profile_name.clone());

    let profile = inner_create_profile(
        state,
        CreateProfileInput {
            name: new_profile_name,
            profile_type: manifest.profile_type,
            minecraft_version: manifest.minecraft_version.clone(),
            loader_version: manifest.loader_version.clone(),
            account_id: None,
            java_path: manifest.java_path.clone(),
            memory_min_mb: manifest.memory_min_mb,
            memory_max_mb: manifest.memory_max_mb,
            jvm_args: Some(manifest.jvm_args.clone()),
            launch_args: Some(manifest.launch_args.clone()),
            notes: manifest.notes.clone(),
        },
    )?;

    for reference in manifest.content {
        let content_type = ContentType::from_str(&reference.content_type)?;
        if content_type == ContentType::Modpack {
            return Err(AppError::Validation(
                "shared modpack references are not implemented in this slice yet".to_string(),
            ));
        }

        install_exact_version(
            state,
            &profile.summary,
            &reference.project_id,
            &reference.version_id,
            content_type,
            crate::dto::InstallScope::from_str(&reference.install_scope)?,
            reference.target_rel_path.as_deref(),
            None,
            true,
        )?;
    }

    crate::commands::profiles::inner_get_profile_detail(state, &profile.summary.id)
}
