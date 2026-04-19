use std::{
    collections::HashMap,
    fs,
    io::{Cursor, Read},
    path::{Component, Path, PathBuf},
};

use chrono::Utc;
use reqwest::blocking::Client;
use rusqlite::params;
use serde::Deserialize;
use sha1::{Digest as Sha1Digest, Sha1};
use sha2::Sha512;
use uuid::Uuid;
use zip::ZipArchive;

use crate::{
    commands::profiles::inner_create_profile,
    dto::{
        ContentType, CreateProfileInput, ImportMrpackInput, InstallModpackInput, InstallScope,
        ProfileDetail, ProfileType,
    },
    error::{AppError, AppResult},
    modrinth::{content_type_from_modrinth, ModrinthClient},
    profile_fs::ensure_profile_target,
    state::AppState,
};

pub fn import_mrpack(state: &AppState, input: ImportMrpackInput) -> AppResult<ProfileDetail> {
    let source_path = PathBuf::from(input.source_path.trim());
    if !source_path.exists() {
        return Err(AppError::NotFound(format!(
            ".mrpack source file not found: {}",
            source_path.to_string_lossy()
        )));
    }

    let bytes = fs::read(&source_path)?;
    let source_label = source_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("local.mrpack")
        .to_string();
    import_mrpack_bytes(state, &bytes, input.new_name, Some(source_label))
}

pub fn install_modrinth_modpack(
    state: &AppState,
    input: InstallModpackInput,
) -> AppResult<ProfileDetail> {
    let client = ModrinthClient::new()?;
    let project = client.get_project(&input.project_id)?;
    let project_type = content_type_from_modrinth(&project.project_type)?;
    if project_type != ContentType::Modpack {
        return Err(AppError::Validation(format!(
            "project {} is a {}, not a modpack",
            project.title, project_type
        )));
    }

    let version = client
        .get_project_versions(&input.project_id)?
        .into_iter()
        .find(|version| {
            version
                .primary_file()
                .ok()
                .is_some_and(|file| file.filename.to_ascii_lowercase().ends_with(".mrpack"))
        })
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "project {} does not expose a downloadable .mrpack version",
                project.title
            ))
        })?;
    let file = version.primary_file()?;
    let bytes = client.download_bytes(&file.url)?;
    import_mrpack_bytes(
        state,
        &bytes,
        input.new_name,
        Some(format!("modrinth:{}@{}", project.slug, version.version_number)),
    )
}

fn import_mrpack_bytes(
    state: &AppState,
    bytes: &[u8],
    new_name: Option<String>,
    source_label: Option<String>,
) -> AppResult<ProfileDetail> {
    let mut archive = ZipArchive::new(Cursor::new(bytes))?;
    let index = read_modpack_index(&mut archive)?;
    validate_modpack_index(&index)?;

    let minecraft_version = index
        .dependencies
        .get("minecraft")
        .cloned()
        .ok_or_else(|| AppError::Validation("modpack is missing a minecraft dependency".to_string()))?;
    let fabric_loader = index.dependencies.get("fabric-loader").cloned();
    let profile_type = if fabric_loader.is_some() {
        ProfileType::Fabric
    } else {
        ProfileType::Vanilla
    };

    if index
        .dependencies
        .keys()
        .any(|key| matches!(key.as_str(), "forge" | "neoforge" | "quilt-loader"))
    {
        return Err(AppError::Validation(
            "this Blocksmith build only imports vanilla and Fabric Modrinth packs".to_string(),
        ));
    }

    let mut notes = index.summary.clone();
    if let Some(source_label) = source_label {
        let prefix = format!("Imported from {source_label}");
        notes = Some(match notes {
            Some(existing) if !existing.trim().is_empty() => format!("{prefix}\n\n{existing}"),
            _ => prefix,
        });
    }

    let profile = inner_create_profile(
        state,
        CreateProfileInput {
            name: new_name
                .filter(|value| !value.trim().is_empty())
                .unwrap_or(index.name.clone()),
            profile_type,
            minecraft_version,
            loader_version: fabric_loader,
            account_id: None,
            java_path: None,
            memory_min_mb: None,
            memory_max_mb: None,
            jvm_args: Some(String::new()),
            launch_args: Some(String::new()),
            notes,
        },
    )?;

    let instance_root = PathBuf::from(&profile.summary.directory_path).join("minecraft");
    let http = Client::builder()
        .user_agent("Blocksmith/0.1.0")
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(180))
        .build()?;

    for file in index.files.iter().filter(should_install_on_client) {
        install_mrpack_file(state, &profile, &http, &instance_root, file)?;
    }

    apply_archive_layer(&mut archive, "overrides", &instance_root, state)?;
    apply_archive_layer(&mut archive, "client-overrides", &instance_root, state)?;

    Ok(profile)
}

