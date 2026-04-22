use std::{
    collections::{BTreeMap, HashMap},
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use reqwest::blocking::Client;
use rayon::prelude::*;
use rusqlite::OptionalExtension;
use serde::Deserialize;
use sha1::{Digest, Sha1};
use uuid::Uuid;
use zip::ZipArchive;

use crate::{
    auth::{ensure_launcher_unlocked, resolve_launch_auth_session, LaunchAuthSession},
    dto::{FabricLoaderSummary, LaunchPlan, MinecraftVersionSummary, ProfileSummary, ProfileType},
    error::{AppError, AppResult},
    paths::AppPaths,
    state::AppState,
};

const VERSION_MANIFEST_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const FABRIC_LOADERS_URL: &str = "https://meta.fabricmc.net/v2/versions/loader";
const FABRIC_PROFILE_URL: &str = "https://meta.fabricmc.net/v2/versions/loader/{minecraft}/{loader}/profile/json";
const JAVA_RUNTIME_ALL_URL: &str =
    "https://launchermeta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json";
const DOWNLOAD_ATTEMPTS: usize = 3;

pub struct PreparedLaunch {
    pub plan: LaunchPlan,
    pub command: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LaunchResolutionMode {
    Preview,
    Prepare,
}

pub fn list_minecraft_versions() -> AppResult<Vec<MinecraftVersionSummary>> {
    let client = http_client()?;
    let manifest = fetch_version_manifest(&client)?;
    Ok(manifest
        .versions
        .into_iter()
        .map(|entry| MinecraftVersionSummary {
            id: entry.id,
            kind: entry.kind,
            release_time: entry.release_time,
        })
        .collect())
}

pub fn list_fabric_loader_versions(minecraft_version: &str) -> AppResult<Vec<FabricLoaderSummary>> {
    let client = http_client()?;
    let response = client
        .get(format!("{FABRIC_LOADERS_URL}/{minecraft_version}"))
        .send()?
        .error_for_status()?;
    let payload: Vec<FabricLoaderVersionEnvelope> = response.json()?;

    Ok(payload
        .into_iter()
        .map(|entry| FabricLoaderSummary {
            version: entry.loader.version,
            stable: entry.loader.stable,
        })
        .collect())
}

pub fn resolve_launch_plan(state: &AppState, profile: &ProfileSummary) -> AppResult<LaunchPlan> {
    Ok(resolve_launch_plan_inner(state, profile, LaunchResolutionMode::Preview)?.plan)
}

pub fn prepare_launch(state: &AppState, profile: &ProfileSummary) -> AppResult<PreparedLaunch> {
    ensure_launcher_unlocked(state)?;
    resolve_launch_plan_inner(state, profile, LaunchResolutionMode::Prepare)
}

fn resolve_launch_plan_inner(
    state: &AppState,
    profile: &ProfileSummary,
    mode: LaunchResolutionMode,
) -> AppResult<PreparedLaunch> {
    let client = http_client()?;
    let manifest = fetch_version_manifest(&client)?;
    let version = resolve_version_metadata(&client, &manifest, profile)?;
    let cache = ensure_cache_layout(&state.paths)?;
    let minecraft_dir = PathBuf::from(&profile.directory_path).join("minecraft");
    if mode == LaunchResolutionMode::Prepare {
        fs::create_dir_all(&minecraft_dir)?;
    }

    let auth_session = match mode {
        LaunchResolutionMode::Preview => {
            resolve_launch_auth_preview(state, profile.account_id.as_deref(), &profile.name)?
        }
        LaunchResolutionMode::Prepare => {
            resolve_launch_auth_session(state, profile.account_id.as_deref(), &profile.name)?
        }
    };
    let client_download = version
        .downloads
        .client
        .clone()
        .ok_or_else(|| AppError::Validation("version metadata did not include a client download".to_string()))?;
    let version_root = cache.versions_dir.join(&version.id);
    let asset_index = version
        .asset_index
        .clone()
        .ok_or_else(|| AppError::Validation("version metadata did not include an asset index".to_string()))?;
    let client_jar = match mode {
        LaunchResolutionMode::Preview => planned_download_path(&client_download, &version_root, None)?,
        LaunchResolutionMode::Prepare => ensure_download(&client, &client_download, &version_root, None)?,
    };
    let assets_root = match mode {
        LaunchResolutionMode::Preview => cache.assets_dir.clone(),
        LaunchResolutionMode::Prepare => ensure_assets(&client, &cache.assets_dir, &asset_index)?,
    };
    let logging_config = match mode {
        LaunchResolutionMode::Preview => planned_logging_config_path(&cache.assets_dir, version.logging.as_ref())?,
        LaunchResolutionMode::Prepare => ensure_logging_config(&client, &cache.assets_dir, version.logging.as_ref())?,
    };
    let libraries = match mode {
        LaunchResolutionMode::Preview => planned_libraries(&cache.libraries_dir, &version.libraries)?,
        LaunchResolutionMode::Prepare => ensure_libraries(&client, &cache.libraries_dir, &version.libraries)?,
    };
    let natives_dir = match mode {
        LaunchResolutionMode::Preview => planned_natives_dir(&cache.natives_dir, &version.libraries, &profile.id)?,
        LaunchResolutionMode::Prepare => extract_natives(&cache.natives_dir, &version.libraries)?,
    };
    let java_executable = match mode {
        LaunchResolutionMode::Preview => {
            preview_java_executable(state, profile, version.java_version.as_ref())?
        }
        LaunchResolutionMode::Prepare => {
            resolve_java_executable(state, profile, &client, &cache, version.java_version.as_ref())?
        }
    };
    let launcher_name = load_setting(state, "launcher_name").unwrap_or_else(|_| "Blocksmith".to_string());
    let launcher_version = load_setting(state, "launcher_version").unwrap_or_else(|_| "0.1.0".to_string());

    let classpath = build_classpath(&libraries, &client_jar);
    let memory_args = build_memory_args(profile);
    let user_jvm_args = split_args(&profile.jvm_args);
    let user_game_args = split_args(&profile.launch_args);

    let placeholder_values = placeholder_map(
        &version,
        profile,
        &auth_session,
        &minecraft_dir,
        &cache.libraries_dir,
        &assets_root,
        &asset_index.id,
        &classpath,
        natives_dir.as_ref(),
        &launcher_name,
        &launcher_version,
        &client_jar,
    );

    let mut jvm_args = render_arguments(&version.arguments.jvm, &placeholder_values)?;
    if let Some(logging) = version.logging.as_ref() {
        if let Some(argument) = &logging.argument {
            if let Some(path) = &logging_config {
                let mut replacements = placeholder_values.clone();
                replacements.insert("path".to_string(), path.to_string_lossy().into_owned());
                jvm_args.push(apply_placeholders(argument, &replacements));
            }
        }
    }
    jvm_args.extend(default_native_jvm_args(natives_dir.as_ref()));
    jvm_args.extend(user_jvm_args);

    let mut game_args = if !version.arguments.game.is_empty() {
        render_arguments(&version.arguments.game, &placeholder_values)?
    } else {
        split_args(
            &version
                .minecraft_arguments
                .clone()
                .unwrap_or_default()
                .replace("${classpath_separator}", ";"),
        )
        .into_iter()
        .map(|arg| apply_placeholders(&arg, &placeholder_values))
        .collect()
    };
    game_args.extend(user_game_args);

    let mut command = Vec::new();
    command.extend(memory_args.clone());
    command.extend(jvm_args.clone());
    command.push("-cp".to_string());
    command.push(
        classpath
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(";"),
    );
    command.push(version.main_class.clone());
    command.extend(game_args.clone());

    let mut preview_values = placeholder_values.clone();
    preview_values.insert("auth_access_token".to_string(), "<redacted>".to_string());
    let redacted_game_args = if !version.arguments.game.is_empty() {
        render_arguments(&version.arguments.game, &preview_values)?
    } else {
        split_args(
            &version
                .minecraft_arguments
                .clone()
                .unwrap_or_default()
                .replace("${classpath_separator}", ";"),
        )
        .into_iter()
        .map(|arg| apply_placeholders(&arg, &preview_values))
        .collect()
    };

    let plan = LaunchPlan {
        profile_id: profile.id.clone(),
        java_executable: java_executable.to_string_lossy().into_owned(),
        working_directory: minecraft_dir.to_string_lossy().into_owned(),
        game_version: profile.minecraft_version.clone(),
        loader_version: profile.loader_version.clone(),
        memory_args,
        jvm_args,
        game_args: redacted_game_args,
        libraries: libraries
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect(),
        assets_root: assets_root.to_string_lossy().into_owned(),
        natives_dir: natives_dir
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned()),
        account_id: auth_session.account_id.clone(),
        main_class: version.main_class,
        classpath: classpath
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect(),
        client_jar: client_jar.to_string_lossy().into_owned(),
        logging_config: logging_config.map(|path| path.to_string_lossy().into_owned()),
        username: auth_session.username,
        user_type: auth_session.user_type,
        online: auth_session.online,
        command_preview: {
            let mut preview = Vec::new();
            preview.push(java_executable.to_string_lossy().into_owned());
            preview.extend(command.iter().map(|value| {
                value.replace(&placeholder_values["auth_access_token"], "<redacted>")
            }));
            preview
        },
    };

    let mut full_command = Vec::new();
    full_command.push(java_executable.to_string_lossy().into_owned());
    full_command.extend(command);

    Ok(PreparedLaunch {
        plan,
        command: full_command,
    })
}

pub fn launch_process(
    prepared: &PreparedLaunch,
    log_path: &Path,
    working_directory: &Path,
) -> AppResult<std::process::Child> {
    let executable = prepared
        .command
        .first()
        .ok_or_else(|| AppError::Internal("launch command was empty".to_string()))?;
    let args = &prepared.command[1..];

    let stdout = File::options()
        .create(true)
        .append(true)
        .open(log_path)?;
    let stderr = stdout.try_clone()?;

    let child = Command::new(executable)
        .args(args)
        .current_dir(working_directory)
        .stdout(stdout)
        .stderr(stderr)
        .spawn()?;

    Ok(child)
}

#[derive(Clone)]
struct CacheLayout {
    versions_dir: PathBuf,
    libraries_dir: PathBuf,
    assets_dir: PathBuf,
    natives_dir: PathBuf,
    runtime_manifests_dir: PathBuf,
}

fn ensure_cache_layout(paths: &AppPaths) -> AppResult<CacheLayout> {
    let root = paths.cache_dir.join("minecraft");
    let layout = CacheLayout {
        versions_dir: root.join("versions"),
        libraries_dir: root.join("libraries"),
        assets_dir: root.join("assets"),
        natives_dir: paths.temp_dir.join("natives"),
        runtime_manifests_dir: root.join("runtime-manifests"),
    };

    for directory in [
        &layout.versions_dir,
        &layout.libraries_dir,
        &layout.assets_dir,
        &layout.assets_dir.join("indexes"),
        &layout.assets_dir.join("objects"),
        &layout.assets_dir.join("log_configs"),
        &layout.natives_dir,
        &layout.runtime_manifests_dir,
    ] {
        fs::create_dir_all(directory)?;
    }

    Ok(layout)
}

fn http_client() -> AppResult<Client> {
    Client::builder()
        .user_agent("Blocksmith/0.1.0")
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(Into::into)
}

fn fetch_version_manifest(client: &Client) -> AppResult<VersionManifest> {
    client
        .get(VERSION_MANIFEST_URL)
        .send()?
        .error_for_status()?
        .json()
        .map_err(Into::into)
}

fn resolve_version_metadata(
    client: &Client,
    manifest: &VersionManifest,
    profile: &ProfileSummary,
) -> AppResult<ResolvedVersionMetadata> {
    let raw = match profile.profile_type {
        ProfileType::Vanilla => fetch_complete_version_by_id(client, manifest, &profile.minecraft_version)?,
        ProfileType::Fabric => {
            let loader_version = resolve_fabric_loader_version(client, profile)?;
            let profile_url = FABRIC_PROFILE_URL
                .replace("{minecraft}", &profile.minecraft_version)
                .replace("{loader}", &loader_version);
            let fabric: RawVersionMetadata = client
                .get(profile_url)
                .send()?
                .error_for_status()?
                .json()?;
            merge_inherited_version(client, manifest, fabric)?
        }
    };

    Ok(ResolvedVersionMetadata {
        id: raw.id,
        arguments: raw.arguments.unwrap_or_default(),
        minecraft_arguments: raw.minecraft_arguments,
        main_class: raw
            .main_class
            .ok_or_else(|| AppError::Validation("version metadata did not include a main class".to_string()))?,
        downloads: raw.downloads.unwrap_or_default(),
        asset_index: raw.asset_index,
        logging: raw.logging.and_then(|entry| entry.client),
        libraries: dedupe_libraries(raw.libraries),
        version_type: raw.kind.unwrap_or_else(|| "release".to_string()),
        java_version: raw.java_version,
    })
}

fn resolve_fabric_loader_version(client: &Client, profile: &ProfileSummary) -> AppResult<String> {
    let requested = profile
        .loader_version
        .as_deref()
        .unwrap_or("latest")
        .trim()
        .to_string();
    if requested != "latest" && !requested.is_empty() {
        return Ok(requested);
    }

    let response = client
        .get(format!("{FABRIC_LOADERS_URL}/{}", profile.minecraft_version))
        .send()?
        .error_for_status()?;
    let loaders: Vec<FabricLoaderSummary> = response
        .json::<Vec<FabricLoaderVersionEnvelope>>()?
        .into_iter()
        .map(|entry| FabricLoaderSummary {
            version: entry.loader.version,
            stable: entry.loader.stable,
        })
        .collect();
    loaders
        .iter()
        .find(|entry| entry.stable)
        .or_else(|| loaders.first())
        .map(|entry| entry.version.clone())
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "no Fabric loaders were found for Minecraft {}",
                profile.minecraft_version
            ))
        })
}

