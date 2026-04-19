use std::time::Duration;

use serde::Deserialize;

use crate::{
    dto::{ContentSearchResult, ContentType, ProfileSummary, ProfileType},
    error::{AppError, AppResult},
};

const MODRINTH_API: &str = "https://api.modrinth.com/v2";

pub struct ModrinthClient {
    client: reqwest::blocking::Client,
}

impl ModrinthClient {
    pub fn new() -> AppResult<Self> {
        let client = reqwest::blocking::Client::builder()
            .user_agent("Blocksmith/0.1.0")
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(120))
            .build()?;

        Ok(Self { client })
    }

    pub fn search_projects(
        &self,
        query: &str,
        profile: Option<&ProfileSummary>,
        content_type: Option<ContentType>,
    ) -> AppResult<Vec<ContentSearchResult>> {
        let trimmed_query = query.trim();
        if trimmed_query.is_empty() {
            return Ok(Vec::new());
        }

        let mut request = self.client.get(format!("{MODRINTH_API}/search")).query(&[
            ("query", trimmed_query),
            ("limit", "24"),
            ("index", "relevance"),
        ]);

        if let Some(content_type) = content_type {
            let project_type = modrinth_project_type(content_type)?;
            let facets = serde_json::to_string(&vec![vec![format!("project_type:{project_type}")]])?;
            request = request.query(&[("facets", facets.as_str())]);
        }

        let response = request.send()?.error_for_status()?;
        let payload: SearchResponse = response.json()?;

        let results = payload
            .hits
            .into_iter()
            .filter_map(|hit| {
                let mapped_type = content_type_from_modrinth(&hit.project_type).ok()?;
                let supported_versions = hit.versions.unwrap_or_default();

                if let Some(profile) = profile {
                    if !supported_versions.is_empty()
                        && !supported_versions
                            .iter()
                            .any(|version| version == &profile.minecraft_version)
                    {
                        return None;
                    }
                }

                Some(ContentSearchResult {
                    project_id: hit.project_id,
                    slug: hit.slug,
                    title: hit.title,
                    summary: hit.description.unwrap_or_default(),
                    author: hit.author,
                    icon_url: hit.icon_url,
                    content_type: mapped_type,
                    supported_versions,
                    supported_loaders: hit.loaders.unwrap_or_default(),
                    categories: hit
                        .display_categories
                        .filter(|categories| !categories.is_empty())
                        .or(hit.categories)
                        .unwrap_or_default(),
                })
            })
            .collect();

        Ok(results)
    }

    pub fn get_project(&self, project_id: &str) -> AppResult<ProjectResponse> {
        let response = self
            .client
            .get(format!("{MODRINTH_API}/project/{project_id}"))
            .send()?
            .error_for_status()?;

        Ok(response.json()?)
    }

    pub fn get_version(&self, version_id: &str) -> AppResult<VersionResponse> {
        let response = self
            .client
            .get(format!("{MODRINTH_API}/version/{version_id}"))
            .send()?
            .error_for_status()?;

        Ok(response.json()?)
    }

    pub fn get_project_versions(&self, project_id: &str) -> AppResult<Vec<VersionResponse>> {
        let response = self
            .client
            .get(format!("{MODRINTH_API}/project/{project_id}/version"))
            .send()?
            .error_for_status()?;

        Ok(response.json()?)
    }

    pub fn get_latest_compatible_version(
        &self,
        project_id: &str,
        profile: &ProfileSummary,
        content_type: ContentType,
    ) -> AppResult<VersionResponse> {
        let versions = self.get_project_versions(project_id)?;
        versions
            .into_iter()
            .find(|version| version_is_compatible(profile, content_type, version))
            .ok_or_else(|| {
                AppError::NotFound(format!(
                    "no compatible {} version found for Minecraft {}",
                    content_type,
                    profile.minecraft_version
                ))
            })
    }

    pub fn download_bytes(&self, url: &str) -> AppResult<Vec<u8>> {
        let response = self.client.get(url).send()?.error_for_status()?;
        Ok(response.bytes()?.to_vec())
    }
}

pub fn version_is_compatible(
    profile: &ProfileSummary,
    content_type: ContentType,
    version: &VersionResponse,
) -> bool {
    if !version.game_versions.is_empty()
        && !version
            .game_versions
            .iter()
            .any(|candidate| candidate == &profile.minecraft_version)
    {
        return false;
    }

    match content_type {
        ContentType::Mod | ContentType::Modpack => match profile.profile_type {
            ProfileType::Fabric => version
                .loaders
                .iter()
                .any(|loader| loader.eq_ignore_ascii_case("fabric")),
            ProfileType::Vanilla => version.loaders.is_empty()
                || version.loaders.iter().any(|loader| {
                    loader.eq_ignore_ascii_case("vanilla")
                        || loader.eq_ignore_ascii_case("minecraft")
                }),
        },
        ContentType::ResourcePack | ContentType::ShaderPack | ContentType::Datapack => true,
    }
}

pub fn content_type_from_modrinth(value: &str) -> AppResult<ContentType> {
    match value {
        "mod" => Ok(ContentType::Mod),
        "resourcepack" => Ok(ContentType::ResourcePack),
        "shader" => Ok(ContentType::ShaderPack),
        "datapack" => Ok(ContentType::Datapack),
        "modpack" => Ok(ContentType::Modpack),
        other => Err(AppError::Validation(format!(
            "unsupported Modrinth project type: {other}"
        ))),
    }
}

pub fn modrinth_project_type(content_type: ContentType) -> AppResult<&'static str> {
    match content_type {
        ContentType::Mod => Ok("mod"),
        ContentType::ResourcePack => Ok("resourcepack"),
        ContentType::ShaderPack => Ok("shader"),
        ContentType::Datapack => Ok("datapack"),
        ContentType::Modpack => Ok("modpack"),
    }
}

#[derive(Debug, Deserialize)]
pub struct SearchResponse {
    pub hits: Vec<SearchHit>,
}

#[derive(Debug, Deserialize)]
pub struct SearchHit {
    pub project_id: String,
    pub project_type: String,
    pub slug: String,
    pub author: Option<String>,
    pub title: String,
    pub description: Option<String>,
    pub categories: Option<Vec<String>>,
    pub display_categories: Option<Vec<String>>,
    pub versions: Option<Vec<String>>,
    pub loaders: Option<Vec<String>>,
    pub icon_url: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct ProjectResponse {
    pub id: String,
    pub slug: String,
    pub title: String,
    pub project_type: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct VersionResponse {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub version_number: String,
    pub changelog: Option<String>,
    #[serde(default)]
    pub game_versions: Vec<String>,
    #[serde(default)]
    pub loaders: Vec<String>,
    #[serde(default)]
    pub files: Vec<VersionFile>,
    #[serde(default)]
    pub dependencies: Vec<VersionDependency>,
}

impl VersionResponse {
    pub fn primary_file(&self) -> AppResult<&VersionFile> {
        self.files
            .iter()
            .find(|file| file.primary)
            .or_else(|| self.files.first())
            .ok_or_else(|| {
                AppError::NotFound(format!("version {} does not expose any downloadable files", self.id))
            })
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct VersionFile {
    pub url: String,
    pub filename: String,
    pub primary: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VersionDependency {
    pub project_id: Option<String>,
    pub version_id: Option<String>,
    #[serde(rename = "dependency_type")]
    pub dependency_type: String,
}
