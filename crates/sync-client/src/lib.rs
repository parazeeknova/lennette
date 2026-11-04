//! Platform-independent managed-account client. Credentials belong in the device vault.
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
pub use lennette_protocol as protocol;
use rand::RngExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::Mutex,
};
use url::Url;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("network request failed: {0}")]
    Network(#[from] reqwest::Error),
    #[error("managed service returned {0}: {1}")]
    Status(u16, String),
    #[error("{0}")]
    Protocol(String),
    #[error("local callback failed: {0}")]
    Io(#[from] std::io::Error),
}
pub type Result<T> = std::result::Result<T, Error>;
#[derive(Clone, Serialize, Deserialize)]
pub struct SavedSession {
    pub tokens: protocol::Tokens,
    pub api_url: String,
    pub expires_at: u64,
}
#[derive(Clone)]
pub struct Client {
    pub http: reqwest::Client,
    base: Url,
    session: Arc<Mutex<SavedSession>>,
}
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn base_url(value: &str) -> Result<Url> {
    let url: Url = value
        .parse()
        .map_err(|_| Error::Protocol("invalid API URL".into()))?;
    if url.scheme() != "https" && !(url.scheme() == "http" && url.host_str() == Some("127.0.0.1")) {
        return Err(Error::Protocol("API URL must use HTTPS or loopback".into()));
    }
    Ok(url)
}
fn http() -> Result<reqwest::Client> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    Ok(reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(180))
        .build()?)
}
impl Client {
    pub fn restore(session: SavedSession) -> Result<Self> {
        Ok(Self {
            base: base_url(&session.api_url)?,
            http: http()?,
            session: Arc::new(Mutex::new(session)),
        })
    }
    pub async fn saved(&self) -> SavedSession {
        self.session.lock().await.clone()
    }
    pub async fn account_id(&self) -> Uuid {
        self.session.lock().await.tokens.account_id
    }
    pub fn url(&self, path: &str) -> Result<Url> {
        self.base
            .join(path)
            .map_err(|_| Error::Protocol("invalid API path".into()))
    }
    pub async fn access_token(&self) -> Result<String> {
        let mut session = self.session.lock().await;
        if session.expires_at <= now() + 60 {
            let response = self
                .http
                .post(self.url("/v1/auth/refresh")?)
                .json(&serde_json::json!({"refresh_token":session.tokens.refresh_token}))
                .send()
                .await?;
            let tokens: protocol::Tokens = checked(response).await?.json().await?;
            if tokens.account_id != session.tokens.account_id {
                return Err(Error::Protocol("account changed during refresh".into()));
            }
            session.expires_at = now() + tokens.expires_in;
            session.tokens = tokens;
        }
        Ok(session.tokens.access_token.clone())
    }
    pub async fn request<T: Serialize + ?Sized>(
        &self,
        method: reqwest::Method,
        path: &str,
        body: &T,
    ) -> Result<reqwest::Response> {
        let token = self.access_token().await?;
        checked(
            self.http
                .request(method, self.url(path)?)
                .bearer_auth(token)
                .json(body)
                .send()
                .await?,
        )
        .await
    }
    pub async fn push(
        &self,
        mutations: &[protocol::Mutation],
    ) -> Result<Vec<protocol::PushResult>> {
        Ok(self
            .request(
                reqwest::Method::POST,
                "/v1/sync/push",
                &serde_json::json!({"mutations":mutations}),
            )
            .await?
            .json()
            .await?)
    }
    pub async fn pull(&self, cursor: i64) -> Result<protocol::PullResponse> {
        Ok(self
            .request(
                reqwest::Method::GET,
                &format!("/v1/sync/pull?after={cursor}"),
                &(),
            )
            .await?
            .json()
            .await?)
    }
    pub async fn logout(&self) -> Result<()> {
        self.request(reqwest::Method::POST, "/v1/auth/logout", &())
            .await?;
        Ok(())
    }
    pub async fn checkout(&self, plan: &str) -> Result<String> {
        let v: Value = self
            .request(
                reqwest::Method::POST,
                "/v1/billing/checkout",
                &serde_json::json!({"plan":plan}),
            )
            .await?
            .json()
            .await?;
        v["checkout_url"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| Error::Protocol("checkout URL missing".into()))
    }
    pub async fn portal(&self) -> Result<String> {
        let v: Value = self
            .request(reqwest::Method::POST, "/v1/billing/portal", &())
            .await?
            .json()
            .await?;
        v["url"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| Error::Protocol("billing portal URL missing".into()))
    }
}
pub async fn checked(response: reqwest::Response) -> Result<reqwest::Response> {
    if response.status().is_success() {
        return Ok(response);
    }
    let status = response.status().as_u16();
    let message = if status == 401 {
        "Please sign in again."
    } else if status == 402 {
        "Subscription or usage limit reached. Existing data is still available."
    } else if status == 409 {
        "Changes conflict with another device. Review the sync conflict before continuing."
    } else {
        "The managed service could not complete this request."
    };
    Err(Error::Status(status, message.into()))
}