fn fetch_complete_version_by_id(
    client: &Client,
    manifest: &VersionManifest,
    version_id: &str,
) -> AppResult<RawVersionMetadata> {
    let entry = manifest
        .versions
        .iter()
        .find(|entry| entry.id == version_id)
        .ok_or_else(|| AppError::NotFound(format!("minecraft version not found: {version_id}")))?;
    let raw: RawVersionMetadata = client
        .get(&entry.url)
        .send()?
        .error_for_status()?
        .json()?;
    merge_inherited_version(client, manifest, raw)
}

fn merge_inherited_version(
    client: &Client,
    manifest: &VersionManifest,
    raw: RawVersionMetadata,
) -> AppResult<RawVersionMetadata> {
    let Some(parent_id) = raw.inherits_from.clone() else {
        return Ok(raw);
    };
    let parent = fetch_complete_version_by_id(client, manifest, &parent_id)?;
    Ok(parent.merge(raw))
}

fn dedupe_libraries(libraries: Vec<Library>) -> Vec<Library> {
    let mut result = Vec::new();
    let mut used = HashMap::new();
    for library in libraries.into_iter().rev() {
        if used.insert(library.name.clone(), true).is_some() {
            continue;
        }
        result.push(library);
    }
    result.reverse();
    result
}

fn ensure_assets(client: &Client, assets_dir: &Path, asset_index: &AssetIndexDownload) -> AppResult<PathBuf> {
    let indexes_dir = assets_dir.join("indexes");
    let objects_dir = assets_dir.join("objects");
    fs::create_dir_all(&indexes_dir)?;
    fs::create_dir_all(&objects_dir)?;

    let index_filename = if asset_index.id.ends_with(".json") {
        asset_index.id.clone()
    } else {
        format!("{}.json", asset_index.id)
    };
    let index_download = DownloadDescriptor {
        url: asset_index.url.clone(),
        sha1: asset_index.sha1.clone(),
        path: None,
        id: Some(asset_index.id.clone()),
    };
    let index_path = ensure_download(client, &index_download, &indexes_dir, Some(index_filename))?;
    let file = File::open(&index_path)?;
    let payload: AssetIndexPayload = serde_json::from_reader(file)?;
    let mut pending = Vec::new();
    for object in payload.objects.into_values() {
        let prefix = object
            .hash
            .get(..2)
            .ok_or_else(|| AppError::Validation("asset hash was unexpectedly short".to_string()))?;
        let object_dir = objects_dir.join(prefix);
        fs::create_dir_all(&object_dir)?;
        let object_path = object_dir.join(&object.hash);
        if object_path.exists() && sha1_matches_path(&object_path, Some(&object.hash))? {
            continue;
        }

        let url = format!("https://resources.download.minecraft.net/{prefix}/{}", object.hash);
        let download = DownloadDescriptor {
            url,
            sha1: Some(object.hash),
            path: None,
            id: None,
        };
        pending.push((download, object_dir));
    }

    tracing::info!("minecraft assets pending download: {}", pending.len());
    pending
        .into_par_iter()
        .try_for_each(|(download, object_dir)| ensure_download(client, &download, &object_dir, None).map(|_| ()))?;

    Ok(assets_dir.to_path_buf())
}

