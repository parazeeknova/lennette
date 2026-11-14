use lennette_sync_client::Client;
use serde::Deserialize;
use serde_json::Value;

pub const PROVIDER: &str = "lennette-managed";
pub const DEFAULT_MODEL: &str = "gemini-3.5-flash-lite";
const BOOTSTRAP_MODELS: &[&str] = &[
    DEFAULT_MODEL,
    "gemini-3.8-flash",
    "gemini-4-hydrogen",
    "gemini-4-argon",
    "gemini-4-carbon",
    "deepseek-ai/DeepSeek-V4.1-Flash-1017",
];
// Plan eligibility changes more often than provider model IDs. Keep this short
// and session-scoped; explicit refresh always rechecks the current entitlement.
const CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(5 * 60);
struct CachedCatalog {
    owner: Client,
    models: Vec<Model>,
    fetched_at: std::time::Instant,
}
impl CachedCatalog {
    fn valid_for(&self, client: &Client) -> bool {
        self.owner.same_session(client) && self.fetched_at.elapsed() < CACHE_TTL
    }
}
static CATALOG: std::sync::Mutex<Option<CachedCatalog>> = std::sync::Mutex::new(None);

pub(crate) fn cached_models(client: &Client) -> Option<Vec<Model>> {
    CATALOG
        .lock()
        .unwrap()
        .as_ref()
        .filter(|cache| cache.valid_for(client))
        .map(|cache| cache.models.clone())
}

pub(crate) fn has_catalog(client: &Client) -> bool {
    CATALOG
        .lock()
        .unwrap()
        .as_ref()
        .is_some_and(|cache| cache.valid_for(client))
}
pub(crate) fn selected_model(client: &Client, saved: Option<&str>) -> String {
    let cache = CATALOG.lock().unwrap();
    let valid = saved.filter(|id| {
        match cache
            .as_ref()
            .filter(|cache| cache.owner.same_session(client))
        {
            Some(cache) => cache.models.iter().any(|m| m.id == *id),
            None => BOOTSTRAP_MODELS.contains(id),
        }
    });
    valid.unwrap_or(DEFAULT_MODEL).to_owned()
}

#[derive(Clone, Deserialize)]
pub struct Model {
    pub id: String,
    pub name: String,
    pub multiplier: u32,
    pub plus_only: bool,
}

pub async fn models(client: &Client) -> Result<Vec<Model>, String> {
    if let Some(models) = cached_models(client) {
        return Ok(models);
    }
    refresh_models(client).await
}

pub async fn refresh_models(client: &Client) -> Result<Vec<Model>, String> {
    let catalog = client.request(reqwest::Method::GET, "/v1/catalog", &());
    let usage = client.request(reqwest::Method::GET, "/v1/billing/usage", &());
    let (catalog, usage) = tokio::try_join!(catalog, usage).map_err(|e| e.to_string())?;
    let catalog: Value = catalog.json().await.map_err(|e| e.to_string())?;
    let usage: Value = usage.json().await.map_err(|e| e.to_string())?;
    let models = eligible_models(
        catalog,
        usage["limits"]["id"].as_str() == Some("managed_plus"),
    )?;
    *CATALOG.lock().unwrap() = Some(CachedCatalog {
        owner: client.clone(),
        models: models.clone(),
        fetched_at: std::time::Instant::now(),
    });
    Ok(models)
}

/// Public previews never attach an account token or a provider API key.
pub async fn public_models() -> Result<Vec<Model>, String> {
    public_models_from(&super::api_url()).await
}

async fn public_models_from(base: &str) -> Result<Vec<Model>, String> {
    let mut url: reqwest::Url = base.parse().map_err(|_| "Invalid account service URL.")?;
    if url.scheme() != "https" && !(url.scheme() == "http" && url.host_str() == Some("127.0.0.1")) {
        return Err("Account service must use HTTPS or loopback.".into());
    }
    url.set_path("/v1/catalog");
    url.set_query(None);
    url.set_fragment(None);
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())?;
    let response =
        client.get(url).send().await.map_err(
            |_| "Hosted model previews are unavailable. Your local providers still work.",
        )?;
    let response = lennette_sync_client::checked(response)
        .await
        .map_err(|e| e.to_string())?;
    let catalog = response
        .json()
        .await
        .map_err(|_| "Could not read hosted model previews.")?;
    eligible_models(catalog, true)
}

