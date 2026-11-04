//! Durable upload journal closes the R2-write/Postgres-commit crash window.
use crate::{
    entities::{attachments, object_uploads},
    error::Result,
    state::AppState,
    storage,
};
use chrono::{Duration, Utc};
use sea_orm::{
    ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect, Set,
    TransactionTrait,
};
use uuid::Uuid;

async fn lock(
    db: &impl ConnectionTrait,
    id: Uuid,
    wait: bool,
) -> std::result::Result<bool, sea_orm::DbErr> {
    let sql = if wait {
        "SELECT pg_advisory_xact_lock(hashtextextended($1,0))"
    } else {
        "SELECT pg_try_advisory_xact_lock(hashtextextended($1,0)) AS locked"
    };
    let row = db
        .query_one_raw(sea_orm::Statement::from_sql_and_values(
            sea_orm::DbBackend::Postgres,
            sql,
            [id.to_string().into()],
        ))
        .await?;
    Ok(wait || row.is_some_and(|r| r.try_get::<bool>("", "locked").unwrap_or(false)))
}

pub async fn begin(state: &AppState, account: Uuid, id: Uuid) -> Result<()> {
    // A short separate transaction avoids holding a pool connection while opening another.
    let tx = state.db.begin().await?;
    lock(&tx, id, true).await?;
    object_uploads::Entity::insert(object_uploads::ActiveModel {
        account_id: Set(account),
        attachment_id: Set(id),
        created_at: Set(Utc::now()),
    })
    .on_conflict(
        sea_orm::sea_query::OnConflict::columns([
            object_uploads::Column::AccountId,
            object_uploads::Column::AttachmentId,
        ])
        .update_column(object_uploads::Column::CreatedAt)
        .to_owned(),
    )
    .exec(&tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn finish(state: &AppState, account: Uuid, id: Uuid) {
    if object_uploads::Entity::delete_by_id((account, id))
        .exec(&state.db)
        .await
        .is_err()
    {
        tracing::warn!("upload journal cleanup deferred");
    }
}

pub async fn reconcile(state: &AppState) -> Result<()> {
    if state.attachments.is_none() {
        return Ok(());
    }
    reclaim_unlinked(state).await?;
    let cutoff = Utc::now() - Duration::hours(1);
    let jobs = object_uploads::Entity::find()
        .filter(object_uploads::Column::CreatedAt.lt(cutoff))
        .order_by_asc(object_uploads::Column::CreatedAt)
        .limit(100)
        .all(&state.db)
        .await?;
    let mut failed = false;
    for job in jobs {
        if reconcile_job(state, job, cutoff).await.is_err() {
            failed = true;
            tracing::warn!("object cleanup deferred; continuing remaining jobs");
        }
    }
    if failed {
        return Err(crate::error::ApiError::StorageUnavailable);
    }
    Ok(())
}

async fn reclaim_unlinked(state: &AppState) -> Result<()> {
    // A 24-hour grace allows upload-then-sync workflows and interrupted desktop clients.
    let candidates = state.db.query_all_raw(sea_orm::Statement::from_string(sea_orm::DbBackend::Postgres,
        "SELECT id,account_id FROM attachments a WHERE created_at < now()-interval '24 hours' AND NOT EXISTS(SELECT 1 FROM attachment_refs r WHERE r.attachment_id=a.id) AND NOT EXISTS(SELECT 1 FROM records r WHERE r.account_id=a.account_id AND r.deleted=false AND r.data::text LIKE '%' || a.id::text || '%') ORDER BY created_at LIMIT 100")).await?;
    for row in candidates {
        let account = row.try_get("", "account_id")?;
        let id = row.try_get("", "id")?;
        let tx = state.db.begin().await?;
        let model = crate::billing::lock_account(&tx, account).await;
        if model.is_err() {
            continue;
        }
        let reclaimed = storage::references::detach(&tx, account, id).await?;
        if reclaimed > 0 {
            tx.execute_raw(sea_orm::Statement::from_sql_and_values(
                sea_orm::DbBackend::Postgres,
                "UPDATE accounts SET storage_bytes=greatest(0,storage_bytes-$2) WHERE id=$1",
                [account.into(), reclaimed.into()],
            ))
            .await?;
        }
        tx.commit().await?;
    }
    Ok(())
}

async fn reconcile_job(
    state: &AppState,
    job: object_uploads::Model,
    cutoff: chrono::DateTime<Utc>,
) -> Result<()> {
    let tx = state.db.begin().await?;
    if !lock(&tx, job.attachment_id, false).await? {
        return Ok(());
    }
    // Recheck age after acquiring the lock: a newer upload may have refreshed this job.
    let Some(current) = object_uploads::Entity::find_by_id((job.account_id, job.attachment_id))
        .one(&tx)
        .await?
    else {
        return Ok(());
    };
    if current.created_at >= cutoff {
        return Ok(());
    }
    let row = attachments::Entity::find_by_id(job.attachment_id)
        .one(&tx)
        .await?;
    if !row.is_some_and(|r| r.account_id == job.account_id) {
        storage::delete(state, job.account_id, job.attachment_id).await?;
    }
    object_uploads::Entity::delete_by_id((job.account_id, job.attachment_id))
        .exec(&tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

pub fn start(state: AppState) {
    if state.attachments.is_none() {
        return;
    }
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            tokio::select! {
                _ = state.shutdown.cancelled() => break,
                _ = interval.tick() => {
                    if reconcile(&state).await.is_err() { tracing::warn!("object cleanup failed; retrying on next sweep"); }
                }
            }
        }
    });
}