fn ensure_logging_config(
    client: &Client,
    assets_dir: &Path,
    logging: Option<&LoggingClientConfig>,
) -> AppResult<Option<PathBuf>> {
    let Some(logging) = logging else {
        return Ok(None);
    };

    let log_configs_dir = assets_dir.join("log_configs");
    fs::create_dir_all(&log_configs_dir)?;
    let file_name = logging
        .file
        .id
        .as_deref()
        .unwrap_or("client-log4j.xml")
        .split('/')
        .next_back()
        .unwrap_or("client-log4j.xml")
        .to_string();
    let path = ensure_download(client, &logging.file, &log_configs_dir, Some(file_name))?;
    Ok(Some(path))
}

fn ensure_libraries(client: &Client, libraries_dir: &Path, libraries: &[Library]) -> AppResult<Vec<PathBuf>> {
    let downloads = libraries
        .iter()
        .filter(|library| library_is_allowed(library.rules.as_deref()))
        .map(|library| {
            Ok(library.artifact_download()?.map(|artifact| {
                let override_name = artifact.path.clone();
                (artifact, override_name)
            }))
        })
        .collect::<AppResult<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();

    tracing::info!("minecraft libraries pending download: {}", downloads.len());
    downloads
        .into_par_iter()
        .map(|(artifact, override_name)| ensure_download(client, &artifact, libraries_dir, override_name))
        .collect()
}

fn extract_natives(temp_dir: &Path, libraries: &[Library]) -> AppResult<Option<PathBuf>> {
    let mut natives = Vec::new();
    for library in libraries.iter().filter(|library| library_is_allowed(library.rules.as_deref())) {
        if let Some(download) = library.native_download()? {
            natives.push((library, download));
        }
    }

    if natives.is_empty() {
        return Ok(None);
    }

    let launch_native_dir = temp_dir.join(format!("natives-{}", Uuid::new_v4().simple()));
    fs::create_dir_all(&launch_native_dir)?;

    for (library, download) in natives {
        let jar_dir = temp_dir.join("native-jars");
        fs::create_dir_all(&jar_dir)?;
        let file_name = download
            .path
            .clone()
            .or_else(|| library.name.rsplit(':').next().map(|value| format!("{value}.jar")))
            .unwrap_or_else(|| format!("{}.jar", Uuid::new_v4().simple()));
        let jar_path = ensure_download(&http_client()?, &download, &jar_dir, Some(file_name))?;
        extract_native_archive(&jar_path, &launch_native_dir, library.extract.as_ref())?;
    }

    Ok(Some(launch_native_dir))
}

fn extract_native_archive(
    archive_path: &Path,
    destination: &Path,
    extract_rules: Option<&LibraryExtractRules>,
) -> AppResult<()> {
    let file = File::open(archive_path)?;
    let mut archive = ZipArchive::new(file)?;

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let entry_name = entry.name().replace('\\', "/");
        if entry.is_dir() {
            continue;
        }

        if extract_rules
            .and_then(|rules| rules.exclude.as_ref())
            .is_some_and(|patterns| patterns.iter().any(|pattern| entry_name.starts_with(pattern)))
        {
            continue;
        }

        let Some(output_name) = native_archive_output_name(&entry_name) else {
            continue;
        };

        let output_path = destination.join(output_name);
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut output = File::create(output_path)?;
        io::copy(&mut entry, &mut output)?;
    }

    Ok(())
}

fn native_archive_output_name(entry_name: &str) -> Option<String> {
    let normalized = entry_name.replace('\\', "/");
    if normalized.starts_with("META-INF/") {
        return None;
    }

    let file_name = Path::new(&normalized)
        .file_name()?
        .to_string_lossy()
        .into_owned();
    let lower = file_name.to_ascii_lowercase();
    if lower.is_empty() || lower == ".ds_store" || lower.ends_with(".sha1") || lower.ends_with(".git") {
        return None;
    }

    if !(lower.ends_with(".dll")
        || lower.ends_with(".so")
        || lower.ends_with(".dylib")
        || lower.ends_with(".jnilib"))
    {
        return None;
    }

    Some(file_name)
}

fn ensure_download(
    client: &Client,
    download: &DownloadDescriptor,
    root: &Path,
    override_name: Option<String>,
) -> AppResult<PathBuf> {
    fs::create_dir_all(root)?;
    let target_path = planned_download_path(download, root, override_name)?;

    if target_path.exists() && sha1_matches_path(&target_path, download.sha1.as_deref())? {
        return Ok(target_path);
    }

    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut last_error: Option<AppError> = None;
    for attempt in 1..=DOWNLOAD_ATTEMPTS {
        let temp_path = temporary_download_path(&target_path);
        let result = (|| -> AppResult<()> {
            let mut response = client.get(&download.url).send()?.error_for_status()?;
            let mut output = File::create(&temp_path)?;
            io::copy(&mut response, &mut output)?;
            output.flush()?;

            if !sha1_matches_path(&temp_path, download.sha1.as_deref())? {
                return Err(AppError::Validation(format!(
                    "downloaded file failed checksum validation: {}",
                    target_path.to_string_lossy()
                )));
            }

            if target_path.exists() {
                fs::remove_file(&target_path)?;
            }
            fs::rename(&temp_path, &target_path)?;
            Ok(())
        })();

        match result {
            Ok(()) => return Ok(target_path),
            Err(error) => {
                let _ = fs::remove_file(&temp_path);
                let should_retry = attempt < DOWNLOAD_ATTEMPTS;
                tracing::warn!(
                    "download attempt {attempt}/{DOWNLOAD_ATTEMPTS} failed for {}: {}",
                    download.url,
                    error
                );
                last_error = Some(error);
                if !should_retry {
                    break;
                }
            }
        }
    }

    Err(last_error.unwrap_or_else(|| {
        AppError::Internal(format!("download failed without a captured error: {}", download.url))
    }))
}

fn temporary_download_path(target_path: &Path) -> PathBuf {
    target_path.with_extension(format!(
        "{}{}.download",
        target_path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| format!("{extension}."))
            .unwrap_or_default(),
        Uuid::new_v4().simple()
    ))
}

fn planned_download_path(
    download: &DownloadDescriptor,
    root: &Path,
    override_name: Option<String>,
) -> AppResult<PathBuf> {
    let relative = override_name
        .or_else(|| download.path.clone())
        .or_else(|| {
            download
                .id
                .as_deref()
                .and_then(|id| id.split('/').next_back())
                .map(ToString::to_string)
        })
        .or_else(|| download.url.split('/').next_back().map(ToString::to_string))
        .ok_or_else(|| {
            AppError::Validation(format!("download {} did not include a target path", download.url))
        })?;

    Ok(relative
        .split('/')
        .fold(root.to_path_buf(), |path, part| path.join(part)))
}

fn sha1_matches_path(path: &Path, expected: Option<&str>) -> AppResult<bool> {
    let Some(expected) = expected else {
        return Ok(path.exists());
    };

    let mut file = File::open(path)?;
    let mut hasher = Sha1::new();
    let mut buffer = [0u8; 8192];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    Ok(format!("{:x}", hasher.finalize()) == expected)
}

fn build_classpath(libraries: &[PathBuf], client_jar: &Path) -> Vec<PathBuf> {
    let mut classpath = libraries.to_vec();
    classpath.push(client_jar.to_path_buf());
    classpath
}

fn build_memory_args(profile: &ProfileSummary) -> Vec<String> {
    let min = profile.memory_min_mb.unwrap_or(512);
    let max = profile.memory_max_mb.unwrap_or(2048).max(min);
    vec![format!("-Xms{min}M"), format!("-Xmx{max}M")]
}

