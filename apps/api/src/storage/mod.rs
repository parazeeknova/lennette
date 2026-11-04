//! Private R2 objects. Only authenticated API handlers expose attachment bytes.
pub mod cleanup;
mod r2;
pub mod references;
use crate::{
    error::{ApiError, Result},
    state::AppState,
};
use object_store::{ObjectStore, ObjectStoreExt, path::Path};
pub use r2::connect;
use std::{future::Future, time::Duration};
use uuid::Uuid;

pub async fn operation<T>(future: impl Future<Output = object_store::Result<T>>) -> Result<T> {
    match tokio::time::timeout(Duration::from_secs(45), future).await {
        Ok(result) => result.map_err(failure),
        Err(_) => Err(ApiError::StorageUnavailable),
    }
}

pub fn key(account: Uuid, attachment: Uuid) -> Path {
    Path::from(format!("accounts/{account}/attachments/{attachment}"))
}

pub fn checksum(bytes: &[u8]) -> String {
    use base64::Engine;
    use sha2::Digest;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sha2::Sha256::digest(bytes))
}

pub fn bucket(state: &AppState) -> Result<&dyn ObjectStore> {
    state.attachments.as_deref().ok_or(ApiError::Unavailable)
}

pub fn failure(error: object_store::Error) -> ApiError {
    // SDK error chains may include signed URLs or credential identifiers.
    let _ = error;
    tracing::warn!("attachment object-store operation failed");
    ApiError::StorageUnavailable
}

pub async fn delete(state: &AppState, account: Uuid, attachment: Uuid) -> Result<()> {
    let key = key(account, attachment);
    let store = bucket(state)?;
    operation(async {
        match store.delete(&key).await {
            Ok(()) | Err(object_store::Error::NotFound { .. }) => Ok(()),
            Err(error) => Err(error),
        }
    })
    .await
}
