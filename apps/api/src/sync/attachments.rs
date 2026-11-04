use crate::{
    auth::Principal,
    billing,
    entities::{accounts, attachments},
    error::{ApiError, Result},
    state::AppState,
    storage,
};
use axum::{
    Extension, Json,
    body::Bytes,
    extract::{Path, Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use object_store::ObjectStoreExt;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set, TransactionTrait};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

#[derive(Deserialize)]
pub struct UploadQuery {
    pub id: Option<Uuid>,
    pub name: String,
    pub content_type: String,
}
pub async fn upload(
    State(state): State<AppState>,
    Extension(p): Extension<Principal>,
    Query(q): Query<UploadQuery>,
    body: Bytes,
) -> Result<Json<Value>> {
    if q.name.is_empty() || q.name.len() > 200 || body.is_empty() || body.len() > 10 * 1024 * 1024 {
        return Err(ApiError::BadRequest(
            "upload a supported meeting document up to 10 MiB".into(),
        ));
    }
    let id = q.id.unwrap_or_else(Uuid::new_v4);
    let _permit = state
        .upload_permits
        .clone()
        .try_acquire_owned()
        .map_err(|_| ApiError::RateLimited)?;
    state
        .limit(format!("uploads:{}", p.account_id), 20, 60)
        .await?;
    let declared =
        lennette_attachments::declared_type(&q.name).map_err(|m| ApiError::BadRequest(m.into()))?;
    if declared != q.content_type {
        return Err(ApiError::BadRequest(lennette_attachments::REJECTED.into()));
    }
    let sha256 = storage::checksum(&body);
    storage::bucket(&state)?;
    billing::entitlement(&state.db, p.account_id).await?;
    crate::security::malware::scan(state.config.clamd_address.as_deref(), &body).await?;
    let (filename, content_type, data) = (q.name.clone(), q.content_type.clone(), body.clone());
    tokio::task::spawn_blocking(move || {
        lennette_attachments::validate(&filename, Some(&content_type), &data)
    })
    .await
    .map_err(|_| ApiError::UnsafeAttachment)?
    .map_err(|message| ApiError::BadRequest(message.into()))?;
    storage::cleanup::begin(&state, p.account_id, id).await?;
    let tx = state.db.begin().await?;
    use sea_orm::ConnectionTrait;
    tx.execute_raw(sea_orm::Statement::from_sql_and_values(
        sea_orm::DbBackend::Postgres,
        "SELECT pg_advisory_xact_lock(hashtextextended($1,0))",
        [id.to_string().into()],
    ))
    .await?;
    let account = billing::lock_account(&tx, p.account_id).await?;
    let (plan, _) = billing::entitlement(&tx, p.account_id).await?;
    if let Some(existing) = attachments::Entity::find_by_id(id).one(&tx).await? {
        if existing.account_id != p.account_id {
            return Err(ApiError::NotFound);
        }
        if existing.sha256 != sha256
            || existing.content_type != q.content_type
            || existing.name != q.name
        {
            return Err(ApiError::Conflict);
        }
        return Ok(Json(json!({"id":id,"size_bytes":existing.size_bytes})));
    }
    let size = body.len() as i64;
    if account.storage_bytes + size > plan.storage_bytes {
        return Err(ApiError::PaymentRequired);
    }
    storage::operation(storage::bucket(&state)?.put(&storage::key(p.account_id, id), body.into()))
        .await?;
    attachments::ActiveModel {
        id: Set(id),
        account_id: Set(p.account_id),
        name: Set(q.name),
        content_type: Set(q.content_type),
        size_bytes: Set(size),
        sha256: Set(sha256),
        created_at: Set(chrono::Utc::now()),
    }
    .insert(&tx)
    .await?;
    let mut model: accounts::ActiveModel = account.into();
    model.storage_bytes = Set(model.storage_bytes.clone().unwrap() + size);
    model.update(&tx).await?;
    tx.commit().await?;
    storage::cleanup::finish(&state, p.account_id, id).await;
    Ok(Json(json!({"id":id,"size_bytes":size})))
}
pub async fn download(
    State(state): State<AppState>,
    Extension(p): Extension<Principal>,
    Path(id): Path<Uuid>,
) -> Result<Response> {
    let row = attachments::Entity::find_by_id(id)
        .filter(attachments::Column::AccountId.eq(p.account_id))
        .one(&state.db)
        .await?
        .ok_or(ApiError::NotFound)?;
    let object =
        storage::operation(storage::bucket(&state)?.get(&storage::key(p.account_id, id))).await?;
    if object.meta.size != row.size_bytes as u64 || object.meta.size > 10 * 1024 * 1024 {
        return Err(ApiError::StorageUnavailable);
    }
    let body = storage::operation(object.bytes()).await?;
    if storage::checksum(&body) != row.sha256 {
        return Err(ApiError::StorageUnavailable);
    }
    Ok((
        [
            (header::CONTENT_TYPE, row.content_type),
            (header::CONTENT_DISPOSITION, "attachment".into()),
            (header::CACHE_CONTROL, "private, no-store".into()),
            (
                header::CONTENT_SECURITY_POLICY,
                "default-src 'none'; sandbox".into(),
            ),
            (
                header::HeaderName::from_static("x-content-type-options"),
                "nosniff".into(),
            ),
        ],
        body,
    )
        .into_response())
}
pub async fn delete_attachment(
    State(state): State<AppState>,
    Extension(p): Extension<Principal>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode> {
    let tx = state.db.begin().await?;
    let account = billing::lock_account(&tx, p.account_id).await?;
    let row = attachments::Entity::find_by_id(id)
        .filter(attachments::Column::AccountId.eq(p.account_id))
        .one(&tx)
        .await?
        .ok_or(ApiError::NotFound)?;
    use sea_orm::ConnectionTrait;
    let linked = tx.query_one_raw(sea_orm::Statement::from_sql_and_values(sea_orm::DbBackend::Postgres,
        "SELECT EXISTS(SELECT 1 FROM attachment_refs WHERE account_id=$1 AND attachment_id=$2) OR EXISTS(SELECT 1 FROM records WHERE account_id=$1 AND deleted=false AND data::text LIKE $3) AS linked",
        [p.account_id.into(),id.into(),format!("%{id}%").into()])).await?.ok_or(ApiError::NotFound)?.try_get::<bool>("","linked")?;
    if linked {
        return Err(ApiError::BadRequest(
            "remove this attachment from synced sources before deleting its file".into(),
        ));
    }
    // Keep metadata and quota until R2 confirms deletion, so failures can be retried.
    storage::delete(&state, p.account_id, id).await?;
    attachments::Entity::delete_by_id(id).exec(&tx).await?;
    let mut model: accounts::ActiveModel = account.into();
    model.storage_bytes = Set(model
        .storage_bytes
        .clone()
        .unwrap()
        .saturating_sub(row.size_bytes));
    model.update(&tx).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
pub async fn delete_account(
    State(state): State<AppState>,
    Extension(p): Extension<Principal>,
) -> Result<StatusCode> {
    state
        .limit(format!("delete-account:{}", p.account_id), 3, 3600)
        .await?;
    use secrecy::ExposeSecret;
    if let Some(sub) = crate::entities::subscriptions::Entity::find_by_id(p.account_id)
        .one(&state.db)
        .await?
        && !matches!(sub.status.as_str(), "cancelled" | "expired" | "failed")
    {
        if state.config.dodo_key.expose_secret().is_empty() {
            return Err(ApiError::Unavailable);
        }
        let response: Value = state
            .http
            .patch(
                state
                    .config
                    .dodo_url
                    .join(&format!("subscriptions/{}", sub.subscription_id))
                    .map_err(anyhow::Error::from)?,
            )
            .bearer_auth(state.config.dodo_key.expose_secret())
            .json(&json!({"status":"cancelled"}))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        if response["status"] != "cancelled" {
            return Err(ApiError::BadRequest(
                "subscription cancellation is still pending; try again after billing confirms"
                    .into(),
            ));
        }
    }
    let tx = state.db.begin().await?;
    // Serialize against uploads while removing all account objects.
    billing::lock_account(&tx, p.account_id).await?;
    use sea_orm::ConnectionTrait;
    // Journal survives account/session cascades and is retried even if R2 is down.
    tx.execute_raw(sea_orm::Statement::from_sql_and_values(
        sea_orm::DbBackend::Postgres,
        "INSERT INTO object_uploads(account_id,attachment_id,created_at) SELECT account_id,id,now()-interval '2 hours' FROM attachments WHERE account_id=$1 ON CONFLICT(account_id,attachment_id) DO UPDATE SET created_at=excluded.created_at",
        [p.account_id.into()],
    )).await?;
    tx.execute_raw(sea_orm::Statement::from_sql_and_values(
        sea_orm::DbBackend::Postgres,
        "UPDATE object_uploads SET created_at=now()-interval '2 hours' WHERE account_id=$1",
        [p.account_id.into()],
    ))
    .await?;
    accounts::Entity::delete_by_id(p.account_id)
        .exec(&tx)
        .await?;
    tx.commit().await?;
    tracing::info!(account_id=%p.account_id, "account deleted; durable object erasure scheduled");
    let worker = state.clone();
    tokio::spawn(async move {
        if storage::cleanup::reconcile(&worker).await.is_err() {
            tracing::warn!("account object erasure deferred to background retry");
        }
    });
    Ok(StatusCode::ACCEPTED)
}