fn default_native_jvm_args(natives_dir: Option<&PathBuf>) -> Vec<String> {
    let Some(natives_dir) = natives_dir else {
        return Vec::new();
    };

    let natives = natives_dir.to_string_lossy().into_owned();
    vec![
        format!("-Djava.library.path={natives}"),
        format!("-Djna.tmpdir={natives}"),
        format!("-Dorg.lwjgl.system.SharedLibraryExtractPath={natives}"),
        format!("-Dio.netty.native.workdir={natives}"),
    ]
}

fn placeholder_map(
    version: &ResolvedVersionMetadata,
    profile: &ProfileSummary,
    auth_session: &LaunchAuthSession,
    minecraft_dir: &Path,
    libraries_dir: &Path,
    assets_root: &Path,
    asset_index_id: &str,
    classpath: &[PathBuf],
    natives_dir: Option<&PathBuf>,
    launcher_name: &str,
    launcher_version: &str,
    client_jar: &Path,
) -> HashMap<String, String> {
    let mut map = HashMap::new();
    map.insert("auth_player_name".to_string(), auth_session.username.clone());
    map.insert("version_name".to_string(), version.id.clone());
    map.insert(
        "game_directory".to_string(),
        minecraft_dir.to_string_lossy().into_owned(),
    );
    map.insert(
        "assets_root".to_string(),
        assets_root.to_string_lossy().into_owned(),
    );
    map.insert("assets_index_name".to_string(), asset_index_id.to_string());
    map.insert("auth_uuid".to_string(), auth_session.uuid.clone());
    map.insert(
        "auth_access_token".to_string(),
        auth_session.access_token.clone(),
    );
    map.insert("clientid".to_string(), auth_session.account_id.clone().unwrap_or_default());
    map.insert("auth_xuid".to_string(), auth_session.xuid.clone().unwrap_or_default());
    map.insert("user_type".to_string(), auth_session.user_type.clone());
    map.insert("version_type".to_string(), version.version_type.clone());
    map.insert("user_properties".to_string(), "{}".to_string());
    map.insert(
        "natives_directory".to_string(),
        natives_dir
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default(),
    );
    map.insert(
        "launcher_name".to_string(),
        launcher_name.to_string(),
    );
    map.insert(
        "launcher_version".to_string(),
        launcher_version.to_string(),
    );
    map.insert(
        "classpath".to_string(),
        classpath
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(";"),
    );
    map.insert("classpath_separator".to_string(), ";".to_string());
    map.insert(
        "library_directory".to_string(),
        libraries_dir.to_string_lossy().into_owned(),
    );
    map.insert(
        "primary_jar".to_string(),
        client_jar.to_string_lossy().into_owned(),
    );
    map.insert("game_assets".to_string(), assets_root.to_string_lossy().into_owned());
    map.insert("resolution_width".to_string(), "1280".to_string());
    map.insert("resolution_height".to_string(), "720".to_string());
    map.insert("profile_name".to_string(), profile.name.clone());
    map
}

fn render_arguments(arguments: &[LaunchArgument], replacements: &HashMap<String, String>) -> AppResult<Vec<String>> {
    let mut rendered = Vec::new();
    for argument in arguments {
        if !argument.is_allowed() {
            continue;
        }

        for value in argument.values() {
            rendered.push(apply_placeholders(value, replacements));
        }
    }
    Ok(rendered)
}

fn apply_placeholders(value: &str, replacements: &HashMap<String, String>) -> String {
    let mut rendered = value.to_string();
    for (key, replacement) in replacements {
        rendered = rendered.replace(&format!("${{{key}}}"), replacement);
    }
    rendered
}

