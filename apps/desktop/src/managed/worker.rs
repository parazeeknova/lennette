//! Socket notifications wake durable cursor-based sync; the local outbox stays
//! offline-capable and a slow recovery pull covers lost notifications.
use super::active;
use futures_util::{SinkExt, StreamExt};
use lennette_sync_client::Client;
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};

pub(super) const RECOVERY_INTERVAL: Duration = Duration::from_secs(60);
static REMOTE_CHANGE: AtomicBool = AtomicBool::new(true);

pub(super) fn take_remote_change() -> bool {
    REMOTE_CHANGE.swap(false, Ordering::AcqRel)
}

pub(super) fn should_sync(pending: bool, remote: bool, failures: u32, idle: Duration) -> bool {
    pending || remote || failures > 0 || idle >= RECOVERY_INTERVAL
}

pub(super) async fn listen() {
    let mut failures = 0u32;
    loop {
        let Some(client) = active() else {
            failures = 0;
            tokio::time::sleep(Duration::from_millis(500)).await;
            continue;
        };
        let started = tokio::time::Instant::now();
        if listen_session(&client).await.is_err() {
            tracing::debug!("Sync notifications disconnected; reconnecting with backoff");
        }
        if active().is_none_or(|current| !client.same_session(&current)) {
            failures = 0;
            continue;
        }
        if started.elapsed() >= Duration::from_secs(60) {
            failures = 0;
        }
        failures = failures.saturating_add(1);
        let base = (1u64 << failures.min(5)).min(30);
        // Different devices should not all reconnect at the same instant.
        let jitter = rand::random::<u64>() % 500;
        tokio::time::sleep(Duration::from_millis(base * 1000 + jitter)).await;
    }
}

async fn listen_session(client: &Client) -> Result<(), ()> {
    // Notifications are hints: HTTP pulls apply the durable cursor and preserve
    // conflict handling. Resume the socket from the last applied local cursor.
    let account = client.account_id().await;
    let cursor = super::DATABASE
        .get()
        .and_then(|path| super::Database::open(path).ok())
        .filter(|db| {
            super::sync::meta(db.connection(), "account")
                .ok()
                .flatten()
                .as_deref()
                == Some(&account.to_string())
        })
        .and_then(|db| super::sync::cursor(db.connection()).ok())
        .unwrap_or(0);
    let mut url = client
        .url(&format!("/v1/sync/ws?after={cursor}"))
        .map_err(|_| ())?;
    let scheme = if url.scheme() == "https" { "wss" } else { "ws" };
    url.set_scheme(scheme).map_err(|_| ())?;
    let mut request = url.as_str().into_client_request().map_err(|_| ())?;
    let token = client.access_token().await.map_err(|_| ())?;
    request.headers_mut().insert(
        "Authorization",
        format!("Bearer {token}").parse().map_err(|_| ())?,
    );
    let connection = tokio::time::timeout(
        Duration::from_secs(10),
        tokio_tungstenite::connect_async(request),
    )
    .await
    .map_err(|_| ())?;
    let (socket, _) = match connection {
        Ok(connection) => connection,
        Err(error) => {
            if matches!(error, tokio_tungstenite::tungstenite::Error::Http(ref response) if response.status().as_u16() == 401)
            {
                client.recover_access_token(&token).await.map_err(|_| ())?;
            }
            return Err(());
        }
    };
    // Reconcile after connecting, covering changes during a network outage.
    REMOTE_CHANGE.store(true, Ordering::Release);
    receive_notifications(
        socket,
        || active().is_some_and(|current| client.same_session(&current)),
        || REMOTE_CHANGE.store(true, Ordering::Release),
        || client.mark_revoked(),
    )
    .await
}