pub struct Login {
    listener: TcpListener,
    http: reqwest::Client,
    base: Url,
    verifier: String,
    challenge: String,
    pub authorization_url: String,
}
impl Login {
    pub async fn begin(api_url: &str, provider: &str, device: Uuid) -> Result<Self> {
        let base = base_url(api_url)?;
        let http = http()?;
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let verifier = URL_SAFE_NO_PAD.encode(rand::rng().random::<[u8; 32]>());
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        if !matches!(provider, "google" | "github") {
            return Err(Error::Protocol("unsupported sign-in provider".into()));
        }
        let request = protocol::AuthStart {
            device_id: device,
            device_name: format!("{} desktop", std::env::consts::OS),
            code_challenge: challenge.clone(),
            redirect_uri: format!(
                "http://127.0.0.1:{}/oauth/callback",
                listener.local_addr()?.port()
            ),
        };
        let value: Value = checked(
            http.post(base.join(&format!("/v1/auth/{provider}/start")).unwrap())
                .json(&request)
                .send()
                .await?,
        )
        .await?
        .json()
        .await?;
        let authorization_url = value["authorization_url"]
            .as_str()
            .ok_or_else(|| Error::Protocol("missing authorization URL".into()))?
            .to_owned();
        Ok(Self {
            listener,
            http,
            base,
            verifier,
            challenge,
            authorization_url,
        })
    }
    pub async fn finish(self) -> Result<Client> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(300);
        let ticket = loop {
            let (mut socket, _) = tokio::time::timeout_at(deadline, self.listener.accept())
                .await
                .map_err(|_| Error::Protocol("Sign-in timed out. Try again.".into()))??;
            let mut data = vec![0; 8192];
            let count = tokio::time::timeout(Duration::from_secs(3), socket.read(&mut data))
                .await
                .map_err(|_| Error::Protocol("invalid callback".into()))??;
            let line = std::str::from_utf8(&data[..count])
                .unwrap_or("")
                .lines()
                .next()
                .unwrap_or("");
            let path = line
                .strip_prefix("GET ")
                .and_then(|s| s.strip_suffix(" HTTP/1.1"));
            let callback = path.and_then(|p| Url::parse(&format!("http://127.0.0.1{p}")).ok());
            let valid = callback.filter(|u| u.path() == "/oauth/callback");
            // Validate the nonce before exchanging a ticket; unrelated loopback requests cannot complete login.
            if let Some(url) = valid {
                let pairs: std::collections::HashMap<_, _> =
                    url.query_pairs().into_owned().collect();
                if pairs.get("state") == Some(&self.challenge)
                    && let Some(ticket) = pairs.get("ticket")
                {
                    socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nSigned in. Return to Lennette.").await?;
                    break ticket.clone();
                }
            }
            socket
                .write_all(b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\nInvalid callback")
                .await
                .ok();
        };
        let tokens: protocol::Tokens = checked(
            self.http
                .post(self.base.join("/v1/auth/exchange").unwrap())
                .json(&protocol::AuthExchange {
                    ticket,
                    code_verifier: self.verifier,
                })
                .send()
                .await?,
        )
        .await?
        .json()
        .await?;
        let expires_at = now() + tokens.expires_in;
        Client::restore(SavedSession {
            tokens,
            api_url: self.base.to_string(),
            expires_at,
        })
    }
}