fn split_args(value: &str) -> Vec<String> {
    value
        .split_whitespace()
        .map(ToString::to_string)
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JavaCompatibility {
    Compatible,
    UnknownVersion,
    TooOld { detected: u32, required: u32 },
}

fn resolve_java_executable(
    state: &AppState,
    profile: &ProfileSummary,
    client: &Client,
    cache: &CacheLayout,
    java_version: Option<&JavaVersionDescriptor>,
) -> AppResult<PathBuf> {
    let required_java_major = java_version.map(|descriptor| descriptor.major_version);

    if let Some(path) = configured_java_path(profile.java_path.as_deref()) {
        return match java_compatibility(&path, required_java_major) {
            JavaCompatibility::Compatible | JavaCompatibility::UnknownVersion => Ok(path),
            JavaCompatibility::TooOld { detected, required } => Err(incompatible_java_error(
                "profile Java path",
                profile,
                detected,
                required,
                true,
            )),
        };
    }

    let default_java_path = load_setting(state, "default_java_path")
        .ok()
        .and_then(|value| configured_java_path(Some(value.as_str())));
    let managed_preference = load_setting(state, "managed_runtime_preference")
        .unwrap_or_else(|_| "auto".to_string())
        .to_ascii_lowercase();
    let allow_managed = !matches!(managed_preference.as_str(), "never" | "off" | "system");
    let require_managed = matches!(managed_preference.as_str(), "managed" | "required");
    let mut last_incompatible_source: Option<(&'static str, u32, u32)> = None;

    if let Some(path) = default_java_path {
        match java_compatibility(&path, required_java_major) {
            JavaCompatibility::Compatible | JavaCompatibility::UnknownVersion => return Ok(path),
            JavaCompatibility::TooOld { detected, required } => {
                tracing::warn!(
                    "ignoring default Java path {:?}: detected Java {}, but profile {} requires Java {} or newer",
                    path,
                    detected,
                    profile.minecraft_version,
                    required
                );
                last_incompatible_source = Some(("default Java path", detected, required));
            }
        }
    }

    if allow_managed {
        if let Some(java_version) = java_version {
            match ensure_managed_runtime(state, client, cache, java_version) {
                Ok(path) => return Ok(path),
                Err(error) if require_managed => return Err(error),
                Err(error) => {
                    tracing::warn!(
                        "failed to prepare managed runtime for Minecraft {}: {}",
                        profile.minecraft_version,
                        error
                    );
                }
            }
        }
    }

    if let Some(path) = find_java_on_path() {
        match java_compatibility(&path, required_java_major) {
            JavaCompatibility::Compatible | JavaCompatibility::UnknownVersion => return Ok(path),
            JavaCompatibility::TooOld { detected, required } => {
                tracing::warn!(
                    "ignoring Java on PATH {:?}: detected Java {}, but profile {} requires Java {} or newer",
                    path,
                    detected,
                    profile.minecraft_version,
                    required
                );
                last_incompatible_source = Some(("Java on PATH", detected, required));
            }
        }
    }

    if let Some((source, detected, required)) = last_incompatible_source {
        return Err(incompatible_java_error(
            source,
            profile,
            detected,
            required,
            false,
        ));
    }

    Err(AppError::Validation(
        if require_managed {
            "managed Java runtime could not be installed for this version, and no override path was configured."
                .to_string()
        } else {
            "no Java executable was found. Set profile.java_path or settings.default_java_path, or leave managed_runtime_preference on auto."
                .to_string()
        },
    ))
}

fn preview_java_executable(
    state: &AppState,
    profile: &ProfileSummary,
    java_version: Option<&JavaVersionDescriptor>,
) -> AppResult<PathBuf> {
    let required_java_major = java_version.map(|descriptor| descriptor.major_version);

    if let Some(path) = configured_java_path(profile.java_path.as_deref()) {
        return match java_compatibility(&path, required_java_major) {
            JavaCompatibility::Compatible | JavaCompatibility::UnknownVersion => Ok(path),
            JavaCompatibility::TooOld { detected, required } => Err(incompatible_java_error(
                "profile Java path",
                profile,
                detected,
                required,
                true,
            )),
        };
    }

    let default_java_path = load_setting(state, "default_java_path")
        .ok()
        .and_then(|value| configured_java_path(Some(value.as_str())));
    let managed_preference = load_setting(state, "managed_runtime_preference")
        .unwrap_or_else(|_| "auto".to_string())
        .to_ascii_lowercase();
    let allow_managed = !matches!(managed_preference.as_str(), "never" | "off" | "system");
    let require_managed = matches!(managed_preference.as_str(), "managed" | "required");
    let mut last_incompatible_source: Option<(&'static str, u32, u32)> = None;

    if let Some(path) = default_java_path {
        match java_compatibility(&path, required_java_major) {
            JavaCompatibility::Compatible | JavaCompatibility::UnknownVersion => return Ok(path),
            JavaCompatibility::TooOld { detected, required } => {
                tracing::warn!(
                    "ignoring default Java path {:?}: detected Java {}, but profile {} requires Java {} or newer",
                    path,
                    detected,
                    profile.minecraft_version,
                    required
                );
                last_incompatible_source = Some(("default Java path", detected, required));
            }
        }
    }

    if allow_managed {
        if let Some(java_version) = java_version {
            if let Some(installed) = installed_managed_runtime_executable(&state.paths, java_version) {
                return Ok(installed);
            }
            return Ok(planned_managed_runtime_executable(&state.paths, java_version));
        }
    }

    if let Some(path) = find_java_on_path() {
        match java_compatibility(&path, required_java_major) {
            JavaCompatibility::Compatible | JavaCompatibility::UnknownVersion => return Ok(path),
            JavaCompatibility::TooOld { detected, required } => {
                tracing::warn!(
                    "ignoring Java on PATH {:?}: detected Java {}, but profile {} requires Java {} or newer",
                    path,
                    detected,
                    profile.minecraft_version,
                    required
                );
                last_incompatible_source = Some(("Java on PATH", detected, required));
            }
        }
    }

    if let Some((source, detected, required)) = last_incompatible_source {
        return Err(incompatible_java_error(
            source,
            profile,
            detected,
            required,
            false,
        ));
    }

    Err(AppError::Validation(
        if require_managed {
            "managed Java runtime could not be resolved for preview, and no override path was configured."
                .to_string()
        } else {
            "no Java executable was found. Set profile.java_path or settings.default_java_path, or leave managed_runtime_preference on auto."
                .to_string()
        },
    ))
}

fn ensure_managed_runtime(
    state: &AppState,
    client: &Client,
    cache: &CacheLayout,
    java_version: &JavaVersionDescriptor,
) -> AppResult<PathBuf> {
    let runtime_catalog: RuntimeAllManifest = client
        .get(JAVA_RUNTIME_ALL_URL)
        .send()?
        .error_for_status()?
        .json()?;
    let component = runtime_catalog
        .windows_x64
        .get(&java_version.component)
        .and_then(|components| components.first())
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "Mojang runtime component '{}' was not found for windows-x64",
                java_version.component
            ))
        })?;

    let runtime_root = managed_runtime_root(&state.paths, java_version);
    let runtime_version_marker = runtime_root.join(".blocksmith-runtime-version");
    let (javaw_path, java_path) = managed_runtime_executable_candidates(&runtime_root);
    let expected_version = format!("{}|{}", component.version.name, java_version.major_version);

    if javaw_path.exists()
        && runtime_version_marker.exists()
        && fs::read_to_string(&runtime_version_marker)
            .ok()
            .is_some_and(|value| value.trim() == expected_version)
    {
        return Ok(javaw_path);
    }
    if java_path.exists()
        && runtime_version_marker.exists()
        && fs::read_to_string(&runtime_version_marker)
            .ok()
            .is_some_and(|value| value.trim() == expected_version)
    {
        return Ok(java_path);
    }

    if runtime_root.exists() {
        fs::remove_dir_all(&runtime_root)?;
    }
    fs::create_dir_all(&runtime_root)?;

    let runtime_manifest_path = ensure_download(
        client,
        &component.manifest,
        &cache.runtime_manifests_dir,
        Some(format!("{}-windows-x64-manifest.json", java_version.component)),
    )?;
    let runtime_manifest: RuntimeManifest = serde_json::from_reader(File::open(&runtime_manifest_path)?)?;
    let mut pending_runtime_files = Vec::new();

    for (relative_path, entry) in runtime_manifest.files {
        let normalized = relative_path.replace('\\', "/");
        let target_path = normalized
            .split('/')
            .fold(runtime_root.clone(), |path, part| path.join(part));

        match entry.kind.as_str() {
            "directory" => {
                fs::create_dir_all(&target_path)?;
            }
            "file" => {
                let raw = entry
                    .downloads
                    .and_then(|downloads| downloads.raw)
                    .ok_or_else(|| {
                        AppError::Validation(format!(
                            "runtime manifest file {} did not expose a raw download",
                            normalized
                        ))
                    })?;
                pending_runtime_files.push((raw, normalized));
            }
            "link" => {}
            other => {
                return Err(AppError::Validation(format!(
                    "unsupported runtime manifest entry type: {other}"
                )))
            }
        }
    }

    tracing::info!(
        "minecraft runtime files pending download: {}",
        pending_runtime_files.len()
    );
    pending_runtime_files.into_par_iter().try_for_each(|(raw, normalized)| {
        ensure_download(client, &raw, &runtime_root, Some(normalized)).map(|_| ())
    })?;

    fs::write(&runtime_version_marker, expected_version)?;

    if javaw_path.exists() {
        Ok(javaw_path)
    } else if java_path.exists() {
        Ok(java_path)
    } else {
        Err(AppError::NotFound(format!(
            "managed runtime '{}' installed without javaw.exe or java.exe",
            java_version.component
        )))
    }
}

fn managed_runtime_root(paths: &AppPaths, java_version: &JavaVersionDescriptor) -> PathBuf {
    paths
        .runtimes_dir
        .join("minecraft")
        .join(&java_version.component)
        .join("windows-x64")
        .join(&java_version.component)
}

fn managed_runtime_executable_candidates(runtime_root: &Path) -> (PathBuf, PathBuf) {
    (
        runtime_root.join("bin").join("javaw.exe"),
        runtime_root.join("bin").join("java.exe"),
    )
}

fn installed_managed_runtime_executable(
    paths: &AppPaths,
    java_version: &JavaVersionDescriptor,
) -> Option<PathBuf> {
    let runtime_root = managed_runtime_root(paths, java_version);
    let (javaw_path, java_path) = managed_runtime_executable_candidates(&runtime_root);
    if javaw_path.exists() {
        Some(javaw_path)
    } else if java_path.exists() {
        Some(java_path)
    } else {
        None
    }
}

fn planned_managed_runtime_executable(paths: &AppPaths, java_version: &JavaVersionDescriptor) -> PathBuf {
    let runtime_root = managed_runtime_root(paths, java_version);
    let (javaw_path, _) = managed_runtime_executable_candidates(&runtime_root);
    javaw_path
}

fn find_java_on_path() -> Option<PathBuf> {
    for executable in ["javaw", "java"] {
        if let Ok(output) = Command::new("where").arg(executable).output() {
            if output.status.success() {
                let first = String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .find(|line| !line.trim().is_empty())
                    .map(str::trim)
                    .map(PathBuf::from);
                if first.is_some() {
                    return first;
                }
            }
        }
    }

    None
}

fn configured_java_path(value: Option<&str>) -> Option<PathBuf> {
    let trimmed = value?.trim();
    if trimmed.is_empty() {
        return None;
    }

    let path = PathBuf::from(trimmed);
    if path.is_file() {
        Some(path)
    } else {
        None
    }
}

fn java_compatibility(path: &Path, required_major: Option<u32>) -> JavaCompatibility {
    let Some(required) = required_major else {
        return JavaCompatibility::Compatible;
    };

    match probe_java_major_version(path) {
        Some(detected) if detected < required => JavaCompatibility::TooOld { detected, required },
        Some(_) => JavaCompatibility::Compatible,
        None => JavaCompatibility::UnknownVersion,
    }
}

fn incompatible_java_error(
    source: &str,
    profile: &ProfileSummary,
    detected: u32,
    required: u32,
    is_profile_override: bool,
) -> AppError {
    let guidance = if is_profile_override {
        "Update that profile override or clear it to let Blocksmith use its managed runtime."
    } else {
        "Update that runtime, clear the override, or let Blocksmith use its managed runtime."
    };

    AppError::Validation(format!(
        "{source} is Java {detected}, but Minecraft {} requires Java {required} or newer. {guidance}",
        profile.minecraft_version
    ))
}

fn probe_java_major_version(java_path: &Path) -> Option<u32> {
    let probe_path = if java_path
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.eq_ignore_ascii_case("javaw.exe"))
        .unwrap_or(false)
    {
        let sibling = java_path.with_file_name("java.exe");
        if sibling.is_file() {
            sibling
        } else {
            java_path.to_path_buf()
        }
    } else {
        java_path.to_path_buf()
    };

    let output = Command::new(probe_path).arg("-version").output().ok()?;
    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    parse_java_major_version(&combined)
}

