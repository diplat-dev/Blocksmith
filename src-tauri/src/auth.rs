use std::{
    io::{Read, Write},
    net::TcpListener,
    process::Command,
    thread,
    time::{Duration, Instant},
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::Utc;
use rand::{distributions::Alphanumeric, thread_rng, Rng};
use reqwest::blocking::{multipart, Client};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::Url;
use uuid::Uuid;

use crate::{
    credential_store,
    dto::AccountSummary,
    error::{AppError, AppResult},
    state::AppState,
};

pub const PACKAGED_MICROSOFT_CLIENT_ID: &str = "58a4d0d2-52b6-4209-b74f-fc2c2033e9d8";
pub const MINECRAFT_OWNERSHIP_REQUIRED_MESSAGE: &str =
    "Sign in once with a Microsoft account that owns Minecraft to enable downloads and launch.";

const LIVE_AUTHORIZE_URL: &str = "https://login.live.com/oauth20_authorize.srf";
const LIVE_TOKEN_URL: &str = "https://login.live.com/oauth20_token.srf";
const XBOX_AUTH_URL: &str = "https://user.auth.xboxlive.com/user/authenticate";
const XSTS_AUTH_URL: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
const MINECRAFT_LOGIN_URL: &str = "https://api.minecraftservices.com/authentication/login_with_xbox";
const MINECRAFT_ENTITLEMENTS_URL: &str = "https://api.minecraftservices.com/entitlements/mcstore";
const MINECRAFT_PROFILE_URL: &str = "https://api.minecraftservices.com/minecraft/profile";
const MINECRAFT_SKIN_UPLOAD_URL: &str = "https://api.minecraftservices.com/minecraft/profile/skins";
const MICROSOFT_SCOPE: &str = "XboxLive.signin XboxLive.offline_access";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchAuthSession {
    pub account_id: Option<String>,
    pub username: String,
    pub uuid: String,
    pub access_token: String,
    pub user_type: String,
    pub xuid: Option<String>,
    pub online: bool,
}

pub fn sign_in_with_microsoft(state: &AppState) -> AppResult<AccountSummary> {
    let client_id = load_required_setting(state, "microsoft_client_id")?;
    let auth_client = http_client()?;

    let (listeners, redirect_uri) = start_loopback_listener()?;
    let state_token = random_token(32);
    let code_verifier = random_token(64);
    let code_challenge = pkce_challenge(&code_verifier);
    let auth_url = build_authorization_url(&client_id, &redirect_uri, &state_token, &code_challenge)?;

    open_system_browser(auth_url.as_str())?;
    let auth_code = wait_for_oauth_code(listeners, &state_token)?;

    let live_tokens =
        exchange_authorization_code(&auth_client, &client_id, &redirect_uri, &code_verifier, &auth_code)?;
    persist_microsoft_account(state, &auth_client, live_tokens.refresh_token, &live_tokens.access_token)
}

pub fn launcher_unlocked(state: &AppState) -> AppResult<bool> {
    let connection = state.db()?;
    let count: i64 = connection.query_row(
        "
        SELECT COUNT(*)
        FROM accounts
        WHERE provider = 'microsoft' AND owns_minecraft = 1
        ",
        [],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

pub fn ensure_launcher_unlocked(state: &AppState) -> AppResult<()> {
    if launcher_unlocked(state)? {
        Ok(())
    } else {
        Err(AppError::Validation(
            MINECRAFT_OWNERSHIP_REQUIRED_MESSAGE.to_string(),
        ))
    }
}

pub fn resolve_launch_auth_session(
    state: &AppState,
    account_id: Option<&str>,
    offline_fallback_name: &str,
) -> AppResult<LaunchAuthSession> {
    let Some(account_id) = account_id else {
        return Ok(offline_session(None, offline_fallback_name, None));
    };

    let connection = state.db()?;
    let account_row = connection
        .query_row(
            "
            SELECT accounts.id, accounts.username, accounts.uuid, accounts.provider, account_tokens.token_reference
            FROM accounts
            LEFT JOIN account_tokens ON account_tokens.account_id = accounts.id
            WHERE accounts.id = ?1
            ",
            params![account_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| AppError::NotFound(format!("account not found: {account_id}")))?;
    drop(connection);

    let (account_id, username, uuid, provider, token_reference) = account_row;
    if !provider.eq_ignore_ascii_case("microsoft") {
        return Ok(offline_session(Some(account_id), &username, Some(uuid)));
    }

    let token_reference = token_reference.ok_or_else(|| {
        AppError::Validation(format!(
            "microsoft account {account_id} is missing its stored refresh token"
        ))
    })?;
    let refresh_token = credential_store::read_secret(&token_reference)?.ok_or_else(|| {
        AppError::Validation(format!(
            "refresh token for account {account_id} was not found in Windows Credential Manager"
        ))
    })?;

    let client_id = load_required_setting(state, "microsoft_client_id")?;
    let auth_client = http_client()?;
    let live_tokens = refresh_live_token(&auth_client, &client_id, &refresh_token)?;
    let minecraft = exchange_minecraft_chain(&auth_client, &live_tokens.access_token)?;
    persist_refresh_token(state, &account_id, &token_reference, &username, &live_tokens.refresh_token)?;
    update_microsoft_account_profile(
        state,
        &account_id,
        &minecraft.profile.name,
        &minecraft.profile.id,
        minecraft.profile.primary_skin_url(),
    )?;

    Ok(LaunchAuthSession {
        account_id: Some(account_id),
        username: minecraft.profile.name,
        uuid: minecraft.profile.id,
        access_token: minecraft.minecraft_access_token,
        user_type: "msa".to_string(),
        xuid: Some(minecraft.user_hash),
        online: true,
    })
}

pub fn upload_skin_for_account(
    state: &AppState,
    account_id: &str,
    skin_path: &std::path::Path,
    model_variant: &str,
) -> AppResult<()> {
    let session = resolve_launch_auth_session(state, Some(account_id), "Player")?;
    if !session.online {
        return Err(AppError::Validation(
            "only authenticated Microsoft accounts can upload skins to Minecraft services"
                .to_string(),
        ));
    }

    let form = multipart::Form::new()
        .text("variant", model_variant.to_string())
        .file("file", skin_path)
        .map_err(|error| AppError::Internal(format!("failed to prepare skin upload: {error}")))?;

    http_client()?
        .post(MINECRAFT_SKIN_UPLOAD_URL)
        .bearer_auth(session.access_token)
        .multipart(form)
        .send()?
        .error_for_status()?;

    Ok(())
}

pub fn delete_persisted_account_token(state: &AppState, account_id: &str) -> AppResult<()> {
    let connection = state.db()?;
    let token_reference: Option<String> = connection
        .query_row(
            "SELECT token_reference FROM account_tokens WHERE account_id = ?1",
            params![account_id],
            |row| row.get(0),
        )
        .optional()?;
    drop(connection);

    if let Some(token_reference) = token_reference {
        credential_store::delete_secret(&token_reference)?;
    }

    Ok(())
}

fn persist_microsoft_account(
    state: &AppState,
    client: &Client,
    refresh_token: String,
    live_access_token: &str,
) -> AppResult<AccountSummary> {
    let minecraft = exchange_minecraft_chain(client, live_access_token)?;
    let connection = state.db()?;
    let existing_id: Option<String> = connection
        .query_row(
            "SELECT id FROM accounts WHERE provider = 'microsoft' AND uuid = ?1",
            params![minecraft.profile.id],
            |row| row.get(0),
        )
        .optional()?;

    let account_id = existing_id.unwrap_or_else(|| format!("account-{}", Uuid::new_v4().simple()));
    let now = Utc::now().to_rfc3339();
    let token_reference = credential_target(&account_id);
    credential_store::write_secret(&token_reference, &minecraft.profile.name, &refresh_token)?;

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
        VALUES (?1, ?2, ?3, 'microsoft', ?4, NULL, 1, ?5, ?6, ?7)
        ON CONFLICT(id) DO UPDATE SET
          username = excluded.username,
          uuid = excluded.uuid,
          provider = excluded.provider,
          avatar_url = excluded.avatar_url,
          owns_minecraft = excluded.owns_minecraft,
          ownership_verified_at = excluded.ownership_verified_at,
          updated_at = excluded.updated_at
        ",
        params![
            account_id,
            minecraft.profile.name,
            minecraft.profile.id,
            minecraft.profile.primary_skin_url(),
            now,
            now,
            now,
        ],
    )?;

    connection.execute(
        "
        INSERT INTO account_tokens (account_id, token_reference, updated_at)
        VALUES (?1, ?2, ?3)
        ON CONFLICT(account_id) DO UPDATE SET
          token_reference = excluded.token_reference,
          updated_at = excluded.updated_at
        ",
        params![account_id, token_reference, now],
    )?;

    drop(connection);
    account_summary(state, &account_id)
}

fn persist_refresh_token(
    state: &AppState,
    account_id: &str,
    token_reference: &str,
    username: &str,
    refresh_token: &str,
) -> AppResult<()> {
    credential_store::write_secret(token_reference, username, refresh_token)?;
    let connection = state.db()?;
    connection.execute(
        "UPDATE account_tokens SET updated_at = ?1 WHERE account_id = ?2",
        params![Utc::now().to_rfc3339(), account_id],
    )?;
    Ok(())
}

fn update_microsoft_account_profile(
    state: &AppState,
    account_id: &str,
    username: &str,
    uuid: &str,
    avatar_url: Option<String>,
) -> AppResult<()> {
    let connection = state.db()?;
    connection.execute(
        "
        UPDATE accounts
        SET username = ?1,
            uuid = ?2,
            avatar_url = ?3,
            owns_minecraft = 1,
            ownership_verified_at = ?4,
            updated_at = ?4
        WHERE id = ?5
        ",
        params![username, uuid, avatar_url, Utc::now().to_rfc3339(), account_id],
    )?;
    Ok(())
}

fn account_summary(state: &AppState, account_id: &str) -> AppResult<AccountSummary> {
    let connection = state.db()?;
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
        .ok_or_else(|| AppError::NotFound(format!("account not found: {account_id}")))
}

fn load_required_setting(state: &AppState, key: &str) -> AppResult<String> {
    let connection = state.db()?;
    let value: Option<String> = connection
        .query_row("SELECT value FROM settings WHERE key = ?1", params![key], |row| row.get(0))
        .optional()?;

    let mut value = value.unwrap_or_default();
    if key == "microsoft_client_id" && value.trim().is_empty() {
        value = PACKAGED_MICROSOFT_CLIENT_ID.to_string();
    }

    if value.trim().is_empty() {
        return Err(AppError::Validation(format!(
            "setting '{key}' must be configured before this integration can run"
        )));
    }

    Ok(value)
}

fn http_client() -> AppResult<Client> {
    reqwest::blocking::Client::builder()
        .user_agent("Blocksmith/0.1.0")
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(Into::into)
}

fn start_loopback_listener() -> AppResult<(Vec<TcpListener>, String)> {
    let ipv4_listener = TcpListener::bind("127.0.0.1:0")?;
    let port = ipv4_listener.local_addr()?.port();

    let mut listeners = vec![ipv4_listener];
    if let Ok(ipv6_listener) = TcpListener::bind(format!("[::1]:{port}")) {
        listeners.push(ipv6_listener);
    }

    Ok((listeners, format!("http://localhost:{port}/callback")))
}

fn build_authorization_url(
    client_id: &str,
    redirect_uri: &str,
    state: &str,
    code_challenge: &str,
) -> AppResult<Url> {
    let mut url = Url::parse(LIVE_AUTHORIZE_URL)
        .map_err(|error| AppError::Internal(format!("failed to build authorization URL: {error}")))?;
    url.query_pairs_mut()
        .append_pair("client_id", client_id)
        .append_pair("response_type", "code")
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("scope", MICROSOFT_SCOPE)
        .append_pair("state", state)
        .append_pair("code_challenge", code_challenge)
        .append_pair("code_challenge_method", "S256");
    Ok(url)
}

fn wait_for_oauth_code(listeners: Vec<TcpListener>, expected_state: &str) -> AppResult<String> {
    for listener in &listeners {
        listener.set_nonblocking(true)?;
    }
    let deadline = Instant::now() + Duration::from_secs(240);

    loop {
        for listener in &listeners {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let mut buffer = [0u8; 4096];
                    let read = stream.read(&mut buffer)?;
                    let request = String::from_utf8_lossy(&buffer[..read]);
                    let request_line = request.lines().next().unwrap_or_default();
                    let path = request_line
                        .split_whitespace()
                        .nth(1)
                        .ok_or_else(|| AppError::Validation("oauth callback request was malformed".to_string()))?;
                    let callback_url = Url::parse(&format!("http://localhost{path}")).map_err(|error| {
                        AppError::Validation(format!("oauth callback could not be parsed: {error}"))
                    })?;

                    let state = callback_url
                        .query_pairs()
                        .find(|(key, _)| key == "state")
                        .map(|(_, value)| value.into_owned());
                    let code = callback_url
                        .query_pairs()
                        .find(|(key, _)| key == "code")
                        .map(|(_, value)| value.into_owned());
                    let error = callback_url
                        .query_pairs()
                        .find(|(key, _)| key == "error")
                        .map(|(_, value)| value.into_owned());

                    let response = if let Some(ref error) = error {
                        format!(
                            "HTTP/1.1 400 Bad Request\r\nContent-Type: text/html\r\n\r\n<h1>Blocksmith sign-in failed</h1><p>{error}</p>"
                        )
                    } else {
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n<h1>Blocksmith sign-in complete</h1><p>You can close this window and return to the launcher.</p>".to_string()
                    };
                    stream.write_all(response.as_bytes())?;
                    stream.flush()?;

                    if let Some(error) = error {
                        return Err(AppError::Validation(format!(
                            "microsoft sign-in was cancelled or denied: {error}"
                        )));
                    }

                    if state.as_deref() != Some(expected_state) {
                        return Err(AppError::Validation(
                            "oauth callback state did not match the expected sign-in session"
                                .to_string(),
                        ));
                    }

                    return code.ok_or_else(|| {
                        AppError::Validation("oauth callback did not include an authorization code".to_string())
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(error) => return Err(error.into()),
            }
        }

        if Instant::now() > deadline {
            return Err(AppError::Validation(
                "timed out waiting for the Microsoft sign-in callback".to_string(),
            ));
        }

        thread::sleep(Duration::from_millis(150));
    }
}

fn open_system_browser(url: &str) -> AppResult<()> {
    Command::new("rundll32")
        .args(["url.dll,FileProtocolHandler", url])
        .spawn()
        .map_err(|error| AppError::Internal(format!("failed to open the system browser: {error}")))?;
    Ok(())
}

fn exchange_authorization_code(
    client: &Client,
    client_id: &str,
    redirect_uri: &str,
    code_verifier: &str,
    code: &str,
) -> AppResult<LiveTokenResponse> {
    let response = client
        .post(LIVE_TOKEN_URL)
        .form(&[
            ("client_id", client_id),
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("scope", MICROSOFT_SCOPE),
            ("code_verifier", code_verifier),
        ])
        .send()?
        .error_for_status()?;

    Ok(response.json()?)
}

fn refresh_live_token(client: &Client, client_id: &str, refresh_token: &str) -> AppResult<LiveTokenResponse> {
    let response = client
        .post(LIVE_TOKEN_URL)
        .form(&[
            ("client_id", client_id),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("scope", MICROSOFT_SCOPE),
        ])
        .send()?
        .error_for_status()?;

    Ok(response.json()?)
}

fn exchange_minecraft_chain(client: &Client, live_access_token: &str) -> AppResult<MinecraftSession> {
    let xbox_auth: XboxTokenEnvelope = client
        .post(XBOX_AUTH_URL)
        .json(&serde_json::json!({
            "Properties": {
                "AuthMethod": "RPS",
                "SiteName": "user.auth.xboxlive.com",
                "RpsTicket": format!("d={live_access_token}")
            },
            "RelyingParty": "http://auth.xboxlive.com",
            "TokenType": "JWT"
        }))
        .send()?
        .error_for_status()?
        .json()?;

    let user_hash = xbox_auth.user_hash()?;
    let xsts: XboxTokenEnvelope = client
        .post(XSTS_AUTH_URL)
        .json(&serde_json::json!({
            "Properties": {
                "SandboxId": "RETAIL",
                "UserTokens": [xbox_auth.token]
            },
            "RelyingParty": "rp://api.minecraftservices.com/",
            "TokenType": "JWT"
        }))
        .send()?
        .error_for_status()?
        .json()?;

    let minecraft_login_response = client
        .post(MINECRAFT_LOGIN_URL)
        .json(&serde_json::json!({
            "identityToken": format!("XBL3.0 x={user_hash};{}", xsts.token)
        }))
        .send()?;
    if !minecraft_login_response.status().is_success() {
        let status = minecraft_login_response.status();
        let body = minecraft_login_response.text().unwrap_or_default();
        let body_lower = body.to_ascii_lowercase();

        if body_lower.contains("invalid app registration") || body_lower.contains("appreginfo") {
            return Err(AppError::Validation(
                "Microsoft sign-in succeeded, but Minecraft Services rejected this app registration. New launcher app IDs need separate Minecraft/Xbox approval before online sign-in will work."
                    .to_string(),
            ));
        }

        let detail = body.trim();
        return Err(AppError::Validation(if detail.is_empty() {
            format!("minecraft services rejected the Xbox login request with {status}")
        } else {
            format!("minecraft services rejected the Xbox login request with {status}: {detail}")
        }));
    }
    let minecraft_login: MinecraftLoginResponse = minecraft_login_response.json()?;

    let entitlements: EntitlementsResponse = client
        .get(MINECRAFT_ENTITLEMENTS_URL)
        .bearer_auth(&minecraft_login.access_token)
        .send()?
        .error_for_status()?
        .json()?;

    if entitlements.items.is_empty() {
        return Err(AppError::Validation(
            "the signed-in Microsoft account does not appear to own Minecraft Java Edition"
                .to_string(),
        ));
    }

    let profile: MinecraftProfile = client
        .get(MINECRAFT_PROFILE_URL)
        .bearer_auth(&minecraft_login.access_token)
        .send()?
        .error_for_status()?
        .json()?;

    Ok(MinecraftSession {
        minecraft_access_token: minecraft_login.access_token,
        user_hash,
        profile,
    })
}

fn credential_target(account_id: &str) -> String {
    format!("Blocksmith/{account_id}/refresh-token")
}

fn offline_session(
    account_id: Option<String>,
    username: &str,
    uuid: Option<String>,
) -> LaunchAuthSession {
    LaunchAuthSession {
        account_id,
        username: username.trim().to_string(),
        uuid: uuid.unwrap_or_else(|| Uuid::new_v4().to_string()),
        access_token: "0".to_string(),
        user_type: "legacy".to_string(),
        xuid: None,
        online: false,
    }
}

fn random_token(length: usize) -> String {
    thread_rng()
        .sample_iter(&Alphanumeric)
        .take(length)
        .map(char::from)
        .collect()
}

fn pkce_challenge(verifier: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(hasher.finalize())
}

#[derive(Debug, Deserialize)]
struct LiveTokenResponse {
    access_token: String,
    refresh_token: String,
}

#[derive(Debug, Deserialize)]
struct XboxTokenEnvelope {
    #[serde(rename = "Token")]
    token: String,
    #[serde(rename = "DisplayClaims")]
    display_claims: XboxDisplayClaims,
}

impl XboxTokenEnvelope {
    fn user_hash(&self) -> AppResult<String> {
        self.display_claims
            .xui
            .first()
            .map(|claim| claim.user_hash.clone())
            .ok_or_else(|| AppError::Validation("xbox auth response did not include a user hash".to_string()))
    }
}

#[derive(Debug, Deserialize)]
struct XboxDisplayClaims {
    xui: Vec<XboxUserClaim>,
}

#[derive(Debug, Deserialize)]
struct XboxUserClaim {
    #[serde(rename = "uhs")]
    user_hash: String,
}

#[derive(Debug, Deserialize)]
struct MinecraftLoginResponse {
    access_token: String,
}

#[derive(Debug, Deserialize)]
struct EntitlementsResponse {
    #[serde(default)]
    items: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct MinecraftProfile {
    id: String,
    name: String,
    #[serde(default)]
    skins: Vec<MinecraftSkin>,
}

impl MinecraftProfile {
    fn primary_skin_url(&self) -> Option<String> {
        self.skins
            .iter()
            .find(|skin| skin.state.eq_ignore_ascii_case("ACTIVE"))
            .or_else(|| self.skins.first())
            .map(|skin| skin.url.clone())
    }
}

#[derive(Debug, Deserialize)]
struct MinecraftSkin {
    state: String,
    url: String,
}

struct MinecraftSession {
    minecraft_access_token: String,
    user_hash: String,
    profile: MinecraftProfile,
}

#[cfg(test)]
mod tests {
    use std::{env, fs};

    use chrono::Utc;
    use rusqlite::params;
    use uuid::Uuid;

    use super::{launcher_unlocked, start_loopback_listener};
    use crate::{paths::AppPaths, state::AppState};

    struct TestRoot {
        root: std::path::PathBuf,
    }

    impl TestRoot {
        fn new(prefix: &str) -> Self {
            let root = env::temp_dir()
                .join("blocksmith-auth-tests")
                .join(format!("{prefix}-{}", Uuid::new_v4().simple()));
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
    fn loopback_listener_uses_localhost_redirect_uri() {
        let (listeners, redirect_uri) = start_loopback_listener().expect("listener should bind");

        assert!(!listeners.is_empty());
        assert!(redirect_uri.starts_with("http://localhost:"));
        assert!(redirect_uri.ends_with("/callback"));
    }

    #[test]
    fn launcher_unlock_requires_a_verified_microsoft_owner() {
        let test_root = TestRoot::new("unlock");
        let paths = test_root.paths();
        paths.ensure_layout().expect("should create isolated app layout");
        let state = AppState::bootstrap(paths).expect("should bootstrap isolated state");
        let connection = state.db().expect("should open database");
        let now = Utc::now().to_rfc3339();

        connection
            .execute(
                "
                INSERT INTO accounts (
                  id, username, uuid, provider, avatar_url, current_skin_id,
                  owns_minecraft, ownership_verified_at, created_at, updated_at
                )
                VALUES (?1, ?2, ?3, ?4, NULL, NULL, ?5, ?6, ?7, ?8)
                ",
                params![
                    "manual-account",
                    "Offline",
                    "uuid-manual",
                    "manual",
                    0,
                    Option::<String>::None,
                    now,
                    now
                ],
            )
            .expect("should insert local account");
        drop(connection);

        assert!(!launcher_unlocked(&state).expect("query should succeed"));

        let connection = state.db().expect("should reopen database");
        connection
            .execute(
                "
                INSERT INTO accounts (
                  id, username, uuid, provider, avatar_url, current_skin_id,
                  owns_minecraft, ownership_verified_at, created_at, updated_at
                )
                VALUES (?1, ?2, ?3, 'microsoft', NULL, NULL, 1, ?4, ?5, ?6)
                ",
                params![
                    "msa-account",
                    "Taylor",
                    "uuid-msa",
                    now,
                    now,
                    now
                ],
            )
            .expect("should insert verified microsoft account");
        drop(connection);

        assert!(launcher_unlocked(&state).expect("query should succeed"));
    }
}
