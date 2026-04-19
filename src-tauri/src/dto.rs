#![allow(dead_code)]

use std::{fmt, path::Path, str::FromStr};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProfileType {
    Vanilla,
    Fabric,
}

impl Default for ProfileType {
    fn default() -> Self {
        Self::Vanilla
    }
}

impl ProfileType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Vanilla => "vanilla",
            Self::Fabric => "fabric",
        }
    }
}

impl fmt::Display for ProfileType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ProfileType {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "vanilla" => Ok(Self::Vanilla),
            "fabric" => Ok(Self::Fabric),
            other => Err(AppError::Validation(format!(
                "unsupported profile type: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum InstallScope {
    Profile,
    World,
}

impl Default for InstallScope {
    fn default() -> Self {
        Self::Profile
    }
}

impl InstallScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Profile => "profile",
            Self::World => "world",
        }
    }
}

impl fmt::Display for InstallScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for InstallScope {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "profile" => Ok(Self::Profile),
            "world" => Ok(Self::World),
            other => Err(AppError::Validation(format!(
                "unsupported install scope: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContentType {
    Mod,
    ResourcePack,
    ShaderPack,
    Datapack,
    Modpack,
}

impl Default for ContentType {
    fn default() -> Self {
        Self::Mod
    }
}

impl ContentType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mod => "mod",
            Self::ResourcePack => "resource_pack",
            Self::ShaderPack => "shader_pack",
            Self::Datapack => "datapack",
            Self::Modpack => "modpack",
        }
    }

    pub fn default_relative_directory(self) -> &'static str {
        match self {
            Self::Mod => "minecraft/mods",
            Self::ResourcePack => "minecraft/resourcepacks",
            Self::ShaderPack => "minecraft/shaderpacks",
            Self::Datapack => "minecraft/datapacks",
            Self::Modpack => "minecraft",
        }
    }
}