async fn receive_notifications<S>(
    mut socket: tokio_tungstenite::WebSocketStream<S>,
    current: impl Fn() -> bool,
    changed: impl Fn(),
    revoked: impl Fn(),
) -> Result<(), ()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let mut check = tokio::time::interval(Duration::from_secs(1));
    let mut last_frame = tokio::time::Instant::now();
    loop {
        tokio::select! {
            _ = check.tick() => {
                if last_frame.elapsed() >= Duration::from_secs(45) {
                    return Err(());
                }
                if !current() {
                    let _ = socket.close(None).await;
                    return Ok(());
                }
            },
            message = socket.next() => {
                last_frame = tokio::time::Instant::now();
                match message {
                Some(Ok(Message::Text(text))) => {
                    if serde_json::from_str::<serde_json::Value>(&text).ok().is_some_and(|v| v["type"] == "session_revoked") {
                        revoked();
                        return Ok(());
                    }
                    // Live AI/transcription frames share this socket; only
                    // durable record changes should trigger a sync pull.
                    if let Ok(page) = serde_json::from_str::<lennette_sync_client::protocol::PullResponse>(&text)
                        && !page.changes.is_empty()
                    {
                        changed();
                    }
                },
                Some(Ok(Message::Ping(payload))) => socket.send(Message::Pong(payload)).await.map_err(|_| ())?,
                None | Some(Err(_)) | Some(Ok(Message::Close(_))) => return Err(()),
                _ => {},
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn socket_ignores_live_frames_delivers_changes_answers_pings_and_closes_on_signout() {
        use std::sync::{Arc, atomic::AtomicUsize};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let connect =
            tokio_tungstenite::connect_async(format!("ws://{}", listener.local_addr().unwrap()));
        let accept = async {
            let (stream, _) = listener.accept().await.unwrap();
            tokio_tungstenite::accept_async(stream).await.unwrap()
        };
        let (client, mut server) = tokio::join!(connect, accept);
        let (client, _) = client.unwrap();
        let connected = Arc::new(AtomicBool::new(true));
        let count = Arc::new(AtomicUsize::new(0));
        let (live, changes) = (connected.clone(), count.clone());
        let task = tokio::spawn(async move {
            receive_notifications(
                client,
                || live.load(Ordering::Acquire),
                || {
                    changes.fetch_add(1, Ordering::AcqRel);
                },
                || {},
            )
            .await
        });
        for text in [
            r#"{"type":"live","event":{"type":"transcript"}}"#,
            r#"{"changes":[],"cursor":0,"has_more":false}"#,
            "invalid json",
        ] {
            server.send(Message::Text(text.into())).await.unwrap();
        }
        server
            .send(Message::Ping(vec![1, 2, 3].into()))
            .await
            .unwrap();
        let pong = tokio::time::timeout(Duration::from_secs(2), server.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(matches!(pong, Message::Pong(payload) if payload.as_ref() == [1, 2, 3]));
        assert_eq!(count.load(Ordering::Acquire), 0);
        let page = serde_json::json!({"changes":[{"sequence":1,"id":uuid::Uuid::new_v4(),"kind":"conversation","revision":1,"deleted":false,"data":{},"device_id":uuid::Uuid::new_v4()}],"cursor":1,"has_more":false});
        server
            .send(Message::Text(page.to_string().into()))
            .await
            .unwrap();
        server.send(Message::Ping(vec![4].into())).await.unwrap();
        let _ = tokio::time::timeout(Duration::from_secs(2), server.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(count.load(Ordering::Acquire), 1);
        connected.store(false, Ordering::Release);
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), task)
                .await
                .unwrap()
                .unwrap(),
            Ok(())
        );
        assert!(matches!(server.next().await, Some(Ok(Message::Close(_)))));
    }

    #[tokio::test]
    async fn revoked_control_frame_ends_notifications_and_invalidates_session() {
        use std::sync::Arc;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let connect =
            tokio_tungstenite::connect_async(format!("ws://{}", listener.local_addr().unwrap()));
        let accept = async {
            let (stream, _) = listener.accept().await.unwrap();
            tokio_tungstenite::accept_async(stream).await.unwrap()
        };
        let (client, mut server) = tokio::join!(connect, accept);
        let revoked = Arc::new(AtomicBool::new(false));
        let flag = revoked.clone();
        let task = tokio::spawn(async move {
            receive_notifications(
                client.unwrap().0,
                || true,
                || panic!("Revocation must not pull data"),
                || flag.store(true, Ordering::Release),
            )
            .await
        });
        server
            .send(Message::Text(r#"{"type":"session_revoked"}"#.into()))
            .await
            .unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), task)
                .await
                .unwrap()
                .unwrap(),
            Ok(())
        );
        assert!(revoked.load(Ordering::Acquire));
    }

    #[test]
    fn idle_clients_only_pull_for_changes_retries_or_recovery() {
        assert!(!should_sync(false, false, 0, Duration::from_millis(500)));
        assert!(!should_sync(false, false, 0, Duration::from_secs(59)));
        assert!(should_sync(true, false, 0, Duration::ZERO));
        assert!(should_sync(false, true, 0, Duration::ZERO));
        assert!(should_sync(false, false, 1, Duration::ZERO));
        assert!(should_sync(false, false, 0, RECOVERY_INTERVAL));
    }
}
