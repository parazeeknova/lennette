use crate::{
    auth::{self, Principal},
    billing,
    entities::{accounts, attachments, changes, mutations, records},
    error::{ApiError, Result},
    state::AppState,
};
use axum::{
    Extension, Json,
    body::Bytes,
    extract::{
        Path, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use fred::prelude::*;
use futures_util::StreamExt;
use lennette_protocol::{
    Change, MAX_BATCH, Mutation, PullResponse, PushRequest, PushResult, RecordKind,
};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect, Set,
    TransactionTrait,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::Duration;
use uuid::Uuid;

fn change(model: changes::Model) -> Result<Change> {
    Ok(Change {
        sequence: model.sequence,
        id: model.record_id,
        kind: serde_json::from_value(json!(model.kind)).map_err(anyhow::Error::from)?,
        revision: model.revision,
        deleted: model.deleted,
        data: model.data,
        device_id: model.device_id,
    })
}

pub async fn push(
    State(state): State<AppState>,
    Extension(p): Extension<Principal>,
    Json(request): Json<PushRequest>,
) -> Result<Json<Vec<PushResult>>> {
    if request.mutations.is_empty() || request.mutations.len() > MAX_BATCH {
        return Err(ApiError::BadRequest(
            "send between 1 and 100 mutations".into(),
        ));
    }
    let tx = state.db.begin().await?;
    let account = billing::lock_account(&tx, p.account_id).await?;
    let plan = match billing::entitlement(&tx, p.account_id).await {
        Ok((plan, _)) => plan,
        Err(ApiError::PaymentRequired) if request.mutations.iter().all(|m| m.deleted) => {
            crate::plans::STANDARD
        }
        Err(error) => return Err(error),
    };
    let mut sequence = account.sync_sequence;
    let mut size = account.storage_bytes;
    let mut results = Vec::new();
    let originals = request
        .mutations
        .iter()
        .map(|m| m.mutation_id)
        .collect::<std::collections::HashSet<_>>();
    let mut queue = std::collections::VecDeque::from(request.mutations);
    while let Some(mutation) = queue.pop_front() {
        let derived = !originals.contains(&mutation.mutation_id);
        mutation
            .validate_record()
            .map_err(|e| ApiError::BadRequest(e.into()))?;
        let request_hash =
            auth::digest(&serde_json::to_string(&mutation).map_err(anyhow::Error::from)?);
        if let Some(prior) = mutations::Entity::find_by_id((p.account_id, mutation.mutation_id))
            .one(&tx)
            .await?
        {
            if prior.request_hash != request_hash {
                return Err(ApiError::Conflict);
            }
            if !derived {
                results.push(serde_json::from_value(prior.response).map_err(anyhow::Error::from)?);
            }
            continue;
        }
        let existing = records::Entity::find_by_id((p.account_id, mutation.id))
            .one(&tx)
            .await?;
        if mutation.deleted && existing.is_none() {
            return Err(ApiError::NotFound);
        }
        if mutation.deleted && existing.as_ref().is_some_and(|r| r.deleted) {
            let latest = changes::Entity::find()
                .filter(changes::Column::AccountId.eq(p.account_id))
                .filter(changes::Column::RecordId.eq(mutation.id))
                .order_by_desc(changes::Column::Sequence)
                .one(&tx)
                .await?
                .ok_or(ApiError::NotFound)?;
            let result = PushResult {
                mutation_id: mutation.mutation_id,
                change: change(latest)?,
            };
            mutations::ActiveModel {
                account_id: Set(p.account_id),
                id: Set(mutation.mutation_id),
                request_hash: Set(request_hash),
                response: Set(serde_json::to_value(&result).map_err(anyhow::Error::from)?),
            }
            .insert(&tx)
            .await?;
            if !derived {
                results.push(result);
            }
            continue;
        }
        if existing.as_ref().map_or(0, |r| r.revision) != mutation.base_revision
            || existing
                .as_ref()
                .is_some_and(|r| r.kind != mutation.kind.as_str())
        {
            return Err(ApiError::Conflict);
        }
        if !mutation.deleted {
            let parent = match mutation.kind {
                RecordKind::Message => Some(("conversation_id", "conversation")),
                RecordKind::Transcript
                | RecordKind::Recap
                | RecordKind::Speaker
                | RecordKind::Analysis => Some(("meeting_id", "meeting")),
                RecordKind::Context | RecordKind::MeetingConversation => {
                    Some(("conversation_id", "conversation"))
                }
                _ => None,
            };
            if let Some((field, kind)) = parent {
                let id = mutation.data[field]
                    .as_str()
                    .and_then(|s| s.parse::<Uuid>().ok())
                    .ok_or_else(|| {
                        ApiError::BadRequest(format!("{field} must identify a synced parent"))
                    })?;
                let parent = records::Entity::find_by_id((p.account_id, id))
                    .one(&tx)
                    .await?
                    .ok_or(ApiError::NotFound)?;
                if parent.deleted || parent.kind != kind {
                    return Err(ApiError::Conflict);
                }
            }
        }
        if mutation.deleted
            && matches!(
                mutation.kind,
                RecordKind::Conversation | RecordKind::Meeting
            )
        {
            use sea_orm::ConnectionTrait;
            let field = if mutation.kind == RecordKind::Conversation {
                "conversation_id"
            } else {
                "meeting_id"
            };
            let children=tx.query_all_raw(sea_orm::Statement::from_sql_and_values(sea_orm::DbBackend::Postgres,format!("SELECT id,kind,revision FROM records WHERE account_id=$1 AND deleted=false AND data->>'{field}'=$2"),[p.account_id.into(),mutation.id.to_string().into()])).await?;
            for row in children {
                queue.push_front(Mutation {
                    mutation_id: Uuid::new_v4(),
                    id: row.try_get("", "id")?,
                    kind: serde_json::from_value(json!(row.try_get::<String>("", "kind")?))
                        .map_err(anyhow::Error::from)?,
                    base_revision: row.try_get("", "revision")?,
                    deleted: true,
                    data: json!({}),
                });
            }
        }
        let data = if mutation.deleted {
            let previous = existing.as_ref().map(|r| &r.data).unwrap_or(&mutation.data);
            if mutation.kind == RecordKind::Settings {
                Value::Object(
                    previous
                        .as_object()
                        .map(|object| {
                            object
                                .keys()
                                .map(|key| (key.clone(), Value::Null))
                                .collect()
                        })
                        .unwrap_or_default(),
                )
            } else {
                Value::Object(
                    [
                        "_table",
                        "id",
                        "meeting_id",
                        "conversation_id",
                        "kind",
                        "ref_id",
                        "speaker_key",
                        "prompt_id",
                        "file_id",
                        "mode",
                    ]
                    .into_iter()
                    .filter_map(|key| previous.get(key).map(|v| (key.into(), v.clone())))
                    .collect(),
                )
            }
        } else {
            mutation.data
        };
        if mutation.deleted {
            changes::Entity::update_many()
                .col_expr(
                    changes::Column::Data,
                    sea_orm::sea_query::Expr::value(data.clone()),
                )
                .col_expr(
                    changes::Column::Deleted,
                    sea_orm::sea_query::Expr::value(true),
                )
                .filter(changes::Column::AccountId.eq(p.account_id))
                .filter(changes::Column::RecordId.eq(mutation.id))
                .exec(&tx)
                .await?;
            use sea_orm::ConnectionTrait;
            tx.execute_raw(sea_orm::Statement::from_sql_and_values(sea_orm::DbBackend::Postgres,"UPDATE mutations SET response=jsonb_set(jsonb_set(response,'{change,data}',$3::jsonb),'{change,deleted}','true'::jsonb) WHERE account_id=$1 AND response#>>'{change,id}'=$2",[p.account_id.into(),mutation.id.to_string().into(),data.clone().into()])).await?;
        }
        let new_size = if mutation.deleted {
            0
        } else {
            data.to_string().len() as i64
        };
        let record_delta = new_size - existing.as_ref().map_or(0, |r| r.size_bytes);
        // Charge retained change/idempotency payloads too, not only the current record.
        let history_size = 2 * data.to_string().len() as i64 + 512;
        size = size.saturating_add(record_delta + history_size);
        if size > plan.storage_bytes && !mutation.deleted {
            return Err(ApiError::PaymentRequired);
        }
        sequence += 1;
        let revision = mutation.base_revision + 1;
        let model = records::ActiveModel {
            account_id: Set(p.account_id),
            id: Set(mutation.id),
            kind: Set(mutation.kind.as_str().into()),
            revision: Set(revision),
            deleted: Set(mutation.deleted),
            data: Set(data.clone()),
            device_id: Set(p.device_id),
            size_bytes: Set(new_size),
        };
        if existing.is_some() {
            model.update(&tx).await?;
        } else {
            model.insert(&tx).await?;
        }
        let event = changes::ActiveModel {
            account_id: Set(p.account_id),
            sequence: Set(sequence),
            record_id: Set(mutation.id),
            kind: Set(mutation.kind.as_str().into()),
            revision: Set(revision),
            deleted: Set(mutation.deleted),
            data: Set(data),
            device_id: Set(p.device_id),
        }
        .insert(&tx)
        .await?;
        let result = PushResult {
            mutation_id: mutation.mutation_id,
            change: change(event)?,
        };
        mutations::ActiveModel {
            account_id: Set(p.account_id),
            id: Set(mutation.mutation_id),
            request_hash: Set(request_hash),
            response: Set(serde_json::to_value(&result).map_err(anyhow::Error::from)?),
        }
        .insert(&tx)
        .await?;
        if !derived {
            results.push(result);
        }
    }
    let mut account: accounts::ActiveModel = account.into();
    account.sync_sequence = Set(sequence);
    account.storage_bytes = Set(size);
    account.update(&tx).await?;
    tx.commit().await?;
    // Notifications are an optimization. PostgreSQL is the replay source after dropped Redis messages.
    if let Err(error) = state
        .redis
        .publish::<(), _, _>(format!("sync:{}", p.account_id), sequence)
        .await
    {
        tracing::warn!(%error,"sync notification delayed; polling will recover")
    }
    Ok(Json(results))
}
#[derive(Default, Deserialize)]
pub struct Cursor {
    #[serde(default)]
    pub after: i64,
    pub limit: Option<u64>,
}
pub async fn pull_changes(
    state: &AppState,
    account: Uuid,
    after: i64,
    limit: u64,
) -> Result<PullResponse> {
    if after < 0 {
        return Err(ApiError::BadRequest("negative cursor".into()));
    }
    let limit = limit.clamp(1, 100);
    let mut rows = changes::Entity::find()
        .filter(changes::Column::AccountId.eq(account))
        .filter(changes::Column::Sequence.gt(after))
        .order_by_asc(changes::Column::Sequence)
        .limit(limit + 1)
        .all(&state.db)
        .await?;
    let has_more = rows.len() as u64 > limit;
    rows.truncate(limit as usize);
    let changes = rows.into_iter().map(change).collect::<Result<Vec<_>>>()?;
    let cursor = changes.last().map_or(after, |c| c.sequence);
    Ok(PullResponse {
        changes,
        cursor,
        has_more,
    })
}
pub async fn pull(
    State(state): State<AppState>,
    Extension(p): Extension<Principal>,
    Query(q): Query<Cursor>,
) -> Result<Json<PullResponse>> {
    Ok(Json(
        pull_changes(&state, p.account_id, q.after, q.limit.unwrap_or(100)).await?,
    ))
}
pub async fn socket(
    State(state): State<AppState>,
    Extension(p): Extension<Principal>,
    Query(q): Query<Cursor>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response> {
    if q.after < 0 {
        return Err(ApiError::BadRequest("negative cursor".into()));
    }
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or(ApiError::Unauthorized)?
        .to_owned();
    let key = format!("socket:{}", p.session_id);
    let lease = auth::random_token();
    let acquired: Option<String> = state
        .redis
        .set(
            &key,
            &lease,
            Some(Expiration::EX(45)),
            Some(SetOptions::NX),
            false,
        )
        .await?;
    if acquired.is_none() {
        return Err(ApiError::RateLimited);
    }
    Ok(ws.max_message_size(4096).on_upgrade(move|ws|async move{
        if let Err(error)=stream_changes(&state,&p,ws,q.after,&token,&key,&lease).await{tracing::debug!(%error,"sync connection ended")}
        let _:std::result::Result<i64,_>=state.redis.eval("if redis.call('GET',KEYS[1])==ARGV[1] then return redis.call('DEL',KEYS[1]) else return 0 end",vec![key],vec![lease]).await;
    }))
}
async fn stream_changes(
    state: &AppState,
    p: &Principal,
    mut ws: WebSocket,
    mut cursor: i64,
    token: &str,
    key: &str,
    lease: &str,
) -> Result<()> {
    let subscriber = state.redis.clone_new();
    subscriber.init().await?;
    let mut messages = subscriber.message_rx();
    subscriber
        .subscribe(vec![
            format!("sync:{}", p.account_id),
            format!("live:{}", p.account_id),
        ])
        .await?;
    let result=async{
        let mut poll=tokio::time::interval(Duration::from_secs(1));let mut check=tokio::time::interval(Duration::from_secs(15));
        loop {
            let page=pull_changes(state,p.account_id,cursor,100).await?;
            if !page.changes.is_empty(){cursor=page.cursor;ws.send(Message::Text(serde_json::to_string(&page).map_err(anyhow::Error::from)?.into())).await.map_err(anyhow::Error::from)?;if page.has_more{continue}}
            tokio::select!{
                _=state.shutdown.cancelled()=>break,
                _=poll.tick()=>{},
                notification=messages.recv()=>{if let Ok(message)=notification && message.channel.starts_with("live:") && let Ok(text)=message.value.convert::<String>(){ws.send(Message::Text(text.into())).await.map_err(anyhow::Error::from)?;}},
                _=check.tick()=>{
                    auth::authenticate(state,token).await?;
                    let renewed:i64=state.redis.eval("if redis.call('GET',KEYS[1])==ARGV[1] then return redis.call('EXPIRE',KEYS[1],45) else return 0 end",vec![key],vec![lease]).await?;
                    if renewed==0{return Err(ApiError::Unauthorized)}
                    ws.send(Message::Ping(Bytes::new())).await.map_err(anyhow::Error::from)?;
                },
                incoming=ws.next()=>{match incoming{None|Some(Err(_))|Some(Ok(Message::Close(_)))=>break,_=>{}}}
            }
        }
        Ok(())
    }.await;
    subscriber.quit().await.ok();
    result
}

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
    if q.name.is_empty()
        || q.name.len() > 200
        || body.is_empty()
        || body.len() > 10 * 1024 * 1024
        || !matches!(
            q.content_type.as_str(),
            "application/pdf" | "image/png" | "image/jpeg" | "text/plain"
        )
    {
        return Err(ApiError::BadRequest(
            "upload a PDF, PNG, JPEG or text file up to 10 MiB".into(),
        ));
    }
    let id = q.id.unwrap_or_else(Uuid::new_v4);
    use base64::Engine;
    use sha2::Digest;
    let sha256 =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sha2::Sha256::digest(&body));
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
        if existing.sha256 != sha256 {
            return Err(ApiError::Conflict);
        }
        return Ok(Json(json!({"id":id,"size_bytes":existing.size_bytes})));
    }
    let size = body.len() as i64;
    if account.storage_bytes + size > plan.storage_bytes {
        return Err(ApiError::PaymentRequired);
    }
    let path = state.config.attachment_dir.join(id.to_string());
    use tokio::io::AsyncWriteExt;
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(&path).await.map_err(anyhow::Error::from)?;
    file.write_all(&body).await.map_err(anyhow::Error::from)?;
    file.sync_all().await.map_err(anyhow::Error::from)?;
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
    let body = tokio::fs::read(state.config.attachment_dir.join(id.to_string()))
        .await
        .map_err(anyhow::Error::from)?;
    Ok((
        [
            (header::CONTENT_TYPE, row.content_type),
            (header::CONTENT_DISPOSITION, "attachment".into()),
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
    attachments::Entity::delete_by_id(id).exec(&tx).await?;
    let mut model: accounts::ActiveModel = account.into();
    model.storage_bytes = Set(model
        .storage_bytes
        .clone()
        .unwrap()
        .saturating_sub(row.size_bytes));
    model.update(&tx).await?;
    tx.commit().await?;
    if let Err(e) = tokio::fs::remove_file(state.config.attachment_dir.join(id.to_string())).await {
        tracing::warn!(%e,%id,"orphan attachment needs cleanup")
    }
    Ok(StatusCode::NO_CONTENT)
}
pub async fn delete_account(
    State(state): State<AppState>,
    Extension(p): Extension<Principal>,
) -> Result<StatusCode> {
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
    let rows = attachments::Entity::find()
        .filter(attachments::Column::AccountId.eq(p.account_id))
        .all(&state.db)
        .await?;
    accounts::Entity::delete_by_id(p.account_id)
        .exec(&state.db)
        .await?;
    for file in rows {
        tokio::fs::remove_file(state.config.attachment_dir.join(file.id.to_string()))
            .await
            .ok();
    }
    Ok(StatusCode::NO_CONTENT)
}