impl fmt::Display for ContentType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ContentType {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "mod" => Ok(Self::Mod),
            "resource_pack" => Ok(Self::ResourcePack),
            "shader_pack" => Ok(Self::ShaderPack),
            "datapack" => Ok(Self::Datapack),
            "modpack" => Ok(Self::Modpack),
            other => Err(AppError::Validation(format!(
                "unsupported content type: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileSummary {
    pub id: String,
    pub name: String,
    pub profile_type: ProfileType,
    pub minecraft_version: String,
    pub loader_version: Option<String>,
    pub directory_path: String,
    pub account_id: Option<String>,
    pub java_path: Option<String>,
    pub memory_min_mb: Option<u32>,
    pub memory_max_mb: Option<u32>,
    pub jvm_args: String,
    pub launch_args: String,
    pub notes: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub last_played_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileDetail {
    #[serde(flatten)]
    pub summary: ProfileSummary,
    pub launcher_directory: String,
    pub minecraft_directory: String,
}

impl ProfileDetail {
    pub fn from_summary(summary: ProfileSummary) -> Self {
        let root = Path::new(&summary.directory_path);
        let launcher_directory = root.join(".launcher").to_string_lossy().into_owned();
        let minecraft_directory = root.join("minecraft").to_string_lossy().into_owned();

        Self {
            summary,
            launcher_directory,
            minecraft_directory,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProfileInput {
    pub name: String,
    pub profile_type: ProfileType,
    pub minecraft_version: String,
    #[serde(default)]
    pub loader_version: Option<String>,
    #[serde(default)]
    pub account_id: Option<String>,
    #[serde(default)]
    pub java_path: Option<String>,
    #[serde(default)]
    pub memory_min_mb: Option<u32>,
    #[serde(default)]
    pub memory_max_mb: Option<u32>,
    #[serde(default)]
    pub jvm_args: Option<String>,
    #[serde(default)]
    pub launch_args: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateProfileInput {
    pub source_profile_id: String,
    pub new_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LaunchPlan {
    pub profile_id: String,
    pub java_executable: String,
    pub working_directory: String,
    pub game_version: String,
    pub loader_version: Option<String>,
    pub memory_args: Vec<String>,
    pub jvm_args: Vec<String>,
    pub game_args: Vec<String>,
    pub libraries: Vec<String>,
    pub assets_root: String,
    pub natives_dir: Option<String>,
    pub account_id: Option<String>,
    pub main_class: String,
    pub classpath: Vec<String>,
    pub client_jar: String,
    pub logging_config: Option<String>,
    pub username: String,
    pub user_type: String,
    pub online: bool,
    pub command_preview: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LaunchHistoryEntry {
    pub id: String,
    pub profile_id: String,
    pub account_id: Option<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub status: String,
    pub log_path: String,
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModrinthSearchInput {
    pub query: String,
    #[serde(default)]
    pub profile_id: Option<String>,
    #[serde(default)]
    pub content_type: Option<ContentType>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ContentSearchResult {
    pub project_id: String,
    pub slug: String,
    pub title: String,
    pub summary: String,
    pub author: Option<String>,
    pub icon_url: Option<String>,
    pub content_type: ContentType,
    pub supported_versions: Vec<String>,
    pub supported_loaders: Vec<String>,
    pub categories: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DependencyWarning {
    pub project_id: String,
    pub version_id: Option<String>,
    pub kind: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct InstallPlan {
    pub profile_id: String,
    pub project_title: String,
    pub version_label: String,
    pub content_type: ContentType,
    pub install_scope: InstallScope,
    pub project_id: String,
    pub version_id: String,
    pub target_rel_path: Option<String>,
    pub target_path: String,
    pub rollback_path: Option<String>,
    pub dependencies: Vec<DependencyWarning>,
    pub compatibility_warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateInstallPlanInput {
    pub profile_id: String,
    pub project_id: String,
    pub content_type: ContentType,
    #[serde(default)]
    pub install_scope: Option<InstallScope>,
    #[serde(default)]
    pub target_rel_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyInstallPlanInput {
    pub plan: InstallPlan,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportMrpackInput {
    pub source_path: String,
    #[serde(default)]
    pub new_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallModpackInput {
    pub project_id: String,
    #[serde(default)]
    pub new_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct InstalledContentRecord {
    pub id: String,
    pub profile_id: String,
    pub content_type: String,
    pub install_scope: String,
    pub provider: String,
    pub project_id: String,
    pub version_id: String,
    pub slug: String,
    pub name: String,
    pub local_file_path: String,
    pub target_rel_path: Option<String>,
    pub file_hash: Option<String>,
    pub enabled: bool,
    pub version_number: Option<String>,
    pub installed_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToggleInstalledContentInput {
    pub installed_content_id: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCandidate {
    pub installed_content_id: String,
    pub profile_id: String,
    pub project_id: String,
    pub current_version_id: String,
    pub target_version_id: String,
    pub current_version_label: Option<String>,
    pub target_version_label: Option<String>,
    pub changelog: Option<String>,
    pub compatibility_notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AccountSummary {
    pub id: String,
    pub username: String,
    pub uuid: String,
    pub provider: String,
    pub avatar_url: Option<String>,
    pub current_skin_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub is_authenticated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MinecraftVersionSummary {
    pub id: String,
    pub kind: String,
    pub release_time: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FabricLoaderSummary {
    pub version: String,
    pub stable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateLocalAccountInput {
    pub username: String,
    #[serde(default)]
    pub uuid: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SkinEntry {
    pub id: String,
    pub local_file_path: String,
    pub display_name: String,
    pub model_variant: String,
    pub tags: Vec<String>,
    pub thumbnail_path: Option<String>,
    pub preview_data_url: Option<String>,
    pub imported_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSkinInput {
    #[serde(default)]
    pub source_path: Option<String>,
    #[serde(default)]
    pub file_name: Option<String>,
    #[serde(default)]
    pub source_bytes: Option<Vec<u8>>,
    #[serde(default)]
    pub display_name: Option<String>,
    pub model_variant: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplySkinInput {
    pub account_id: String,
    pub skin_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SharedProfileManifest {
    pub export_version: u32,
    pub profile_name: String,
    pub profile_type: ProfileType,
    pub minecraft_version: String,
    pub loader_version: Option<String>,
    pub java_path: Option<String>,
    pub memory_min_mb: Option<u32>,
    pub memory_max_mb: Option<u32>,
    pub jvm_args: String,
    pub launch_args: String,
    pub notes: Option<String>,
    pub content: Vec<SharedContentReference>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportShareInput {
    pub share_code: String,
    #[serde(default)]
    pub new_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportShareFileInput {
    pub source_path: String,
    #[serde(default)]
    pub new_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareExportResult {
    pub share_code: String,
    pub export_path: String,
    pub manifest: SharedProfileManifest,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SharedContentReference {
    pub project_id: String,
    pub version_id: String,
    pub content_type: String,
    pub install_scope: String,
    pub target_rel_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingEntry {
    pub key: String,
    pub value: String,
    pub category: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DashboardSnapshot {
    pub profile_count: i64,
    pub vanilla_profile_count: i64,
    pub fabric_profile_count: i64,
    pub latest_profile_name: Option<String>,
    pub signed_in_account_count: i64,
    pub local_skin_count: i64,
    pub pending_update_count: i64,
}

pub fn normalize_profile_name(value: &str) -> AppResult<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(AppError::Validation(
            "profile name cannot be empty".to_string(),
        ));
    }

    let illegal = ['<', '>', ':', '"', '/', '\\', '|', '?', '*'];
    if trimmed.chars().any(|character| illegal.contains(&character)) {
        return Err(AppError::Validation(
            "profile name contains invalid filesystem characters".to_string(),
        ));
    }

    Ok(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::{normalize_profile_name, ProfileDetail, ProfileSummary, ProfileType};

    #[test]
    fn normalize_profile_name_rejects_invalid_characters() {
        assert!(normalize_profile_name("Bad/Name").is_err());
        assert!(normalize_profile_name("   ").is_err());
    }

    #[test]
    fn profile_detail_derives_nested_directories() {
        let detail = ProfileDetail::from_summary(ProfileSummary {
            id: "profile-1".to_string(),
            name: "Vanilla".to_string(),
            profile_type: ProfileType::Vanilla,
            minecraft_version: "1.21.5".to_string(),
            loader_version: None,
            directory_path: r"C:\Profiles\profile-1".to_string(),
            account_id: None,
            java_path: None,
            memory_min_mb: None,
            memory_max_mb: None,
            jvm_args: String::new(),
            launch_args: String::new(),
            notes: None,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
            last_played_at: None,
        });

        assert!(detail.launcher_directory.ends_with(r"profile-1\.launcher"));
        assert!(detail.minecraft_directory.ends_with(r"profile-1\minecraft"));
    }
}