fn parse_java_major_version(output: &str) -> Option<u32> {
    for line in output.lines() {
        if let Some(start) = line.find('"') {
            let remainder = &line[start + 1..];
            if let Some(end) = remainder.find('"') {
                if let Some(version) = parse_java_version_token(&remainder[..end]) {
                    return Some(version);
                }
            }
        }

        for token in line.split_whitespace() {
            if let Some(version) = parse_java_version_token(token.trim_matches('"')) {
                return Some(version);
            }
        }
    }

    None
}

fn parse_java_version_token(token: &str) -> Option<u32> {
    let start = token.find(|character: char| character.is_ascii_digit())?;
    let numeric = token[start..]
        .split(|character: char| {
            !character.is_ascii_digit() && character != '.' && character != '_'
        })
        .next()?;

    if numeric.is_empty() {
        return None;
    }

    let mut parts = numeric.split(['.', '_']);
    let first = parts.next()?.parse::<u32>().ok()?;
    if first == 1 {
        parts.next()?.parse::<u32>().ok()
    } else {
        Some(first)
    }
}

fn load_setting(state: &AppState, key: &str) -> AppResult<String> {
    let connection = state.db()?;
    connection
        .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| row.get(0))
        .map_err(Into::into)
}

fn resolve_launch_auth_preview(
    state: &AppState,
    account_id: Option<&str>,
    offline_fallback_name: &str,
) -> AppResult<LaunchAuthSession> {
    let Some(account_id) = account_id else {
        return Ok(offline_preview_session(None, offline_fallback_name, None, false));
    };

    let connection = state.db()?;
    let account = connection
        .query_row(
            "
            SELECT accounts.id, accounts.username, accounts.uuid, accounts.provider,
                   CASE WHEN account_tokens.account_id IS NULL THEN 0 ELSE 1 END
            FROM accounts
            LEFT JOIN account_tokens ON account_tokens.account_id = accounts.id
            WHERE accounts.id = ?1
            ",
            [account_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)? != 0,
                ))
            },
        )
        .optional()?;

    let Some((account_id, username, uuid, provider, has_token)) = account else {
        return Err(AppError::NotFound(format!("account not found: {account_id}")));
    };

    if provider.eq_ignore_ascii_case("microsoft") {
        Ok(offline_preview_session(
            Some(account_id),
            &username,
            Some(uuid),
            has_token,
        ))
    } else {
        Ok(offline_preview_session(
            Some(account_id),
            &username,
            Some(uuid),
            false,
        ))
    }
}

fn offline_preview_session(
    account_id: Option<String>,
    username: &str,
    uuid: Option<String>,
    online: bool,
) -> LaunchAuthSession {
    LaunchAuthSession {
        account_id,
        username: username.trim().to_string(),
        uuid: uuid.unwrap_or_else(|| Uuid::new_v4().to_string()),
        access_token: "<preview>".to_string(),
        user_type: if online {
            "msa".to_string()
        } else {
            "legacy".to_string()
        },
        xuid: None,
        online,
    }
}

fn planned_logging_config_path(
    assets_dir: &Path,
    logging: Option<&LoggingClientConfig>,
) -> AppResult<Option<PathBuf>> {
    let Some(logging) = logging else {
        return Ok(None);
    };

    let file_name = logging
        .file
        .id
        .as_deref()
        .unwrap_or("client-log4j.xml")
        .split('/')
        .next_back()
        .unwrap_or("client-log4j.xml")
        .to_string();
    Ok(Some(planned_download_path(
        &logging.file,
        &assets_dir.join("log_configs"),
        Some(file_name),
    )?))
}

fn planned_libraries(libraries_dir: &Path, libraries: &[Library]) -> AppResult<Vec<PathBuf>> {
    let mut resolved = Vec::new();

    for library in libraries {
        if !library_is_allowed(library.rules.as_deref()) {
            continue;
        }

        if let Some(artifact) = library.artifact_download()? {
            resolved.push(planned_download_path(&artifact, libraries_dir, artifact.path.clone())?);
        }
    }

    Ok(resolved)
}

fn planned_natives_dir(
    temp_dir: &Path,
    libraries: &[Library],
    profile_id: &str,
) -> AppResult<Option<PathBuf>> {
    for library in libraries.iter().filter(|library| library_is_allowed(library.rules.as_deref())) {
        if library.native_download()?.is_some() {
            return Ok(Some(temp_dir.join(format!("preview-{profile_id}"))));
        }
    }

    Ok(None)
}

fn library_is_allowed(rules: Option<&[Rule]>) -> bool {
    rules.map(rules_match).unwrap_or(true)
}

fn rules_match(rules: &[Rule]) -> bool {
    if rules.is_empty() {
        return true;
    }

    let mut allowed = false;
    for rule in rules {
        if rule.applies() {
            allowed = rule.action == "allow";
        }
    }

    allowed
}

#[derive(Debug, Clone)]
struct ResolvedVersionMetadata {
    id: String,
    arguments: VersionArguments,
    minecraft_arguments: Option<String>,
    main_class: String,
    downloads: VersionDownloads,
    asset_index: Option<AssetIndexDownload>,
    logging: Option<LoggingClientConfig>,
    libraries: Vec<Library>,
    version_type: String,
    java_version: Option<JavaVersionDescriptor>,
}

#[derive(Debug, Deserialize)]
struct VersionManifest {
    versions: Vec<VersionManifestEntry>,
}

