//! Read-only, account-scoped views of the desktop's existing synced records.
pub mod realtime;
mod redis;

use crate::{
    auth::Principal,
    error::{ApiError, Result},
    state::AppState,
};
use axum::{
    Extension, Json,
    extract::{Path, Query, State},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

const SORT: &str =
    "LEFT(COALESCE(data->>'updated_at',data->>'started_at',data->>'created_at',''),64)";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListQuery {
    pub account: Option<Uuid>,
    pub kind: String,
    #[serde(default)]
    pub q: String,
    pub after: Option<String>,
    pub limit: Option<u64>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PageQuery {
    pub account: Option<Uuid>,
    pub after: Option<String>,
    pub limit: Option<u64>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PageCursor {
    id: Uuid,
    sort: String,
}
impl PageCursor {
    fn decode(raw: &Option<String>) -> Result<Option<Self>> {
        let Some(raw) = raw else { return Ok(None) };
        let invalid = || ApiError::BadRequest("invalid page cursor".into());
        if raw.len() > 256 {
            return Err(invalid());
        }
        let decoded = URL_SAFE_NO_PAD.decode(raw).map_err(|_| invalid())?;
        let cursor: Self = serde_json::from_slice(&decoded).map_err(|_| invalid())?;
        if cursor.sort.len() > 64 {
            return Err(invalid());
        }
        Ok(Some(cursor))
    }
}
fn entry(id: Uuid, kind: String, data: Value, sort: String) -> Value {
    let cursor = URL_SAFE_NO_PAD.encode(json!({"id":id,"sort":sort}).to_string());
    json!({"id":id,"kind":kind,"data":data,"cursor":cursor})
}
fn page(mut items: Vec<Value>, limit: usize) -> Value {
    let more = items.len() > limit;
    items.truncate(limit);
    let next = more
        .then(|| items.last().map(|item| item["cursor"].clone()))
        .flatten();
    for item in &mut items {
        if let Some(object) = item.as_object_mut() {
            object.remove("cursor");
        }
    }
    json!({"items":items,"next":next})
}

pub async fn list(
    State(state): State<AppState>,
    Extension(p): Extension<Principal>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Value>> {
    if q.account.is_some_and(|id| id != p.account_id) {
        return Err(ApiError::Unauthorized);
    }
    if !matches!(
        q.kind.as_str(),
        "conversation" | "meeting" | "attachment" | "file"
    ) || q.q.chars().count() > 200
    {
        return Err(ApiError::BadRequest(
            "choose conversations, meetings or attachments; search up to 200 characters".into(),
        ));
    }
    state
        .limit(format!("workspace-read:{}", p.account_id), 1200, 60)
        .await?;
    let limit = q.limit.unwrap_or(30).clamp(1, 50) as usize;
    let cursor = PageCursor::decode(&q.after)?;
    if q.kind == "file"
        && let Some(cursor) = &cursor
    {
        chrono::DateTime::parse_from_rfc3339(&cursor.sort)
            .map_err(|_| ApiError::BadRequest("invalid file cursor".into()))?;
    }
    let values = vec![
        p.account_id.into(),
        q.kind.clone().into(),
        q.q.to_lowercase().into(),
        cursor.as_ref().map(|c| c.sort.clone()).into(),
        cursor.as_ref().map(|c| c.id).into(),
        ((limit + 1) as i64).into(),
    ];
    let rows = if q.kind == "file" {
        state.db.query_all_raw(Statement::from_sql_and_values(DbBackend::Postgres,
            "SELECT id,created_at AS sort_key,'file' AS kind,jsonb_build_object('name',name,'mime_type',content_type,'size_bytes',size_bytes,'created_at',created_at) AS data FROM attachments a WHERE account_id=$1 AND $2='file' AND POSITION($3 IN lower(LEFT(name,512)))>0 AND ($4::text IS NULL OR (created_at,id)<($4::timestamptz,$5::uuid)) ORDER BY created_at DESC,id DESC LIMIT $6",values)).await?
    } else {
        state.db.query_all_raw(Statement::from_sql_and_values(DbBackend::Postgres,format!(
            "SELECT id,kind,{SORT} AS sort_key,jsonb_build_object('title',LEFT(data->>'title',200),'name',LEFT(data->>'name',200),'created_at',data->'created_at','updated_at',data->'updated_at','started_at',data->'started_at','ended_at',data->'ended_at','duration_seconds',data->'duration_seconds','mime_type',data->'mime_type','size_bytes',data->'size_bytes') AS data FROM records WHERE account_id=$1 AND kind=$2 AND NOT deleted AND POSITION($3 IN lower(LEFT(COALESCE(data->>'title',data->>'name',''),512)))>0 AND ($4::text IS NULL OR ({SORT},id)<($4::text,$5::uuid)) ORDER BY {SORT} DESC,id DESC LIMIT $6"),values)).await?
    };
    let items = rows
        .into_iter()
        .map(|r| {
            let sort = if q.kind == "file" {
                r.try_get::<chrono::DateTime<chrono::Utc>>("", "sort_key")?
                    .to_rfc3339()
            } else {
                r.try_get::<String>("", "sort_key")?
            };
            Ok(entry(
                r.try_get("", "id")?,
                r.try_get("", "kind")?,
                r.try_get("", "data")?,
                sort,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Json(page(items, limit)))
}

pub async fn detail(
    State(state): State<AppState>,
    Extension(p): Extension<Principal>,
    Path(id): Path<Uuid>,
    Query(q): Query<PageQuery>,
) -> Result<Json<Value>> {
    if q.account.is_some_and(|id| id != p.account_id) {
        return Err(ApiError::Unauthorized);
    }
    state
        .limit(format!("workspace-read:{}", p.account_id), 1200, 60)
        .await?;
    let row=state.db.query_one_raw(Statement::from_sql_and_values(DbBackend::Postgres,
        "SELECT kind,data FROM records WHERE account_id=$1 AND id=$2 AND NOT deleted AND kind IN ('conversation','meeting','attachment')",[p.account_id.into(),id.into()])).await?.ok_or(ApiError::NotFound)?;
    let kind: String = row.try_get("", "kind")?;
    let data: Value = row.try_get("", "data")?;
    let mut result = json!({"record":{"id":id,"kind":kind,"data":data},"items":[],"next":null,"extras":[],"files":[]});
    if kind == "attachment" {
        return Ok(Json(result));
    }
    // Identifiers and ordering expressions are selected from constants, never interpolated input.
    let (field, child, sort) = if kind == "conversation" {
        (
            "conversation_id",
            "message",
            "CASE WHEN jsonb_typeof(data->'timestamp')='number' THEN LEAST(GREATEST((data->>'timestamp')::numeric,0),1e15)::double precision ELSE 0 END",
        )
    } else {
        (
            "meeting_id",
            "transcript",
            "CASE WHEN jsonb_typeof(data->'start_time')='number' THEN LEAST(GREATEST((data->>'start_time')::numeric,0),1e15)::double precision ELSE 0 END",
        )
    };
    let limit = q.limit.unwrap_or(15).clamp(1, 20) as usize;
    let cursor = PageCursor::decode(&q.after)?;
    let after = cursor
        .as_ref()
        .map(|c| {
            c.sort
                .parse::<f64>()
                .map_err(|_| ApiError::BadRequest("invalid history cursor".into()))
        })
        .transpose()?;
    if after.is_some_and(|n| !n.is_finite() || !(0.0..=1e15).contains(&n)) {
        return Err(ApiError::BadRequest("invalid history cursor".into()));
    }
    let rows=state.db.query_all_raw(Statement::from_sql_and_values(DbBackend::Postgres,format!(
        "SELECT id,kind,{sort} AS sort_key,data FROM records WHERE account_id=$1 AND NOT deleted AND data->>'{field}'=$2 AND kind=$3 AND ($4::double precision IS NULL OR ({sort},id)<($4::double precision,$5::uuid)) ORDER BY {sort} DESC,id DESC LIMIT $6"),
        [p.account_id.into(),id.to_string().into(),child.into(),after.into(),cursor.as_ref().map(|c|c.id).into(),((limit+1) as i64).into()])).await?;
    let items = rows
        .into_iter()
        .map(|r| {
            Ok(entry(
                r.try_get("", "id")?,
                r.try_get("", "kind")?,
                r.try_get("", "data")?,
                r.try_get::<f64>("", "sort_key")?.to_string(),
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    let paged = page(items, limit);
    result["items"] = paged["items"].clone();
    result["next"] = paged["next"].clone();
    if q.after.is_none() {
        let extras=state.db.query_all_raw(Statement::from_sql_and_values(DbBackend::Postgres,format!(
            "SELECT id,kind,data FROM records WHERE account_id=$1 AND NOT deleted AND data->>'{field}'=$2 AND kind IN ('recap','context','speaker','analysis') ORDER BY kind,id LIMIT 32"),[p.account_id.into(),id.to_string().into()])).await?;
        result["extras"]=json!(extras.into_iter().map(|r|Ok(json!({"id":r.try_get::<Uuid>("","id")?,"kind":r.try_get::<String>("","kind")?,"data":r.try_get::<Value>("","data")?}))).collect::<Result<Vec<_>>>()?);
        let files=state.db.query_all_raw(Statement::from_sql_and_values(DbBackend::Postgres,format!(
            "SELECT DISTINCT a.id,a.name,a.content_type,a.size_bytes FROM attachments a JOIN attachment_refs f ON f.attachment_id=a.id AND f.account_id=a.account_id JOIN records r ON r.id=f.record_id AND r.account_id=f.account_id WHERE a.account_id=$1 AND NOT r.deleted AND (r.id=$2 OR r.data->>'{field}'=$3) LIMIT 32"),[p.account_id.into(),id.into(),id.to_string().into()])).await?;
        result["files"]=json!(files.into_iter().map(|r|Ok(json!({"id":r.try_get::<Uuid>("","id")?,"kind":"file","data":{"name":r.try_get::<String>("","name")?,"mime_type":r.try_get::<String>("","content_type")?,"size_bytes":r.try_get::<i64>("","size_bytes")?}}))).collect::<Result<Vec<_>>>()?);
    }
    Ok(Json(result))
}
