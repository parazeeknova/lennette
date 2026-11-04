//! Managed account lifecycle. Signing out disables the outbox; local history stays usable.
use crate::{
    core::vault::Vault,
    db::{Database, sync},
};
use lennette_sync_client::{Client, Login, SavedSession};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock, RwLock},
    time::Duration,
};
use uuid::Uuid;

const SESSION_KEY: &str = "managed_session_v1";
mod attachments;
pub use attachments::cache_attachment;
static CLIENT: RwLock<Option<Client>> = RwLock::new(None);
static DATABASE: OnceLock<PathBuf> = OnceLock::new();
static STATUS: Mutex<String> = Mutex::new(String::new());
static MEETING: Mutex<Option<Uuid>> = Mutex::new(None);
static SYNC_LOCK: Mutex<()> = Mutex::new(());
static STARTED: OnceLock<()> = OnceLock::new();
pub fn active() -> Option<Client> {
    CLIENT.read().ok()?.clone()
}
pub fn status() -> String {
    STATUS.lock().unwrap().clone()
}
fn message(value: impl Into<String>) {
    *STATUS.lock().unwrap() = value.into()
}
pub fn api_url() -> String {
    std::env::var("LENNETTE_API_URL")
        .unwrap_or_else(|_| "https://lennapi.itssingularity.com".into())
}
pub fn init(path: Option<&Path>) {
    if let Some(path) = path {
        DATABASE.set(path.to_owned()).ok();
    }
    if let Ok(vault) = Vault::open_default()
        && let Ok(Some(value)) = vault.get(SESSION_KEY)
        && let Ok(saved) = serde_json::from_value::<SavedSession>(value)
    {
        match Client::restore(saved) {
            Ok(client) => {
                *CLIENT.write().unwrap() = Some(client);
                message("Connecting to your account…")
            }
            Err(e) => message(e.to_string()),
        }
    }
    STARTED.get_or_init(|| {
        std::thread::Builder::new()
            .name("lennette-cloud-sync".into())
            .spawn(|| {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("sync runtime");
                let mut failures = 0u32;
                loop {
                    if let (Some(client), Some(path)) = (active(), DATABASE.get()) {
                        let _lock = SYNC_LOCK.lock().unwrap();
                        let result = Database::open(path)
                            .map_err(|e| e.to_string())
                            .and_then(|db| runtime.block_on(sync_and_save(&client, &db)));
                        match result {
                            Ok(()) => failures = 0,
                            Err(error) => {
                                failures = failures.saturating_add(1);
                                message(error);
                            }
                        }
                    } else {
                        failures = 0;
                    }
                    let delay = if failures == 0 {
                        500
                    } else {
                        (1_000u64 << failures.min(5)).min(30_000)
                    };
                    std::thread::sleep(Duration::from_millis(delay));
                }
            })
            .expect("start sync worker");
    });
}
pub fn device_id() -> Result<Uuid, String> {
    let vault = Vault::open_default().map_err(|e| e.to_string())?;
    if let Some(id) = vault
        .get("managed_device_id")
        .map_err(|e| e.to_string())?
        .and_then(|v| v.as_str().and_then(|s| s.parse().ok()))
    {
        return Ok(id);
    }
    let id = Uuid::new_v4();
    vault
        .set("managed_device_id", json!(id))
        .map_err(|e| e.to_string())?;
    Ok(id)
}
pub async fn begin_login(provider: &str) -> Result<Login, String> {
    Login::begin(&api_url(), provider, device_id()?)
        .await
        .map_err(|e| e.to_string())
}
pub async fn complete_login(login: Login, import: bool) -> Result<(), String> {
    let client = login.finish().await.map_err(|e| e.to_string())?;
    let account = client.account_id().await;
    let path = DATABASE
        .get()
        .ok_or("local database is unavailable")?
        .clone();
    tokio::task::spawn_blocking(move || {
        let _lock = SYNC_LOCK.lock().unwrap();
        let db = Database::open(path).map_err(|e| e.to_string())?;
        sync::bind(db.connection(), account, import).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    save_session(&client).await?;
    *CLIENT.write().unwrap() = Some(client);
    message("Signed in. Syncing your account…");
    Ok(())
}
pub async fn sign_out() -> Result<(), String> {
    let client = CLIENT.write().unwrap().take();
    tokio::task::spawn_blocking(|| {
        let _lock = SYNC_LOCK.lock().unwrap();
        if let Some(path) = DATABASE.get() {
            let db = Database::open(path).map_err(|e| e.to_string())?;
            sync::set_meta(db.connection(), "enabled", "0").map_err(|e| e.to_string())?;
        }
        Vault::open_default()
            .and_then(|v| v.remove(&[SESSION_KEY]))
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    message("Local only. Your data remains on this device.");
    if let Some(client) = client {
        client.logout().await.map_err(|e| e.to_string())?;
    }
    Ok(())
}
pub async fn save_session(client: &Client) -> Result<(), String> {
    let saved = serde_json::to_value(client.saved().await).map_err(|e| e.to_string())?;
    let vault = Vault::open_default().map_err(|e| e.to_string())?;
    if vault.get(SESSION_KEY).map_err(|e| e.to_string())?.as_ref() != Some(&saved) {
        vault.set(SESSION_KEY, saved).map_err(|e| e.to_string())?;
    }
    Ok(())
}
async fn sync_cycle(client: &Client, db: &Database) -> Result<(), String> {
    let account = client.account_id().await;
    let connection = db.connection();
    if sync::meta(connection, "enabled")
        .map_err(|e| e.to_string())?
        .as_deref()
        != Some("1")
    {
        return Ok(());
    }
    if sync::meta(connection, "account")
        .map_err(|e| e.to_string())?
        .as_deref()
        != Some(&account.to_string())
    {
        return Err("This history is bound to a different account.".into());
    }
    attachments::upload_pending(client, db).await?;
    // Pull first so conflicts preserve both versions instead of overwriting queued changes.
    loop {
        let page = client
            .pull(sync::cursor(connection).map_err(|e| e.to_string())?)
            .await
            .map_err(|e| e.to_string())?;
        for mut change in page.changes {
            attachments::hydrate(client, db, &mut change).await?;
            sync::apply(connection, &change).map_err(|e| e.to_string())?;
        }
        sync::set_meta(connection, "cursor", &page.cursor.to_string())
            .map_err(|e| e.to_string())?;
        if !page.has_more {
            break;
        }
    }
    let conflicts: i64 = connection
        .query_row("SELECT count(*) FROM cloud_conflicts", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    if conflicts > 0 {
        return Err(format!(
            "{conflicts} sync conflict(s). Choose which version to keep in Account settings."
        ));
    }
    let pending = sync::pending(connection, account).map_err(|e| e.to_string())?;
    if !pending.is_empty() {
        let mut mutations = pending
            .iter()
            .map(|p| p.mutation.clone())
            .collect::<Vec<_>>();
        for mutation in &mut mutations {
            attachments::prepare(db, mutation)?;
        }
        let results = client.push(&mutations).await.map_err(|e| e.to_string())?;
        for result in results {
            let pending = pending
                .iter()
                .find(|p| p.mutation.mutation_id == result.mutation_id)
                .ok_or("unexpected sync acknowledgement")?;
            sync::acknowledge(connection, pending, &result.change).map_err(|e| e.to_string())?;
        }
    }
    message("Signed in · Up to date");
    Ok(())
}
async fn sync_and_save(client: &Client, db: &Database) -> Result<(), String> {
    let result = sync_cycle(client, db).await;
    save_session(client).await?;
    result
}
pub async fn flush() -> Result<(), String> {
    let client = active().ok_or("Sign in to use managed AI.")?;
    let path = DATABASE.get().ok_or("local database unavailable")?.clone();
    let runtime = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let _lock = SYNC_LOCK.lock().unwrap();
        let db = Database::open(path).map_err(|e| e.to_string())?;
        runtime.block_on(sync_and_save(&client, &db))
    })
    .await
    .map_err(|e| e.to_string())?
}
pub fn set_meeting(id: Option<&str>) {
    *MEETING.lock().unwrap() = id.and_then(|s| s.parse().ok());
}
pub fn meeting() -> Option<Uuid> {
    *MEETING.lock().unwrap()
}
pub fn chat_config() -> Option<crate::ai::providers::ProviderConfig> {
    let client = active()?;
    let model = DATABASE
        .get()
        .and_then(|path| Database::open(path).ok())
        .and_then(|db| {
            crate::db::settings::get(db.connection(), "chat_model")
                .ok()
                .flatten()
        })
        .unwrap_or_else(|| "openai/gpt-6-luna".into());
    let mut config =
        crate::ai::providers::ProviderConfig::new("merge-gateway", "managed-session", model);
    config.base_url = client.url("/v1/chat/completions").ok()?.to_string();
    Some(config)
}
pub fn speech_config() -> Option<crate::ai::providers::ProviderConfig> {
    active()?;
    Some(crate::ai::providers::ProviderConfig::new(
        "deepgram-stt",
        "managed-session",
        "nova-3",
    ))
}
pub async fn usage() -> Result<Value, String> {
    let client = active().ok_or("Sign in first.")?;
    client
        .request(reqwest::Method::GET, "/v1/billing/usage", &())
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())
}
pub async fn billing_url(plan: Option<&str>) -> Result<String, String> {
    let client = active().ok_or("Sign in first.")?;
    match plan {
        Some(plan) => client.checkout(plan).await,
        None => client.portal().await,
    }
    .map_err(|e| e.to_string())
}
pub fn resolve_conflicts(keep_local: bool) -> Result<(), String> {
    let _lock = SYNC_LOCK.lock().unwrap();
    let db =
        Database::open(DATABASE.get().ok_or("database unavailable")?).map_err(|e| e.to_string())?;
    sync::resolve_conflicts(db.connection(), keep_local).map_err(|e| e.to_string())
}
