use std::{
    collections::BTreeSet,
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};

use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

const RECORD_SCHEMA_VERSION: u16 = 1;
const WEB_SESSION_LIFETIME_MILLIS: u64 = 30 * 24 * 60 * 60 * 1_000;
const OAUTH_STATE_LIFETIME_MILLIS: u64 = 10 * 60 * 1_000;
const LOGIN_CODE_LIFETIME_MILLIS: u64 = 5 * 60 * 1_000;

#[derive(Clone, Debug)]
pub struct DiscordConfiguration {
    pub client_id: String,
    pub client_secret: String,
    pub website_url: String,
    pub callback_url: String,
    pub token_pepper: String,
    /// Discord user IDs allowed to opt into unfinished website surfaces.
    /// This is server-owned authorization; the browser cannot grant itself
    /// developer access by changing local storage or a request body.
    pub developer_discord_user_ids: BTreeSet<String>,
}

impl DiscordConfiguration {
    pub fn from_environment(website_url: &str) -> Result<Option<Self>, AccountError> {
        let client_id = environment("RLOGS_DISCORD_CLIENT_ID")?;
        let client_secret = environment("RLOGS_DISCORD_CLIENT_SECRET")?;
        let public_api_url = environment("RLOGS_PUBLIC_API_URL")?;
        let callback_url = environment("RLOGS_DISCORD_CALLBACK_URL")?;
        let token_pepper = environment("RLOGS_AUTH_TOKEN_PEPPER")?;
        let developer_discord_user_ids = environment("RLOGS_DEVELOPER_DISCORD_USER_IDS")?
            .map(|value| parse_discord_user_id_set(&value))
            .transpose()?
            .unwrap_or_default();
        let configured = [
            client_id.is_some(),
            client_secret.is_some(),
            public_api_url.is_some(),
            token_pepper.is_some(),
        ];
        if configured.iter().all(|value| !value) && callback_url.is_none() {
            return Ok(None);
        }
        if configured.iter().any(|value| !value) {
            return Err(AccountError::InvalidConfiguration(
                "Discord authentication requires RLOGS_DISCORD_CLIENT_ID, RLOGS_DISCORD_CLIENT_SECRET, RLOGS_PUBLIC_API_URL, and RLOGS_AUTH_TOKEN_PEPPER together".into(),
            ));
        }
        let public_api_url =
            pathless_https_origin(public_api_url.as_deref().unwrap(), "public API")?;
        let website_url = pathless_https_origin(website_url, "website")?;
        let callback_url = match callback_url {
            Some(value) => https_callback_url(&value)?,
            None => format!("{public_api_url}/v1/auth/discord/callback"),
        };
        let token_pepper = token_pepper.unwrap();
        if token_pepper.len() < 32 {
            return Err(AccountError::InvalidConfiguration(
                "RLOGS_AUTH_TOKEN_PEPPER must contain at least 32 characters".into(),
            ));
        }
        Ok(Some(Self {
            client_id: client_id.unwrap(),
            client_secret: client_secret.unwrap(),
            website_url,
            callback_url,
            token_pepper,
            developer_discord_user_ids,
        }))
    }