fn read_modpack_index(archive: &mut ZipArchive<Cursor<&[u8]>>) -> AppResult<MrpackIndex> {
    let mut file = archive
        .by_name("modrinth.index.json")
        .map_err(|_| AppError::Validation("mrpack is missing modrinth.index.json".to_string()))?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;
    serde_json::from_str(&content).map_err(Into::into)
}

fn validate_modpack_index(index: &MrpackIndex) -> AppResult<()> {
    if index.format_version != 1 {
        return Err(AppError::Validation(format!(
            "unsupported mrpack format version: {}",
            index.format_version
        )));
    }

    if !index.game.eq_ignore_ascii_case("minecraft") {
        return Err(AppError::Validation(
            "only Minecraft Modrinth packs are supported".to_string(),
        ));
    }

    Ok(())
}

fn should_install_on_client(file: &&MrpackFile) -> bool {
    file.env
        .as_ref()
        .and_then(|env| env.client.as_deref())
        .map(|value| !value.eq_ignore_ascii_case("unsupported"))
        .unwrap_or(true)
}

fn install_mrpack_file(
    state: &AppState,
    profile: &ProfileDetail,
    http: &Client,
    instance_root: &Path,
    file: &MrpackFile,
) -> AppResult<()> {
    let relative_path = normalize_instance_relative_path(&file.path)?;
    let final_path = safe_instance_join(state, instance_root, &relative_path)?;
    let parent = final_path.parent().ok_or_else(|| {
        AppError::Path("could not resolve mrpack file destination directory".to_string())
    })?;
    fs::create_dir_all(parent)?;

    let download_url = file
        .downloads
        .first()
        .cloned()
        .ok_or_else(|| AppError::Validation(format!("mrpack entry has no download URL: {}", file.path)))?;
    let bytes = http.get(download_url).send()?.error_for_status()?.bytes()?.to_vec();
    verify_mrpack_hashes(file, &bytes)?;

    let temp_path = state
        .paths
        .temp_dir
        .join(format!("mrpack-{}-{}", Uuid::new_v4().simple(), file_name_or_fallback(&relative_path)));
    fs::write(&temp_path, &bytes)?;
    if final_path.exists() {
        fs::remove_file(&final_path)?;
    }
    fs::rename(&temp_path, &final_path)?;

    if let Some((content_type, install_scope, target_rel_path, enabled)) =
        infer_installed_content(&relative_path)
    {
        persist_mrpack_content_record(
            state,
            &profile.summary.id,
            content_type,
            install_scope,
            &target_rel_path,
            &final_path,
            &bytes,
            enabled,
        )?;
    }

    Ok(())
}

fn verify_mrpack_hashes(file: &MrpackFile, bytes: &[u8]) -> AppResult<()> {
    if let Some(expected_sha1) = file.hashes.get("sha1") {
        let mut hasher = Sha1::new();
        hasher.update(bytes);
        let digest = format!("{:x}", hasher.finalize());
        if digest != *expected_sha1 {
            return Err(AppError::Validation(format!(
                "mrpack file failed SHA1 validation: {}",
                file.path
            )));
        }
    }

    if let Some(expected_sha512) = file.hashes.get("sha512") {
        let mut hasher = Sha512::new();
        hasher.update(bytes);
        let digest = format!("{:x}", hasher.finalize());
        if digest != *expected_sha512 {
            return Err(AppError::Validation(format!(
                "mrpack file failed SHA512 validation: {}",
                file.path
            )));
        }
    }

    Ok(())
}

fn apply_archive_layer(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    prefix: &str,
    instance_root: &Path,
    state: &AppState,
) -> AppResult<()> {
    let mut entries = Vec::new();
    for index in 0..archive.len() {
        let name = {
            let file = archive.by_index(index)?;
            file.name().replace('\\', "/")
        };
        if name == prefix || !name.starts_with(&format!("{prefix}/")) {
            continue;
        }
        entries.push((index, name));
    }

    for (index, name) in entries {
        let mut file = archive.by_index(index)?;
        if file.is_dir() {
            continue;
        }

        let relative_path = name
            .strip_prefix(&format!("{prefix}/"))
            .ok_or_else(|| AppError::Path(format!("invalid archive path: {name}")))?;
        let relative_path = normalize_instance_relative_path(relative_path)?;
        let final_path = safe_instance_join(state, instance_root, &relative_path)?;

        if let Some(parent) = final_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        fs::write(final_path, bytes)?;
    }

    Ok(())
}