#[derive(Debug, Deserialize)]
struct VersionManifestEntry {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    url: String,
    #[serde(rename = "releaseTime")]
    release_time: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct RawVersionMetadata {
    id: String,
    #[serde(rename = "inheritsFrom")]
    inherits_from: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
    #[serde(rename = "mainClass")]
    main_class: Option<String>,
    #[serde(default)]
    libraries: Vec<Library>,
    #[serde(default)]
    arguments: Option<VersionArguments>,
    #[serde(rename = "minecraftArguments")]
    minecraft_arguments: Option<String>,
    downloads: Option<VersionDownloads>,
    #[serde(rename = "assetIndex")]
    asset_index: Option<AssetIndexDownload>,
    #[serde(rename = "javaVersion")]
    java_version: Option<JavaVersionDescriptor>,
    logging: Option<LoggingConfiguration>,
}

impl RawVersionMetadata {
    fn merge(mut self, child: RawVersionMetadata) -> Self {
        self.id = child.id;
        self.kind = child.kind.or(self.kind);
        self.main_class = child.main_class.or(self.main_class);
        self.minecraft_arguments = child.minecraft_arguments.or(self.minecraft_arguments);
        self.downloads = child.downloads.or(self.downloads);
        self.asset_index = child.asset_index.or(self.asset_index);
        self.java_version = child.java_version.or(self.java_version);
        self.logging = child.logging.or(self.logging);

        let mut merged_libraries = self.libraries;
        merged_libraries.extend(child.libraries);
        self.libraries = merged_libraries;

        self.arguments = match (self.arguments, child.arguments) {
            (Some(parent), Some(child)) => Some(parent.merge(child)),
            (None, Some(child)) => Some(child),
            (Some(parent), None) => Some(parent),
            (None, None) => None,
        };

        self
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
struct VersionArguments {
    #[serde(default)]
    game: Vec<LaunchArgument>,
    #[serde(default)]
    jvm: Vec<LaunchArgument>,
}

impl VersionArguments {
    fn merge(mut self, child: VersionArguments) -> Self {
        self.game.extend(child.game);
        self.jvm.extend(child.jvm);
        self
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum LaunchArgument {
    Plain(String),
    Ruled {
        rules: Vec<Rule>,
        value: ArgumentValue,
    },
}

impl LaunchArgument {
    fn is_allowed(&self) -> bool {
        match self {
            Self::Plain(_) => true,
            Self::Ruled { rules, .. } => rules_match(rules),
        }
    }

    fn values(&self) -> Vec<&str> {
        match self {
            Self::Plain(value) => vec![value.as_str()],
            Self::Ruled { value, .. } => value.values(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum ArgumentValue {
    Single(String),
    Multiple(Vec<String>),
}

impl ArgumentValue {
    fn values(&self) -> Vec<&str> {
        match self {
            Self::Single(value) => vec![value.as_str()],
            Self::Multiple(values) => values.iter().map(|value| value.as_str()).collect(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct Rule {
    action: String,
    os: Option<OsRule>,
    features: Option<BTreeMap<String, bool>>,
}

impl Rule {
    fn applies(&self) -> bool {
        let os_matches = self.os.as_ref().map(OsRule::matches).unwrap_or(true);
        let features_match = self
            .features
            .as_ref()
            .map(|features| features.iter().all(|(_, expected)| !expected))
            .unwrap_or(true);
        os_matches && features_match
    }
}

#[derive(Debug, Clone, Deserialize)]
struct OsRule {
    name: Option<String>,
    arch: Option<String>,
}

impl OsRule {
    fn matches(&self) -> bool {
        let name_matches = self
            .name
            .as_deref()
            .map(|name| name.eq_ignore_ascii_case("windows"))
            .unwrap_or(true);
        let arch_matches = self
            .arch
            .as_deref()
            .map(|arch| {
                if cfg!(target_pointer_width = "64") {
                    arch.contains("64") || arch.eq_ignore_ascii_case("amd64")
                } else {
                    arch.contains("32") || arch.eq_ignore_ascii_case("x86")
                }
            })
            .unwrap_or(true);
        name_matches && arch_matches
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
struct VersionDownloads {
    client: Option<DownloadDescriptor>,
}

#[derive(Debug, Clone, Deserialize)]
struct AssetIndexDownload {
    id: String,
    url: String,
    sha1: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JavaVersionDescriptor {
    component: String,
    major_version: u32,
}

#[derive(Debug, Clone, Deserialize)]
struct LoggingConfiguration {
    client: Option<LoggingClientConfig>,
}

#[derive(Debug, Clone, Deserialize)]
struct LoggingClientConfig {
    argument: Option<String>,
    file: DownloadDescriptor,
}

#[derive(Debug, Clone, Deserialize)]
struct Library {
    name: String,
    url: Option<String>,
    downloads: Option<LibraryDownloads>,
    rules: Option<Vec<Rule>>,
    natives: Option<HashMap<String, String>>,
    extract: Option<LibraryExtractRules>,
}

impl Library {
    fn artifact_download(&self) -> AppResult<Option<DownloadDescriptor>> {
        if self.is_native_library_entry() {
            return Ok(None);
        }

        if let Some(downloads) = &self.downloads {
            if let Some(artifact) = &downloads.artifact {
                return Ok(Some(artifact.clone()));
            }
        }

        if let Some(url) = &self.url {
            return Ok(Some(maven_download(
                url,
                &self.name,
                self.coordinate_classifier(),
            )?));
        }

        Ok(None)
    }

    fn native_download(&self) -> AppResult<Option<DownloadDescriptor>> {
        if let Some(classifier) = self.coordinate_classifier() {
            if classifier.starts_with("natives-") {
                if !native_classifier_matches(classifier) {
                    return Ok(None);
                }

                if let Some(downloads) = &self.downloads {
                    if let Some(artifact) = &downloads.artifact {
                        return Ok(Some(artifact.clone()));
                    }
                }

                if let Some(url) = &self.url {
                    return Ok(Some(maven_download(url, &self.name, Some(classifier))?));
                }

                return Ok(None);
            }
        }

        let Some(classifier_key) = self
            .natives
            .as_ref()
            .and_then(|natives| natives.get("windows").cloned())
            .map(|value| value.replace("${arch}", native_arch_token()))
        else {
            return Ok(None);
        };

        if let Some(downloads) = &self.downloads {
            if let Some(classifiers) = &downloads.classifiers {
                if let Some(download) = classifiers.get(&classifier_key) {
                    return Ok(Some(download.clone()));
                }
            }
        }

        if let Some(url) = &self.url {
            return Ok(Some(maven_download(url, &self.name, Some(&classifier_key))?));
        }

        Ok(None)
    }

    fn coordinate_classifier(&self) -> Option<&str> {
        let mut parts = self.name.split(':');
        parts.next()?;
        parts.next()?;
        parts.next()?;
        parts.next()
    }

    fn is_native_library_entry(&self) -> bool {
        self.coordinate_classifier()
            .is_some_and(|classifier| classifier.starts_with("natives-"))
    }
}

#[derive(Debug, Clone, Deserialize)]
struct LibraryDownloads {
    artifact: Option<DownloadDescriptor>,
    classifiers: Option<HashMap<String, DownloadDescriptor>>,
}

#[derive(Debug, Clone, Deserialize)]
struct LibraryExtractRules {
    exclude: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
struct DownloadDescriptor {
    url: String,
    sha1: Option<String>,
    path: Option<String>,
    #[serde(default)]
    id: Option<String>,
}

fn native_arch_token() -> &'static str {
    if cfg!(target_arch = "x86") {
        "32"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "64"
    }
}

fn native_classifier_matches(classifier: &str) -> bool {
    #[cfg(target_os = "windows")]
    {
        if cfg!(target_arch = "x86") {
            return matches!(classifier, "natives-windows-x86" | "natives-windows-32");
        }
        if cfg!(target_arch = "aarch64") {
            return matches!(classifier, "natives-windows-arm64" | "natives-windows-aarch64");
        }
        return matches!(classifier, "natives-windows" | "natives-windows-64");
    }

    #[cfg(target_os = "linux")]
    {
        if cfg!(target_arch = "x86") {
            return matches!(classifier, "natives-linux-x86" | "natives-linux-32");
        }
        if cfg!(target_arch = "aarch64") {
            return matches!(classifier, "natives-linux-arm64" | "natives-linux-aarch64");
        }
        return matches!(classifier, "natives-linux" | "natives-linux-64");
    }

    #[cfg(target_os = "macos")]
    {
        if cfg!(target_arch = "aarch64") {
            return classifier == "natives-macos-arm64";
        }
        return classifier == "natives-macos";
    }

    #[allow(unreachable_code)]
    false
}

#[derive(Debug, Deserialize)]
struct RuntimeAllManifest {
    #[serde(rename = "windows-x64")]
    windows_x64: HashMap<String, Vec<RuntimeComponentEnvelope>>,
}

#[derive(Debug, Deserialize)]
struct RuntimeComponentEnvelope {
    manifest: DownloadDescriptor,
    version: RuntimeComponentVersion,
}

#[derive(Debug, Deserialize)]
struct RuntimeComponentVersion {
    name: String,
}

#[derive(Debug, Deserialize)]
struct RuntimeManifest {
    files: HashMap<String, RuntimeManifestEntry>,
}

#[derive(Debug, Deserialize)]
struct RuntimeManifestEntry {
    #[serde(rename = "type")]
    kind: String,
    downloads: Option<RuntimeManifestDownloads>,
}

#[derive(Debug, Deserialize)]
struct RuntimeManifestDownloads {
    raw: Option<DownloadDescriptor>,
}

#[derive(Debug, Deserialize)]
struct AssetIndexPayload {
    objects: HashMap<String, AssetObject>,
}

#[derive(Debug, Deserialize)]
struct AssetObject {
    hash: String,
}

#[derive(Debug, Deserialize)]
struct FabricLoaderVersionEnvelope {
    loader: FabricLoaderDescriptor,
}

#[derive(Debug, Deserialize)]
struct FabricLoaderDescriptor {
    version: String,
    stable: bool,
}

fn maven_download(base_url: &str, coordinate: &str, classifier: Option<&str>) -> AppResult<DownloadDescriptor> {
    let mut parts = coordinate.split(':');
    let group = parts
        .next()
        .ok_or_else(|| AppError::Validation(format!("invalid maven coordinate: {coordinate}")))?;
    let artifact = parts
        .next()
        .ok_or_else(|| AppError::Validation(format!("invalid maven coordinate: {coordinate}")))?;
    let version = parts
        .next()
        .ok_or_else(|| AppError::Validation(format!("invalid maven coordinate: {coordinate}")))?;

    let group_path = group.replace('.', "/");
    let classifier_suffix = classifier.map(|value| format!("-{value}")).unwrap_or_default();
    let file_name = format!("{artifact}-{version}{classifier_suffix}.jar");
    let path = format!("{group_path}/{artifact}/{version}/{file_name}");
    let base = base_url.trim_end_matches('/');
    Ok(DownloadDescriptor {
        url: format!("{base}/{path}"),
        sha1: None,
        path: Some(path),
        id: None,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        apply_placeholders, ensure_cache_layout, ensure_managed_runtime, extract_natives,
        fetch_complete_version_by_id, fetch_version_manifest, http_client, native_archive_output_name,
        native_classifier_matches, parse_java_major_version, prepare_launch, Library,
        RuntimeAllManifest, Rule,
    };
    use std::{collections::HashMap, env, fs, path::PathBuf};

    use crate::{
        auth::MINECRAFT_OWNERSHIP_REQUIRED_MESSAGE,
        dto::{ProfileSummary, ProfileType},
        paths::AppPaths,
        state::AppState,
    };

    struct TestRoot {
        root: PathBuf,
    }

    impl TestRoot {
        fn new(prefix: &str) -> Self {
            let root = env::temp_dir()
                .join("blocksmith-live-tests")
                .join(format!("{prefix}-{}", uuid::Uuid::new_v4().simple()));
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

    #[test]
    fn placeholder_replacement_substitutes_values() {
        let mut replacements = HashMap::new();
        replacements.insert("name".to_string(), "Blocksmith".to_string());
        assert_eq!(apply_placeholders("hello ${name}", &replacements), "hello Blocksmith");
    }

    #[test]
    fn empty_rules_default_to_disallow_sequence_false() {
        assert!(!super::rules_match(&[
            Rule {
                action: "disallow".to_string(),
                os: None,
                features: None,
            },
        ]));
    }

    #[test]
    fn parse_java_major_version_handles_legacy_and_modern_outputs() {
        assert_eq!(
            parse_java_major_version("java version \"1.8.0_441\"\nJava(TM) SE Runtime Environment"),
            Some(8)
        );
        assert_eq!(
            parse_java_major_version("openjdk version \"21.0.7\" 2025-04-15"),
            Some(21)
        );
        assert_eq!(
            parse_java_major_version("openjdk 24 2025-03-18"),
            Some(24)
        );
    }

    #[test]
    fn runtime_catalog_deserializes_component_arrays() {
        let catalog: RuntimeAllManifest = serde_json::from_str(
            r#"
            {
              "windows-x64": {
                "java-runtime-epsilon": [
                  {
                    "manifest": {
                      "url": "https://example.com/runtime-manifest.json",
                      "sha1": "abc123",
                      "id": "runtime-manifest"
                    },
                    "version": {
                      "name": "25.0.1"
                    }
                  }
                ]
              }
            }
            "#,
        )
        .expect("runtime catalog should deserialize");

        let component = catalog
            .windows_x64
            .get("java-runtime-epsilon")
            .and_then(|entries| entries.first())
            .expect("expected runtime component entry");
        assert_eq!(component.version.name, "25.0.1");
        assert_eq!(component.manifest.url, "https://example.com/runtime-manifest.json");
    }

    #[test]
    fn native_library_entries_are_not_treated_as_classpath_jars() {
        let windows_native: Library = serde_json::from_str(
            r#"
            {
              "name": "org.lwjgl:lwjgl:3.4.1:natives-windows",
              "downloads": {
                "artifact": {
                  "path": "org/lwjgl/lwjgl/3.4.1/lwjgl-3.4.1-natives-windows.jar",
                  "sha1": "abc123",
                  "url": "https://libraries.minecraft.net/org/lwjgl/lwjgl/3.4.1/lwjgl-3.4.1-natives-windows.jar"
                }
              }
            }
            "#,
        )
        .expect("native library entry should deserialize");

        assert!(
            windows_native
                .artifact_download()
                .expect("artifact lookup should succeed")
                .is_none(),
            "native-only entries should stay off the Java classpath"
        );
        assert!(
            windows_native
                .native_download()
                .expect("native lookup should succeed")
                .is_some(),
            "matching native entries should be extracted"
        );
    }

    #[test]
    fn native_classifier_matching_filters_wrong_architectures() {
        assert!(native_classifier_matches("natives-windows"));
        assert!(!native_classifier_matches("natives-windows-arm64"));
        assert!(!native_classifier_matches("natives-windows-x86"));
    }

    #[test]
    fn native_archive_output_name_flattens_lwjgl_layout() {
        assert_eq!(
            native_archive_output_name("windows/x64/org/lwjgl/lwjgl.dll"),
            Some("lwjgl.dll".to_string())
        );
        assert_eq!(native_archive_output_name("META-INF/windows/x64/org/lwjgl/lwjgl.dll.sha1"), None);
        assert_eq!(native_archive_output_name("META-INF/versions/11/module-info.class"), None);
    }

    #[test]
    fn prepare_launch_requires_verified_owner_before_downloads() {
        let test_root = TestRoot::new("ownership-gate");
        let paths = test_root.paths();
        paths.ensure_layout().expect("should create isolated app layout");
        let state = AppState::bootstrap(paths).expect("should bootstrap isolated state");
        let profile = ProfileSummary {
            id: "profile-ownership".to_string(),
            name: "Ownership Gate".to_string(),
            profile_type: ProfileType::Vanilla,
            minecraft_version: "1.21.5".to_string(),
            loader_version: None,
            directory_path: state
                .paths
                .profile_root("profile-ownership")
                .to_string_lossy()
                .into_owned(),
            account_id: None,
            java_path: None,
            memory_min_mb: None,
            memory_max_mb: None,
            jvm_args: String::new(),
            launch_args: String::new(),
            notes: None,
            created_at: "2026-04-22T00:00:00Z".to_string(),
            updated_at: "2026-04-22T00:00:00Z".to_string(),
            last_played_at: None,
        };

        let error = match prepare_launch(&state, &profile) {
            Ok(_) => panic!("launch should be blocked"),
            Err(error) => error,
        };

        assert_eq!(error.to_string(), MINECRAFT_OWNERSHIP_REQUIRED_MESSAGE);
        assert!(
            !state.paths.cache_dir.join("versions").exists(),
            "prepare should fail before creating Minecraft download cache directories"
        );
    }

    #[test]
    #[ignore = "live network smoke test"]
    fn live_can_install_managed_runtime_for_latest_release() {
        let test_root = TestRoot::new("managed-runtime");
        let paths = test_root.paths();
        paths.ensure_layout().expect("should create isolated app layout");
        let state = AppState::bootstrap(paths).expect("should bootstrap isolated state");
        let cache = ensure_cache_layout(&state.paths).expect("should create minecraft cache layout");
        let client = http_client().expect("should build HTTP client");
        let manifest = fetch_version_manifest(&client).expect("should fetch Mojang version manifest");
        let latest_release = manifest
            .versions
            .iter()
            .find(|entry| entry.kind == "release")
            .expect("should expose at least one release version");
        let raw = fetch_complete_version_by_id(&client, &manifest, &latest_release.id)
            .expect("should fetch latest release metadata");
        let java_version = raw
            .java_version
            .expect("latest release should declare a Java runtime requirement");

        let executable = ensure_managed_runtime(&state, &client, &cache, &java_version)
            .expect("should install managed runtime");

        assert!(executable.exists(), "managed Java executable should exist");
        assert!(
            executable.starts_with(&state.paths.runtimes_dir),
            "managed Java should live under the Blocksmith runtimes directory"
        );
    }

    #[test]
    #[ignore = "live network smoke test"]
    fn live_extracts_lwjgl_natives_for_latest_release() {
        let test_root = TestRoot::new("lwjgl-natives");
        let paths = test_root.paths();
        paths.ensure_layout().expect("should create isolated app layout");
        let state = AppState::bootstrap(paths).expect("should bootstrap isolated state");
        let client = http_client().expect("should build HTTP client");
        let manifest = fetch_version_manifest(&client).expect("should fetch Mojang version manifest");
        let latest_release = manifest
            .versions
            .iter()
            .find(|entry| entry.kind == "release")
            .expect("should expose at least one release version");
        let raw = fetch_complete_version_by_id(&client, &manifest, &latest_release.id)
            .expect("should fetch latest release metadata");

        let extracted = extract_natives(&state.paths.temp_dir.join("natives"), &raw.libraries)
            .expect("should extract native libraries")
            .expect("latest release should require native libraries");

        assert!(
            extracted.join("lwjgl.dll").exists(),
            "expected lwjgl.dll to be extracted for the Windows x64 runtime"
        );
    }
}
