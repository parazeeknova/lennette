//! Platform-independent managed-account client. Credentials belong in the device vault.
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
pub use lennette_protocol as protocol;
use rand::RngExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
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
    #[error("This device was signed out. Your local history is still available.")]
    SessionRevoked,
    #[error("{0}")]
    Protocol(String),
    #[error("local callback failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("could not save refreshed login: {0}")]
    Persistence(String),
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
    revoked: Arc<AtomicBool>,
    persist: Option<Arc<SessionStore>>,
    dirty: Arc<AtomicBool>,
}
type SessionStore = dyn Fn(&SavedSession) -> Result<()> + Send + Sync;
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
            revoked: Arc::new(AtomicBool::new(false)),
            persist: None,
            dirty: Arc::new(AtomicBool::new(false)),
        })
    }
    pub fn is_revoked(&self) -> bool {
        self.revoked.load(Ordering::Acquire)
    }
    pub fn mark_revoked(&self) {
        self.revoked.store(true, Ordering::Release);
    }
    /// Install the device's credential store before sharing this login.
    pub fn with_persistence(
        mut self,
        persist: impl Fn(&SavedSession) -> Result<()> + Send + Sync + 'static,
    ) -> Self {
        self.persist = Some(Arc::new(persist));
        self
    }

    fn persist_locked(&self, session: &SavedSession) -> Result<()> {
        if let Some(persist) = &self.persist {
            persist(session)?;
        }
        self.dirty.store(false, Ordering::Release);
        Ok(())
    }

    pub async fn persist(&self) -> Result<()> {
        let session = self.session.lock().await;
        self.persist_locked(&session)
    }
    async fn checked(&self, response: reqwest::Response) -> Result<reqwest::Response> {
        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            self.mark_revoked();
        }
        checked(response).await
    }
    /// Whether two handles belong to the same login, even after token rotation.
    pub fn same_session(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.session, &other.session)
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
        self.token(None).await
    }

    /// Recover once from a token rejected after another request rotated it.
    /// Only a rejected refresh token permanently signs out the login.
    pub async fn recover_access_token(&self, rejected: &str) -> Result<String> {
        self.token(Some(rejected)).await
    }

    async fn token(&self, rejected: Option<&str>) -> Result<String> {
        // Dropping an AI/speech request must not drop refresh after the server
        // rotates its credentials. A detached task completes and persists that
        // rotation even when its original caller stops waiting.
        let client = self.clone();
        let rejected = rejected.map(str::to_owned);
        tokio::spawn(async move { client.token_inner(rejected.as_deref()).await })
            .await
            .map_err(|_| Error::Protocol("login refresh task failed".into()))?
    }

    async fn token_inner(&self, rejected: Option<&str>) -> Result<String> {
        if self.is_revoked() {
            return Err(Error::SessionRevoked);
        }
        let mut session = self.session.lock().await;
        if self.is_revoked() {
            return Err(Error::SessionRevoked);
        }
        if session.expires_at <= now() + 60
            || rejected == Some(session.tokens.access_token.as_str())
        {
            let tokens = self.refresh_tokens(&session.tokens.refresh_token).await?;
            if self.is_revoked() {
                return Err(Error::SessionRevoked);
            }
            if tokens.account_id != session.tokens.account_id {
                return Err(Error::Protocol("account changed during refresh".into()));
            }
            session.expires_at = now() + tokens.expires_in;
            session.tokens = tokens;
            self.dirty.store(true, Ordering::Release);
        }
        if self.dirty.load(Ordering::Acquire) {
            self.persist_locked(&session)?;
        }
        Ok(session.tokens.access_token.clone())
    }

    async fn refresh_tokens(&self, refresh: &str) -> Result<protocol::Tokens> {
        // Retry a lost response within the server's idempotent rotation window.
        // HTTP failures (especially a rejected refresh token) are never retried.
        for attempt in 0..2 {
            let response = self
                .http
                .post(self.url("/v1/auth/refresh")?)
                .timeout(Duration::from_secs(20))
                .json(&serde_json::json!({"refresh_token":refresh}))
                .send()
                .await;
            match response {
                Ok(response) => match self.checked(response).await?.json().await {
                    Ok(tokens) => return Ok(tokens),
                    Err(_) if attempt == 0 => continue,
                    Err(error) => return Err(error.into()),
                },
                Err(_) if attempt == 0 => continue,
                Err(error) => return Err(error.into()),
            }
        }
        unreachable!("the second refresh attempt returns a result")
    }

    /// Retry an authenticated HTTP request once after a stale access token.
    pub async fn send(
        &self,
        build: impl Fn(&str) -> reqwest::RequestBuilder,
    ) -> Result<reqwest::Response> {
        let token = self.access_token().await?;
        let response = build(&token).send().await?;
        if response.status() != reqwest::StatusCode::UNAUTHORIZED {
            return checked(response).await;
        }
        drop(response);
        let token = self.recover_access_token(&token).await?;
        checked(build(&token).send().await?).await
    }
    pub async fn request<T: Serialize + ?Sized>(
        &self,
        method: reqwest::Method,
        path: &str,
        body: &T,
    ) -> Result<reqwest::Response> {
        let url = self.url(path)?;
        self.send(|token| {
            self.http
                .request(method.clone(), url.clone())
                .bearer_auth(token)
                .json(body)
        })
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
    if status == 401 {
        return Err(Error::SessionRevoked);
    }
    let message = if status == 402 {
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

#[cfg(test)]
mod session_tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    async fn read_request(socket: &mut tokio::net::TcpStream) -> String {
        let mut request = Vec::new();
        let mut buffer = [0u8; 4096];
        loop {
            let n = socket.read(&mut buffer).await.unwrap();
            assert!(n > 0);
            request.extend_from_slice(&buffer[..n]);
            if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&request[..end]);
                let length = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .and_then(|v| v.trim().parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                if request.len() >= end + 4 + length {
                    break;
                }
            }
        }
        String::from_utf8(request).unwrap()
    }

    #[tokio::test]
    async fn cancelling_the_request_during_refresh_still_persists_the_rotation() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let (started, start) = tokio::sync::oneshot::channel();
        let (release, released) = tokio::sync::oneshot::channel();
        let (persisted, saved) = tokio::sync::oneshot::channel();
        let persisted = StdMutex::new(Some(persisted));
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            assert!(
                read_request(&mut socket)
                    .await
                    .starts_with("POST /v1/auth/refresh ")
            );
            started.send(()).unwrap();
            released.await.unwrap();
            let body = serde_json::to_string(&tokens("new-access", "new-refresh")).unwrap();
            socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
        });
        let client = Client::restore(SavedSession {
            api_url: base,
            expires_at: 0,
            tokens: tokens("old-access", "old-refresh"),
        })
        .unwrap()
        .with_persistence(move |session| {
            assert_eq!(session.tokens.refresh_token, "new-refresh");
            if let Some(sender) = persisted.lock().unwrap().take() {
                let _ = sender.send(());
            }
            Ok(())
        });
        let caller = client.clone();
        let request = tokio::spawn(async move { caller.access_token().await });
        start.await.unwrap();
        request.abort();
        assert!(request.await.unwrap_err().is_cancelled());
        release.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(2), saved)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(client.access_token().await.unwrap(), "new-access");
        assert!(!client.is_revoked());
        server.await.unwrap();
    }

    #[tokio::test]
    async fn lost_refresh_response_is_retried_without_forgetting_the_login() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            for attempt in 0..2 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let request = read_request(&mut socket).await;
                assert!(request.contains("old-refresh"));
                if attempt == 0 {
                    drop(socket);
                    continue;
                }
                let body = serde_json::to_string(&tokens("new-access", "new-refresh")).unwrap();
                socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
            }
        });
        let client = Client::restore(SavedSession {
            api_url: base,
            expires_at: 0,
            tokens: tokens("old-access", "old-refresh"),
        })
        .unwrap();
        assert_eq!(client.access_token().await.unwrap(), "new-access");
        assert!(!client.is_revoked());
        server.await.unwrap();
    }

    async fn fixture(
        expired: bool,
        replies: Vec<(&'static str, u16, Value)>,
    ) -> (Client, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            for (path, status, body) in replies {
                let (mut socket, _) = listener.accept().await.unwrap();
                let request = read_request(&mut socket).await;
                assert_eq!(
                    request.lines().next().unwrap().split_whitespace().nth(1),
                    Some(path)
                );
                let body = body.to_string();
                socket.write_all(format!("HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
            }
        });
        let client = Client::restore(SavedSession {
            api_url: base,
            expires_at: if expired { 0 } else { now() + 3600 },
            tokens: tokens("old-access", "old-refresh"),
        })
        .unwrap();
        (client, server)
    }

    fn tokens(access: &str, refresh: &str) -> protocol::Tokens {
        protocol::Tokens {
            access_token: access.into(),
            refresh_token: refresh.into(),
            expires_in: 900,
            account_id: Uuid::nil(),
        }
    }

    #[tokio::test]
    async fn stale_access_token_refreshes_and_retries_without_revoking_clones() {
        let fresh = serde_json::to_value(tokens("new-access", "new-refresh")).unwrap();
        let (client, server) = fixture(
            false,
            vec![
                ("/v1/me", 401, serde_json::json!({})),
                ("/v1/auth/refresh", 200, fresh),
                ("/v1/me", 200, serde_json::json!({"ok":true})),
            ],
        )
        .await;
        let stored = Arc::new(StdMutex::new(None));
        let write = stored.clone();
        let client = client.with_persistence(move |saved| {
            *write.lock().unwrap() = Some(saved.clone());
            Ok(())
        });
        let clone = client.clone();
        assert!(
            client
                .request(reqwest::Method::GET, "/v1/me", &())
                .await
                .is_ok()
        );
        assert!(!clone.is_revoked());
        assert_eq!(clone.access_token().await.unwrap(), "new-access");
        assert_eq!(
            stored
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .tokens
                .refresh_token,
            "new-refresh"
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn simultaneous_rejected_tokens_share_one_refresh_and_persist_before_return() {
        let (client, server) = fixture(
            false,
            vec![(
                "/v1/auth/refresh",
                200,
                serde_json::to_value(tokens("new-access", "new-refresh")).unwrap(),
            )],
        )
        .await;
        let writes = Arc::new(StdMutex::new(0));
        let count = writes.clone();
        let client = client.with_persistence(move |_| {
            *count.lock().unwrap() += 1;
            Ok(())
        });
        let clone = client.clone();
        let (first, second) = tokio::join!(
            client.recover_access_token("old-access"),
            clone.recover_access_token("old-access")
        );
        assert_eq!(first.unwrap(), "new-access");
        assert_eq!(second.unwrap(), "new-access");
        assert_eq!(*writes.lock().unwrap(), 1);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn failed_persistence_retries_saving_without_rotating_again() {
        let (client, server) = fixture(
            true,
            vec![(
                "/v1/auth/refresh",
                200,
                serde_json::to_value(tokens("new-access", "new-refresh")).unwrap(),
            )],
        )
        .await;
        let writes = Arc::new(StdMutex::new(0));
        let count = writes.clone();
        let client = client.with_persistence(move |_| {
            let mut count = count.lock().unwrap();
            *count += 1;
            if *count == 1 {
                Err(Error::Persistence("temporary storage error".into()))
            } else {
                Ok(())
            }
        });
        assert!(matches!(
            client.access_token().await,
            Err(Error::Persistence(_))
        ));
        assert_eq!(client.access_token().await.unwrap(), "new-access");
        assert_eq!(*writes.lock().unwrap(), 2);
        assert!(!client.is_revoked());
        server.await.unwrap();
    }

    #[tokio::test]
    async fn rejected_refresh_revokes_all_clones_but_transient_failures_do_not() {
        for (status, expired) in [
            (401, false),
            (401, true),
            (402, false),
            (429, false),
            (503, false),
        ] {
            let replies = if status == 401 && !expired {
                vec![
                    ("/v1/me", 401, serde_json::json!({})),
                    ("/v1/auth/refresh", 401, serde_json::json!({})),
                ]
            } else {
                vec![(
                    if expired {
                        "/v1/auth/refresh"
                    } else {
                        "/v1/me"
                    },
                    status,
                    serde_json::json!({}),
                )]
            };
            let (client, server) = fixture(expired, replies).await;
            let clone = client.clone();
            assert!(
                client
                    .request(reqwest::Method::GET, "/v1/me", &())
                    .await
                    .is_err()
            );
            assert_eq!(clone.is_revoked(), status == 401);
            server.await.unwrap();
        }
    }
}