fn eligible_models(catalog: Value, plus: bool) -> Result<Vec<Model>, String> {
    let mut models: Vec<Model> = serde_json::from_value(catalog["models"].clone())
        .map_err(|_| "The account service returned an invalid model catalog.")?;
    models
        .retain(|m| (!m.plus_only || plus) && !m.id.trim().is_empty() && !m.name.trim().is_empty());
    if models.is_empty() {
        return Err("No managed models are currently available. Refresh to retry.".into());
    }
    Ok(models)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn public_previews_need_no_account_or_provider_keys() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            while !bytes.windows(4).any(|v| v == b"\r\n\r\n") {
                let mut chunk = [0u8; 4096];
                let count = socket.read(&mut chunk).await.unwrap();
                assert!(count > 0);
                bytes.extend_from_slice(&chunk[..count]);
            }
            let request = String::from_utf8(bytes).unwrap().to_lowercase();
            assert!(request.starts_with("get /v1/catalog "));
            assert!(!request.contains("authorization:") && !request.contains("cookie:"));
            let body =
                r#"{"models":[{"id":"preview","name":"Preview","multiplier":1,"plus_only":true}]}"#;
            socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).as_bytes()).await.unwrap();
        });
        assert_eq!(public_models_from(&base).await.unwrap()[0].id, "preview");
        server.await.unwrap();
        assert!(public_models_from("http://example.com").await.is_err());
    }

    #[tokio::test]
    async fn loads_catalog_and_tier_through_account_api_without_provider_keys() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let mut paths = Vec::new();
            for _ in 0..4 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                while !bytes.windows(4).any(|v| v == b"\r\n\r\n") {
                    let mut chunk = [0u8; 4096];
                    let read = socket.read(&mut chunk).await.unwrap();
                    assert!(read > 0);
                    bytes.extend_from_slice(&chunk[..read]);
                }
                let request = String::from_utf8(bytes).unwrap();
                assert!(
                    request
                        .to_lowercase()
                        .contains("authorization: bearer test-session-token")
                );
                let path = request.split_whitespace().nth(1).unwrap().to_owned();
                let body = if path == "/v1/catalog" {
                    serde_json::json!({"models":[
                        {"id":"fast","name":"Fast","multiplier":1,"plus_only":false},
                        {"id":"advanced","name":"Advanced","multiplier":5,"plus_only":true}
                    ]})
                } else {
                    assert_eq!(path, "/v1/billing/usage");
                    serde_json::json!({"limits":{"id": if paths.len() < 2 { "managed_plus" } else { "managed" }}})
                }
                .to_string();
                paths.push(path);
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                socket.write_all(response.as_bytes()).await.unwrap();
            }
            paths.sort();
            paths
        });
        let client = Client::restore(lennette_sync_client::SavedSession {
            api_url: base,
            expires_at: lennette_sync_client::now() + 3600,
            tokens: lennette_sync_client::protocol::Tokens {
                access_token: "test-session-token".into(),
                refresh_token: "unused".into(),
                account_id: uuid::Uuid::new_v4(),
                expires_in: 3600,
            },
        })
        .unwrap();
        let initial = tokio::time::timeout(std::time::Duration::from_secs(3), models(&client))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(initial.len(), 2);
        assert_eq!(initial[1].id, "advanced");
        // Reopening does not consume the next network responses. Refresh must
        // recheck plan eligibility and replace the cached Plus-only choices.
        assert_eq!(models(&client).await.unwrap().len(), 2);
        assert_eq!(refresh_models(&client).await.unwrap().len(), 1);
        assert_eq!(models(&client).await.unwrap().len(), 1);
        assert_eq!(
            server.await.unwrap(),
            [
                "/v1/billing/usage",
                "/v1/billing/usage",
                "/v1/catalog",
                "/v1/catalog"
            ]
        );

        let another_session = Client::restore(client.saved().await).unwrap();
        let mut cache = CachedCatalog {
            owner: client.clone(),
            models: initial,
            fetched_at: std::time::Instant::now(),
        };
        assert!(cache.valid_for(&client));
        assert!(!cache.valid_for(&another_session));
        cache.fetched_at -= CACHE_TTL;
        assert!(!cache.valid_for(&client));
    }

    #[test]
    fn catalog_respects_tiers_and_rejects_invalid_responses() {
        let catalog = serde_json::json!({"models":[
            {"id":"fast","name":"Fast","multiplier":1,"plus_only":false},
            {"id":"advanced","name":"Advanced","multiplier":5,"plus_only":true}
        ]});
        assert_eq!(eligible_models(catalog.clone(), false).unwrap().len(), 1);
        assert_eq!(eligible_models(catalog, true).unwrap().len(), 2);
        assert!(eligible_models(serde_json::json!({"models":[]}), false).is_err());
        assert!(eligible_models(serde_json::json!({}), false).is_err());
    }
}
