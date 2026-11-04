use crate::{
    entities::attachments,
    error::{ApiError, Result},
};
use sea_orm::{ConnectionTrait, DatabaseTransaction, DbBackend, EntityTrait, Statement};
use serde_json::Value;
use std::collections::HashSet;
use uuid::Uuid;

fn collect(value: &Value, ids: &mut HashSet<Uuid>, depth: usize) -> Result<()> {
    if depth > 32 || ids.len() > 16 {
        return Err(ApiError::BadRequest(
            "too many attachment references".into(),
        ));
    }
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                if key == "remote_blob_id" && !value.is_null() {
                    ids.insert(value.as_str().and_then(|s| s.parse().ok()).ok_or_else(|| {
                        ApiError::BadRequest("invalid attachment reference".into())
                    })?);
                } else if key == "context_json"
                    && let Some(raw) = value.as_str()
                {
                    collect(
                        &serde_json::from_str::<Value>(raw)
                            .map_err(|_| ApiError::BadRequest("invalid context JSON".into()))?,
                        ids,
                        depth + 1,
                    )?;
                } else {
                    collect(value, ids, depth + 1)?;
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                collect(value, ids, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub async fn update(
    tx: &DatabaseTransaction,
    account: Uuid,
    record: Uuid,
    data: &Value,
) -> Result<Vec<Uuid>> {
    let mut ids = HashSet::new();
    collect(data, &mut ids, 0)?;
    if ids.len() > 16 {
        return Err(ApiError::BadRequest(
            "too many attachment references".into(),
        ));
    }
    let previous=tx.query_all_raw(Statement::from_sql_and_values(DbBackend::Postgres,"DELETE FROM attachment_refs WHERE account_id=$1 AND record_id=$2 RETURNING attachment_id",[account.into(),record.into()])).await?;
    for id in ids {
        let attachment = attachments::Entity::find_by_id(id)
            .one(tx)
            .await?
            .ok_or(ApiError::NotFound)?;
        if attachment.account_id != account {
            return Err(ApiError::NotFound);
        }
        tx.execute_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "INSERT INTO attachment_refs VALUES($1,$2,$3)",
            [account.into(), record.into(), id.into()],
        ))
        .await?;
    }
    previous
        .into_iter()
        .map(|row| row.try_get("", "attachment_id").map_err(ApiError::from))
        .collect()
}

pub async fn detach(tx: &DatabaseTransaction, account: Uuid, id: Uuid) -> Result<i64> {
    // Text search preserves legacy references created before the reference index existed.
    let referenced=tx.query_one_raw(Statement::from_sql_and_values(DbBackend::Postgres,
        "SELECT EXISTS(SELECT 1 FROM attachment_refs WHERE account_id=$1 AND attachment_id=$2) OR EXISTS(SELECT 1 FROM records WHERE account_id=$1 AND deleted=false AND data::text LIKE $3) AS used",
        [account.into(),id.into(),format!("%{id}%").into()])).await?.ok_or(ApiError::NotFound)?.try_get::<bool>("","used")?;
    if referenced {
        return Ok(0);
    }
    let Some(row) = attachments::Entity::find_by_id(id).one(tx).await? else {
        return Ok(0);
    };
    if row.account_id != account {
        return Err(ApiError::NotFound);
    }
    tx.execute_raw(Statement::from_sql_and_values(DbBackend::Postgres,
        "INSERT INTO object_uploads(account_id,attachment_id,created_at) VALUES($1,$2,now()-interval '2 hours') ON CONFLICT(account_id,attachment_id) DO UPDATE SET created_at=excluded.created_at",[account.into(),id.into()])).await?;
    attachments::Entity::delete_by_id(id).exec(tx).await?;
    Ok(row.size_bytes)
}