fn safe_instance_join(state: &AppState, instance_root: &Path, relative_path: &str) -> AppResult<PathBuf> {
    let joined = relative_path
        .split('/')
        .fold(instance_root.to_path_buf(), |path, part| path.join(part));
    ensure_profile_target(&state.paths.profiles_dir, &joined)?;
    Ok(joined)
}

fn normalize_instance_relative_path(value: &str) -> AppResult<String> {
    let path = Path::new(value);
    if path.is_absolute() {
        return Err(AppError::Path(
            "mrpack paths must stay relative to the instance root".to_string(),
        ));
    }

    let mut normalized = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => normalized.push(part.to_string_lossy().into_owned()),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(AppError::Path(
                    "mrpack paths cannot traverse outside the instance".to_string(),
                ))
            }
        }
    }

    if normalized.is_empty() {
        return Err(AppError::Validation(
            "mrpack file path cannot be empty".to_string(),
        ));
    }

    Ok(normalized.join("/"))
}

fn infer_installed_content(
    relative_path: &str,
) -> Option<(ContentType, InstallScope, String, bool)> {
    let lower = relative_path.to_ascii_lowercase();
    let enabled = !lower.ends_with(".disabled");

    if lower.starts_with("mods/") {
        return Some((
            ContentType::Mod,
            InstallScope::Profile,
            format!("minecraft/{relative_path}"),
            enabled,
        ));
    }

    if lower.starts_with("resourcepacks/") {
        return Some((
            ContentType::ResourcePack,
            InstallScope::Profile,
            format!("minecraft/{relative_path}"),
            true,
        ));
    }

    if lower.starts_with("shaderpacks/") {
        return Some((
            ContentType::ShaderPack,
            InstallScope::Profile,
            format!("minecraft/{relative_path}"),
            true,
        ));
    }

    if lower.starts_with("datapacks/") {
        return Some((
            ContentType::Datapack,
            InstallScope::Profile,
            format!("minecraft/{relative_path}"),
            true,
        ));
    }

    if lower.starts_with("saves/") && lower.contains("/datapacks/") {
        return Some((
            ContentType::Datapack,
            InstallScope::World,
            format!("minecraft/{relative_path}"),
            true,
        ));
    }

    None
}

fn persist_mrpack_content_record(
    state: &AppState,
    profile_id: &str,
    content_type: ContentType,
    install_scope: InstallScope,
    target_rel_path: &str,
    local_file_path: &Path,
    bytes: &[u8],
    enabled: bool,
) -> AppResult<()> {
    let connection = state.db()?;
    let now = Utc::now().to_rfc3339();
    let hash = {
        let mut hasher = Sha1::new();
        hasher.update(bytes);
        format!("{:x}", hasher.finalize())
    };
    let slug = local_file_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("mrpack-file")
        .trim_end_matches(".disabled")
        .to_string();
    let record_id = format!("content-{}", Uuid::new_v4().simple());

    connection.execute(
        "
        INSERT INTO installed_content (
          id, profile_id, content_type, install_scope, provider, project_id, version_id, slug, name,
          local_file_path, target_rel_path, file_hash, enabled, version_number, installed_at, updated_at
        )
        VALUES (?1, ?2, ?3, ?4, 'mrpack', ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, NULL, ?13, ?14)
        ",
        params![
            record_id,
            profile_id,
            content_type.as_str(),
            install_scope.as_str(),
            format!("mrpack:{hash}"),
            hash,
            slug,
            local_file_path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("Imported file"),
            local_file_path.to_string_lossy().into_owned(),
            target_rel_path,
            hash,
            if enabled { 1 } else { 0 },
            now,
            now,
        ],
    )?;

    Ok(())
}

fn file_name_or_fallback(relative_path: &str) -> String {
    Path::new(relative_path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("imported.bin")
        .to_string()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MrpackIndex {
    format_version: u32,
    game: String,
    name: String,
    summary: Option<String>,
    #[serde(default)]
    files: Vec<MrpackFile>,
    #[serde(default)]
    dependencies: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MrpackFile {
    path: String,
    downloads: Vec<String>,
    hashes: HashMap<String, String>,
    #[serde(default)]
    env: Option<MrpackFileEnv>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MrpackFileEnv {
    client: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{infer_installed_content, normalize_instance_relative_path};

    #[test]
    fn mrpack_paths_reject_parent_directory_segments() {
        assert!(normalize_instance_relative_path("../mods/evil.jar").is_err());
        assert!(normalize_instance_relative_path("mods/sodium.jar").is_ok());
    }

    #[test]
    fn mrpack_mods_map_to_profile_mod_content() {
        let inferred = infer_installed_content("mods/example.jar").expect("should infer content");
        assert_eq!(inferred.0.as_str(), "mod");
        assert_eq!(inferred.1.as_str(), "profile");
        assert!(inferred.3);
    }
}