    fn callback_url(&self) -> &str {
        &self.callback_url
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AccountView {
    pub schema_version: u16,
    pub submitter_id: String,
    pub account_id: u64,
    pub username: String,
    pub discord_username: String,
    pub discord_global_name: Option<String>,
    pub discord_avatar_url: Option<String>,
    /// Account-level consent to publish future server-verified parse reports.
    pub publish_verified_parses: bool,
    /// Server-authorized access to optional development-only site surfaces.
    pub developer: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PublicAccountIdentity {
    pub schema_version: u16,
    pub account_id: u64,
    pub username: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct WebSessionReceipt {
    pub schema_version: u16,
    pub access_token: String,
    pub expires_unix_millis: u64,
    pub account: AccountView,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AppTokenReceipt {
    pub schema_version: u16,
    pub device_token: String,
    pub device_id: String,
    pub created_unix_millis: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceIdentity {
    pub submitter_id: String,
    pub device_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AccountRecord {
    schema_version: u16,
    submitter_id: String,
    #[serde(default)]
    account_id: Option<u64>,
    #[serde(default)]
    username: Option<String>,
    discord_user_id: String,
    discord_username: String,
    discord_global_name: Option<String>,
    discord_avatar_url: Option<String>,
    #[serde(default)]
    publish_verified_parses: bool,
    created_unix_millis: u64,
    updated_unix_millis: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpiringIdentityRecord {
    schema_version: u16,
    submitter_id: String,
    expires_unix_millis: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OAuthStateRecord {
    schema_version: u16,
    expires_unix_millis: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeviceTokenRecord {
    schema_version: u16,
    submitter_id: String,
    device_id: String,
    created_unix_millis: u64,
    revoked_unix_millis: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct DiscordTokenResponse {
    access_token: String,
}

#[derive(Debug, Deserialize)]
struct DiscordUserResponse {
    id: String,
    username: String,
    global_name: Option<String>,
    avatar: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum AccountError {
    #[error("account authentication is not configured")]
    NotConfigured,
    #[error("invalid account-service configuration: {0}")]
    InvalidConfiguration(String),
    #[error("the login state or one-time code is invalid or expired")]
    InvalidOrExpiredCode,
    #[error("the account token is invalid or expired")]
    Unauthorized,
    #[error("username must be 3-24 lowercase letters, numbers, hyphens, or underscores")]
    InvalidUsername,
    #[error("that username is already in use")]
    UsernameUnavailable,
    #[error("Discord authentication is temporarily unavailable")]
    DiscordUnavailable,
    #[error("account storage failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("account JSON failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("account URL failed: {0}")]
    Url(#[from] url::ParseError),
}

#[derive(Debug)]
pub struct AccountStore {
    root: PathBuf,
    configuration: Option<DiscordConfiguration>,
    client: Client,
    writes: Mutex<()>,
}

impl AccountStore {
    pub fn open(
        root: PathBuf,
        configuration: Option<DiscordConfiguration>,
    ) -> Result<Self, AccountError> {
        for relative in [
            "users",
            "discord-index",
            "oauth-states",
            "login-codes",
            "web-sessions",
            "device-tokens",
            "account-id-index",
            "username-index",
        ] {
            std::fs::create_dir_all(root.join(relative))?;
        }
        let client = Client::builder()
            .connect_timeout(std::time::Duration::from_secs(5))
            .timeout(std::time::Duration::from_secs(15))
            .user_agent(concat!("rLogs-auth/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| AccountError::InvalidConfiguration(error.to_string()))?;
        Ok(Self {
            root,
            configuration,
            client,
            writes: Mutex::new(()),
        })
    }

    pub fn configured(&self) -> bool {
        self.configuration.is_some()
    }

    pub fn begin_discord_login(&self, now: u64) -> Result<String, AccountError> {
        let configuration = self.configuration()?;
        let state = random_token("state");
        let state_hash = token_hash("oauth-state", &state, &configuration.token_pepper);
        let record = OAuthStateRecord {
            schema_version: RECORD_SCHEMA_VERSION,
            expires_unix_millis: now.saturating_add(OAUTH_STATE_LIFETIME_MILLIS),
        };
        let _write = self.write_guard();
        write_json_new(
            &self
                .root
                .join("oauth-states")
                .join(format!("{state_hash}.json")),
            &record,
        )?;
        let mut url = Url::parse("https://discord.com/oauth2/authorize")?;
        url.query_pairs_mut()
            .append_pair("client_id", &configuration.client_id)
            .append_pair("redirect_uri", configuration.callback_url())
            .append_pair("response_type", "code")
            .append_pair("scope", "identify")
            .append_pair("state", &state)
            .append_pair("prompt", "consent");
        Ok(url.to_string())
    }

    pub async fn complete_discord_login(
        &self,
        code: &str,
        state: &str,
        now: u64,
    ) -> Result<String, AccountError> {
        let login_code = self.complete_discord_login_code(code, state, now).await?;
        let configuration = self.configuration()?;
        let mut return_url = Url::parse(&configuration.website_url)?;
        return_url
            .query_pairs_mut()
            .append_pair("auth_code", &login_code);
        return_url.set_fragment(Some("account"));
        Ok(return_url.to_string())
    }

    pub async fn complete_discord_login_code(
        &self,
        code: &str,
        state: &str,
        now: u64,
    ) -> Result<String, AccountError> {
        let configuration = self.configuration()?;
        if code.trim().is_empty() || state.trim().is_empty() {
            return Err(AccountError::InvalidOrExpiredCode);
        }
        let state_hash = token_hash("oauth-state", state, &configuration.token_pepper);
        let state_path = self
            .root
            .join("oauth-states")
            .join(format!("{state_hash}.json"));
        let record: OAuthStateRecord =
            read_json(&state_path)?.ok_or(AccountError::InvalidOrExpiredCode)?;
        if record.schema_version != RECORD_SCHEMA_VERSION || record.expires_unix_millis < now {
            let _ = std::fs::remove_file(state_path);
            return Err(AccountError::InvalidOrExpiredCode);
        }
        std::fs::remove_file(state_path)?;

        let token = self
            .client
            .post("https://discord.com/api/oauth2/token")
            .form(&[
                ("client_id", configuration.client_id.as_str()),
                ("client_secret", configuration.client_secret.as_str()),
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", configuration.callback_url()),
            ])
            .send()
            .await
            .map_err(|_| AccountError::DiscordUnavailable)?
            .error_for_status()
            .map_err(|_| AccountError::DiscordUnavailable)?
            .json::<DiscordTokenResponse>()
            .await
            .map_err(|_| AccountError::DiscordUnavailable)?;
        let discord = self
            .client
            .get("https://discord.com/api/users/@me")
            .bearer_auth(token.access_token)
            .send()
            .await
            .map_err(|_| AccountError::DiscordUnavailable)?
            .error_for_status()
            .map_err(|_| AccountError::DiscordUnavailable)?
            .json::<DiscordUserResponse>()
            .await
            .map_err(|_| AccountError::DiscordUnavailable)?;
        let account = self.upsert_discord_user(discord, now)?;
        let login_code = random_token("login");
        let login_hash = token_hash("login-code", &login_code, &configuration.token_pepper);
        write_json_new(
            &self
                .root
                .join("login-codes")
                .join(format!("{login_hash}.json")),
            &ExpiringIdentityRecord {
                schema_version: RECORD_SCHEMA_VERSION,
                submitter_id: account.submitter_id,
                expires_unix_millis: now.saturating_add(LOGIN_CODE_LIFETIME_MILLIS),
            },
        )?;
        Ok(login_code)
    }

    pub fn exchange_login_code(
        &self,
        code: &str,
        now: u64,
    ) -> Result<WebSessionReceipt, AccountError> {
        let configuration = self.configuration()?;
        let code_hash = token_hash("login-code", code, &configuration.token_pepper);
        let path = self
            .root
            .join("login-codes")
            .join(format!("{code_hash}.json"));
        let record: ExpiringIdentityRecord =
            read_json(&path)?.ok_or(AccountError::InvalidOrExpiredCode)?;
        std::fs::remove_file(path)?;
        if record.schema_version != RECORD_SCHEMA_VERSION || record.expires_unix_millis < now {
            return Err(AccountError::InvalidOrExpiredCode);
        }
        let access_token = random_token("rlw");
        let expires_unix_millis = now.saturating_add(WEB_SESSION_LIFETIME_MILLIS);
        let session_hash = token_hash("web-session", &access_token, &configuration.token_pepper);
        write_json_new(
            &self
                .root
                .join("web-sessions")
                .join(format!("{session_hash}.json")),
            &ExpiringIdentityRecord {
                schema_version: RECORD_SCHEMA_VERSION,
                submitter_id: record.submitter_id.clone(),
                expires_unix_millis,
            },
        )?;
        Ok(WebSessionReceipt {
            schema_version: 1,
            access_token,
            expires_unix_millis,
            account: self.account(&record.submitter_id)?,
        })
    }

    pub fn authenticate_web(&self, token: &str, now: u64) -> Result<AccountView, AccountError> {
        let configuration = self.configuration()?;
        let hash = token_hash("web-session", token, &configuration.token_pepper);
        let path = self.root.join("web-sessions").join(format!("{hash}.json"));
        let record: ExpiringIdentityRecord = read_json(&path)?.ok_or(AccountError::Unauthorized)?;
        if record.schema_version != RECORD_SCHEMA_VERSION || record.expires_unix_millis < now {
            let _ = std::fs::remove_file(path);
            return Err(AccountError::Unauthorized);
        }
        self.account(&record.submitter_id)
    }

    pub fn issue_device_token(
        &self,
        web_token: &str,
        now: u64,
    ) -> Result<AppTokenReceipt, AccountError> {
        let configuration = self.configuration()?;
        let account = self.authenticate_web(web_token, now)?;
        let device_token = random_token("rld");
        let device_id = format!("dev_{}", Uuid::new_v4().simple());
        let hash = token_hash("device-token", &device_token, &configuration.token_pepper);
        write_json_new(
            &self.root.join("device-tokens").join(format!("{hash}.json")),
            &DeviceTokenRecord {
                schema_version: RECORD_SCHEMA_VERSION,
                submitter_id: account.submitter_id,
                device_id: device_id.clone(),
                created_unix_millis: now,
                revoked_unix_millis: None,
            },
        )?;
        Ok(AppTokenReceipt {
            schema_version: 1,
            device_token,
            device_id,
            created_unix_millis: now,
        })
    }

    pub fn authenticate_device(&self, token: &str) -> Result<DeviceIdentity, AccountError> {
        let configuration = self.configuration()?;
        let hash = token_hash("device-token", token, &configuration.token_pepper);
        let record: DeviceTokenRecord =
            read_json(&self.root.join("device-tokens").join(format!("{hash}.json")))?
                .ok_or(AccountError::Unauthorized)?;
        if record.schema_version != RECORD_SCHEMA_VERSION || record.revoked_unix_millis.is_some() {
            return Err(AccountError::Unauthorized);
        }
        Ok(DeviceIdentity {
            submitter_id: record.submitter_id,
            device_id: record.device_id,
        })
    }

    pub fn update_username(
        &self,
        web_token: &str,
        requested_username: &str,
        now: u64,
    ) -> Result<AccountView, AccountError> {
        let account = self.authenticate_web(web_token, now)?;
        let username = validate_username(requested_username)?;
        let _write = self.write_guard();
        let path = self
            .root
            .join("users")
            .join(format!("{}.json", account.submitter_id));
        let mut record: AccountRecord = read_json(&path)?.ok_or(AccountError::Unauthorized)?;
        if record.username.as_deref() == Some(username.as_str()) {
            return self.account_view(record);
        }
        let index_path = self
            .root
            .join("username-index")
            .join(format!("{username}.json"));
        let indexed_submitter: Option<String> = read_json(&index_path)?;
        if indexed_submitter
            .as_deref()
            .is_some_and(|value| value != record.submitter_id)
        {
            return Err(AccountError::UsernameUnavailable);
        }
        if indexed_submitter.is_none() {
            write_json_new(&index_path, &record.submitter_id)?;
        }
        let previous = record.username.replace(username.clone());
        record.updated_unix_millis = now;
        write_json_atomic(&path, &record)?;
        if let Some(previous) = previous.filter(|value| value != &username) {
            let previous_path = self
                .root
                .join("username-index")
                .join(format!("{previous}.json"));
            let previous_owner: Option<String> = read_json(&previous_path)?;
            if previous_owner.as_deref() == Some(record.submitter_id.as_str()) {
                std::fs::remove_file(previous_path)?;
            }
        }
        self.account_view(record)
    }

    pub fn update_publish_verified_parses(
        &self,
        web_token: &str,
        publish_verified_parses: bool,
        now: u64,
    ) -> Result<AccountView, AccountError> {
        let account = self.authenticate_web(web_token, now)?;
        let _write = self.write_guard();
        let path = self
            .root
            .join("users")
            .join(format!("{}.json", account.submitter_id));
        let mut record: AccountRecord = read_json(&path)?.ok_or(AccountError::Unauthorized)?;
        record.publish_verified_parses = publish_verified_parses;
        record.updated_unix_millis = now;
        write_json_atomic(&path, &record)?;
        self.account_view(record)
    }

    pub fn publishes_verified_parses(&self, submitter_id: &str) -> Result<bool, AccountError> {
        Ok(self.account(submitter_id)?.publish_verified_parses)
    }

    pub fn public_identity(
        &self,
        account_id: u64,
    ) -> Result<Option<(String, PublicAccountIdentity)>, AccountError> {
        if !(100_000_000_000..=999_999_999_999).contains(&account_id) {
            return Ok(None);
        }
        let submitter_id: Option<String> = read_json(
            &self
                .root
                .join("account-id-index")
                .join(format!("{account_id}.json")),
        )?;
        let Some(submitter_id) = submitter_id else {
            return Ok(None);
        };
        let account = self.account(&submitter_id)?;
        Ok(Some((
            submitter_id,
            PublicAccountIdentity {
                schema_version: 1,
                account_id: account.account_id,
                username: account.username,
            },
        )))
    }

    fn upsert_discord_user(
        &self,
        discord: DiscordUserResponse,
        now: u64,
    ) -> Result<AccountRecord, AccountError> {
        let configuration = self.configuration()?;
        if discord.id.is_empty() || discord.username.is_empty() {
            return Err(AccountError::DiscordUnavailable);
        }
        let discord_hash = token_hash("discord-user", &discord.id, &configuration.token_pepper);
        let index_path = self
            .root
            .join("discord-index")
            .join(format!("{discord_hash}.json"));
        let existing_submitter: Option<String> = read_json(&index_path)?;
        let submitter_id = existing_submitter.unwrap_or_else(|| {
            let digest = token_hash("submitter", &discord.id, &configuration.token_pepper);
            format!("usr_{}", &digest[..32])
        });
        let user_path = self.root.join("users").join(format!("{submitter_id}.json"));
        let existing: Option<AccountRecord> = read_json(&user_path)?;
        let avatar_url = discord.avatar.as_ref().map(|avatar| {
            format!(
                "https://cdn.discordapp.com/avatars/{}/{}.png",
                discord.id, avatar
            )
        });
        let record = AccountRecord {
            schema_version: RECORD_SCHEMA_VERSION,
            submitter_id: submitter_id.clone(),
            account_id: existing.as_ref().and_then(|value| value.account_id),
            username: existing.as_ref().and_then(|value| value.username.clone()),
            discord_user_id: discord.id,
            discord_username: discord.username,
            discord_global_name: discord.global_name,
            discord_avatar_url: avatar_url,
            publish_verified_parses: existing
                .as_ref()
                .is_some_and(|value| value.publish_verified_parses),
            created_unix_millis: existing
                .as_ref()
                .map_or(now, |value| value.created_unix_millis),
            updated_unix_millis: now,
        };
        let _write = self.write_guard();
        if !index_path.exists() {
            write_json_new(&index_path, &submitter_id)?;
        }
        write_json_atomic(&user_path, &record)?;
        drop(_write);
        self.ensure_public_identity(&submitter_id)
    }

    fn account(&self, submitter_id: &str) -> Result<AccountView, AccountError> {
        self.account_view(self.ensure_public_identity(submitter_id)?)
    }

    fn account_view(&self, record: AccountRecord) -> Result<AccountView, AccountError> {
        let developer = self.configuration.as_ref().is_some_and(|configuration| {
            configuration
                .developer_discord_user_ids
                .contains(&record.discord_user_id)
        });
        account_view(record, developer)
    }

    fn ensure_public_identity(&self, submitter_id: &str) -> Result<AccountRecord, AccountError> {
        let _write = self.write_guard();
        let path = self.root.join("users").join(format!("{submitter_id}.json"));
        let mut record: AccountRecord = read_json(&path)?.ok_or(AccountError::Unauthorized)?;
        let mut changed = false;
        if record.account_id.is_none() {
            record.account_id = Some(self.allocate_account_id(submitter_id)?);
            changed = true;
        }
        if record.username.is_none() {
            record.username = Some(self.allocate_username(
                &record.discord_username,
                submitter_id,
                record.account_id.unwrap(),
            )?);
            changed = true;
        }
        let account_id = record.account_id.unwrap();
        let account_index = self
            .root
            .join("account-id-index")
            .join(format!("{account_id}.json"));
        if !account_index.exists() {
            write_json_new(&account_index, &record.submitter_id)?;
        }
        let username = record.username.as_deref().unwrap();
        let username_index = self
            .root
            .join("username-index")
            .join(format!("{username}.json"));
        if !username_index.exists() {
            write_json_new(&username_index, &record.submitter_id)?;
        }
        if changed {
            write_json_atomic(&path, &record)?;
        }
        Ok(record)
    }

    fn allocate_account_id(&self, submitter_id: &str) -> Result<u64, AccountError> {
        for _ in 0..128 {
            let candidate = random_public_account_id();
            let existing: Option<String> = read_json(
                &self
                    .root
                    .join("account-id-index")
                    .join(format!("{candidate}.json")),
            )?;
            if existing
                .as_deref()
                .is_none_or(|value| value == submitter_id)
            {
                return Ok(candidate);
            }
        }
        Err(AccountError::InvalidConfiguration(
            "could not allocate a unique public account ID".into(),
        ))
    }

    fn allocate_username(
        &self,
        discord_username: &str,
        submitter_id: &str,
        account_id: u64,
    ) -> Result<String, AccountError> {
        let base = default_username(discord_username, account_id);
        for candidate in [
            base.clone(),
            format!(
                "{}-{}",
                truncate_username(&base, 17),
                account_id % 1_000_000
            ),
        ] {
            let existing: Option<String> = read_json(
                &self
                    .root
                    .join("username-index")
                    .join(format!("{candidate}.json")),
            )?;
            if existing
                .as_deref()
                .is_none_or(|value| value == submitter_id)
            {
                return Ok(candidate);
            }
        }
        Err(AccountError::UsernameUnavailable)
    }

    fn configuration(&self) -> Result<&DiscordConfiguration, AccountError> {
        self.configuration
            .as_ref()
            .ok_or(AccountError::NotConfigured)
    }

    fn write_guard(&self) -> std::sync::MutexGuard<'_, ()> {
        self.writes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn environment(name: &str) -> Result<Option<String>, AccountError> {
    match std::env::var(name) {
        Ok(value) if !value.trim().is_empty() => Ok(Some(value.trim().to_owned())),
        Ok(_) | Err(std::env::VarError::NotPresent) => Ok(None),
        Err(error) => Err(AccountError::InvalidConfiguration(format!(
            "could not read {name}: {error}"
        ))),
    }
}

fn pathless_https_origin(value: &str, label: &str) -> Result<String, AccountError> {
    let url = Url::parse(value.trim())?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(AccountError::InvalidConfiguration(format!(
            "{label} URL must be a pathless HTTPS origin"
        )));
    }
    Ok(url.origin().ascii_serialization())
}

fn https_callback_url(value: &str) -> Result<String, AccountError> {
    let mut url = Url::parse(value.trim())?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(AccountError::InvalidConfiguration(
            "Discord callback URL must be HTTPS without credentials, a query, or a fragment".into(),
        ));
    }
    if url.path().is_empty() {
        url.set_path("/");
    }
    Ok(url.to_string())
}

fn random_token(prefix: &str) -> String {
    format!(
        "{prefix}_{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    )
}

fn random_public_account_id() -> u64 {
    const FIRST: u64 = 100_000_000_000;
    const COUNT: u128 = 900_000_000_000;
    FIRST + (Uuid::new_v4().as_u128() % COUNT) as u64
}

fn validate_username(value: &str) -> Result<String, AccountError> {
    let username = value.trim().to_ascii_lowercase();
    if !(3..=24).contains(&username.len())
        || !username.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_'
        })
        || !username
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        || !username
            .bytes()
            .next_back()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
    {
        return Err(AccountError::InvalidUsername);
    }
    Ok(username)
}

fn default_username(discord_username: &str, account_id: u64) -> String {
    let mut value = String::new();
    for character in discord_username.trim().to_ascii_lowercase().chars() {
        let next = if character.is_ascii_lowercase()
            || character.is_ascii_digit()
            || matches!(character, '-' | '_')
        {
            character
        } else {
            '-'
        };
        if !(matches!(next, '-' | '_') && (value.ends_with('-') || value.ends_with('_'))) {
            value.push(next);
        }
    }
    value = truncate_username(
        value.trim_matches(|character| matches!(character, '-' | '_')),
        24,
    );
    if validate_username(&value).is_ok() {
        value
    } else {
        format!("user-{:06}", account_id % 1_000_000)
    }
}

fn truncate_username(value: &str, maximum: usize) -> String {
    value
        .chars()
        .take(maximum)
        .collect::<String>()
        .trim_end_matches(['-', '_'])
        .to_owned()
}

fn account_view(record: AccountRecord, developer: bool) -> Result<AccountView, AccountError> {
    let account_id = record.account_id.ok_or_else(|| {
        AccountError::InvalidConfiguration("account is missing its public account ID".into())
    })?;
    let username = record.username.ok_or_else(|| {
        AccountError::InvalidConfiguration("account is missing its public username".into())
    })?;
    Ok(AccountView {
        schema_version: 1,
        submitter_id: record.submitter_id,
        account_id,
        username,
        discord_username: record.discord_username,
        discord_global_name: record.discord_global_name,
        discord_avatar_url: record.discord_avatar_url,
        publish_verified_parses: record.publish_verified_parses,
        developer,
    })
}

fn parse_discord_user_id_set(value: &str) -> Result<BTreeSet<String>, AccountError> {
    let mut ids = BTreeSet::new();
    for raw in value.split(',') {
        let id = raw.trim();
        if id.is_empty() {
            continue;
        }
        if id.len() > 24 || !id.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(AccountError::InvalidConfiguration(
                "RLOGS_DEVELOPER_DISCORD_USER_IDS must be a comma-separated list of numeric Discord user IDs"
                    .into(),
            ));
        }
        ids.insert(id.to_owned());
    }
    Ok(ids)
}

fn token_hash(domain: &str, token: &str, pepper: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"rlogs-auth-v1\0");
    hasher.update(domain.as_bytes());
    hasher.update(b"\0");
    hasher.update(pepper.as_bytes());
    hasher.update(b"\0");
    hasher.update(token.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<Option<T>, AccountError> {
    match File::open(path) {
        Ok(mut file) => {
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)?;
            Ok(Some(serde_json::from_slice(&bytes)?))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn write_json_new(path: &Path, value: &impl Serialize) -> Result<(), AccountError> {
    let bytes = serde_json::to_vec_pretty(value)?;
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    Ok(())
}

fn write_json_atomic(path: &Path, value: &impl Serialize) -> Result<(), AccountError> {
    let bytes = serde_json::to_vec_pretty(value)?;
    let partial = path.with_extension(format!("partial-{}", Uuid::new_v4().simple()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    drop(file);
    if path.exists() {
        #[cfg(windows)]
        std::fs::remove_file(path)?;
    }
    std::fs::rename(partial, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn configuration() -> DiscordConfiguration {
        DiscordConfiguration {
            client_id: "client".into(),
            client_secret: "secret".into(),
            website_url: "https://site.example.test".into(),
            callback_url: "https://site.example.test/account/".into(),
            token_pepper: "0123456789abcdef0123456789abcdef".into(),
            developer_discord_user_ids: BTreeSet::new(),
        }
    }

    #[test]
    fn device_token_round_trip_keeps_plaintext_out_of_storage() {
        let root = tempfile::tempdir().unwrap();
        let store = AccountStore::open(root.path().into(), Some(configuration())).unwrap();
        let account = store
            .upsert_discord_user(
                DiscordUserResponse {
                    id: "discord-1".into(),
                    username: "tester".into(),
                    global_name: Some("Tester".into()),
                    avatar: None,
                },
                10,
            )
            .unwrap();
        let web_token = random_token("rlw");
        let web_hash = token_hash(
            "web-session",
            &web_token,
            &store.configuration().unwrap().token_pepper,
        );
        write_json_new(
            &root
                .path()
                .join("web-sessions")
                .join(format!("{web_hash}.json")),
            &ExpiringIdentityRecord {
                schema_version: 1,
                submitter_id: account.submitter_id.clone(),
                expires_unix_millis: 1_000,
            },
        )
        .unwrap();
        let issued = store.issue_device_token(&web_token, 20).unwrap();
        let identity = store.authenticate_device(&issued.device_token).unwrap();
        assert_eq!(identity.submitter_id, account.submitter_id);
        let storage = std::fs::read_to_string(
            std::fs::read_dir(root.path().join("device-tokens"))
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .path(),
        )
        .unwrap();
        assert!(!storage.contains(&issued.device_token));
    }

    #[test]
    fn oauth_state_is_single_use_and_bounded() {
        let root = tempfile::tempdir().unwrap();
        let store = AccountStore::open(root.path().into(), Some(configuration())).unwrap();
        let url = Url::parse(&store.begin_discord_login(100).unwrap()).unwrap();
        assert_eq!(url.domain(), Some("discord.com"));
        assert_eq!(
            std::fs::read_dir(root.path().join("oauth-states"))
                .unwrap()
                .count(),
            1
        );
    }

    #[test]
    fn developer_access_is_server_authorized_by_discord_user_id() {
        let root = tempfile::tempdir().unwrap();
        let mut configured = configuration();
        configured
            .developer_discord_user_ids
            .insert("123456789".into());
        let store = AccountStore::open(root.path().into(), Some(configured)).unwrap();
        let record = store
            .upsert_discord_user(
                DiscordUserResponse {
                    id: "123456789".into(),
                    username: "developer".into(),
                    global_name: None,
                    avatar: None,
                },
                10,
            )
            .unwrap();
        assert!(store.account_view(record).unwrap().developer);
        assert_eq!(
            parse_discord_user_id_set("123, 456,123").unwrap(),
            BTreeSet::from(["123".into(), "456".into()])
        );
        assert!(parse_discord_user_id_set("123,user").is_err());
    }

    #[test]
    fn public_account_identity_is_stable_and_username_is_editable() {
        let root = tempfile::tempdir().unwrap();
        let store = AccountStore::open(root.path().into(), Some(configuration())).unwrap();
        let record = store
            .upsert_discord_user(
                DiscordUserResponse {
                    id: "discord-public-identity".into(),
                    username: "Initial.Name".into(),
                    global_name: Some("Initial Name".into()),
                    avatar: None,
                },
                10,
            )
            .unwrap();
        let original = account_view(record, false).unwrap();
        assert!((100_000_000_000..=999_999_999_999).contains(&original.account_id));
        assert_eq!(original.username, "initial-name");
        assert!(!original.developer);

        let public = store.public_identity(original.account_id).unwrap().unwrap();
        assert_eq!(public.0, original.submitter_id);
        assert_eq!(public.1.username, "initial-name");

        let web_token = random_token("rlw");
        let web_hash = token_hash(
            "web-session",
            &web_token,
            &store.configuration().unwrap().token_pepper,
        );
        write_json_new(
            &root
                .path()
                .join("web-sessions")
                .join(format!("{web_hash}.json")),
            &ExpiringIdentityRecord {
                schema_version: 1,
                submitter_id: original.submitter_id,
                expires_unix_millis: 1_000,
            },
        )
        .unwrap();
        let updated = store.update_username(&web_token, "Marie_Rose", 20).unwrap();
        assert_eq!(updated.account_id, original.account_id);
        assert_eq!(updated.username, "marie_rose");
        assert_eq!(
            store
                .public_identity(original.account_id)
                .unwrap()
                .unwrap()
                .1
                .username,
            "marie_rose"
        );
    }

    #[test]
    fn public_username_validation_rejects_unsafe_routes() {
        assert_eq!(validate_username("Marie-Rose").unwrap(), "marie-rose");
        assert!(validate_username("../private").is_err());
        assert!(validate_username("-leading").is_err());
        assert!(validate_username("ab").is_err());
    }

    #[test]
    fn verified_parse_publication_is_explicit_and_persisted() {
        let root = tempfile::tempdir().unwrap();
        let store = AccountStore::open(root.path().into(), Some(configuration())).unwrap();
        let account = store
            .upsert_discord_user(
                DiscordUserResponse {
                    id: "discord-public-parses".into(),
                    username: "parser".into(),
                    global_name: None,
                    avatar: None,
                },
                10,
            )
            .unwrap();
        assert!(!account.publish_verified_parses);
        let web_token = random_token("rlw");
        let web_hash = token_hash(
            "web-session",
            &web_token,
            &store.configuration().unwrap().token_pepper,
        );
        write_json_new(
            &root
                .path()
                .join("web-sessions")
                .join(format!("{web_hash}.json")),
            &ExpiringIdentityRecord {
                schema_version: 1,
                submitter_id: account.submitter_id.clone(),
                expires_unix_millis: 1_000,
            },
        )
        .unwrap();

        let updated = store
            .update_publish_verified_parses(&web_token, true, 20)
            .unwrap();
        assert!(updated.publish_verified_parses);
        assert!(
            store
                .publishes_verified_parses(&account.submitter_id)
                .unwrap()
        );
    }

    #[test]
    fn all_or_none_environment_configuration_is_required() {
        assert!(pathless_https_origin("https://api.example.test", "api").is_ok());
        assert!(pathless_https_origin("http://api.example.test", "api").is_err());
        assert!(pathless_https_origin("https://api.example.test/path", "api").is_err());
        assert_eq!(
            https_callback_url("https://site.example.test/account/").unwrap(),
            "https://site.example.test/account/"
        );
        assert!(https_callback_url("http://site.example.test/account/").is_err());
        assert!(https_callback_url("https://site.example.test/account/?code=bad").is_err());
    }
}
