pub mod plans;
use crate::{
    auth::Principal,
    entities::{
        accounts, billing_holds, reservations, subscriptions, usage_buckets, webhook_events,
    },
    error::{ApiError, Result},
    plans::Plan,
    state::AppState,
};
use axum::{Extension, Json, body::Bytes, extract::State, http::HeaderMap};
use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::{DateTime, Utc};
use hmac::{Hmac, KeyInit, Mac};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseTransaction, EntityTrait, QueryFilter, QuerySelect, Set,
    TransactionTrait,
};
use secrecy::ExposeSecret;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::Sha256;
use uuid::Uuid;

pub async fn entitlement(
    db: &impl sea_orm::ConnectionTrait,
    account: Uuid,
) -> Result<(Plan, subscriptions::Model)> {
    let sub = subscriptions::Entity::find_by_id(account)
        .one(db)
        .await?
        .ok_or(ApiError::PaymentRequired)?;
    if billing_holds::Entity::find_by_id(&sub.subscription_id)
        .one(db)
        .await?
        .is_some()
    {
        return Err(ApiError::PaymentRequired);
    }
    // Cancellation-at-period-end keeps access until the paid period expires.
    if !matches!(sub.status.as_str(), "active" | "cancelled")
        || sub.period_end <= Utc::now()
        || sub.period_start > Utc::now()
    {
        return Err(ApiError::PaymentRequired);
    }
    Ok((
        plans::plan(&sub.plan).ok_or(ApiError::PaymentRequired)?,
        sub,
    ))
}

/// Account row is the serialization point for quota, storage and sync writes.
pub async fn lock_account(tx: &DatabaseTransaction, account: Uuid) -> Result<accounts::Model> {
    accounts::Entity::find_by_id(account)
        .lock_exclusive()
        .one(tx)
        .await?
        .ok_or(ApiError::Unauthorized)
}
pub async fn reserve(state: &AppState, account: Uuid, kind: &str, amount: i64) -> Result<Uuid> {
    if amount <= 0 || !matches!(kind, "ai" | "speech") {
        return Err(ApiError::BadRequest("invalid reservation".into()));
    }
    let tx = state.db.begin().await?;
    lock_account(&tx, account).await?;
    let (plan, sub) = entitlement(&tx, account).await?;
    let bucket = usage_buckets::Entity::find()
        .filter(usage_buckets::Column::AccountId.eq(account))
        .filter(usage_buckets::Column::PeriodStart.eq(sub.period_start))
        .one(&tx)
        .await?;
    let bucket = match bucket {
        Some(b) => b,
        None => {
            usage_buckets::ActiveModel {
                id: Set(Uuid::new_v4()),
                account_id: Set(account),
                period_start: Set(sub.period_start),
                ai_spent: Set(0),
                speech_seconds: Set(0),
            }
            .insert(&tx)
            .await?
        }
    };
    let used = if kind == "ai" {
        bucket.ai_spent
    } else {
        bucket.speech_seconds
    };
    let max = if kind == "ai" {
        plan.ai_microdollars
    } else {
        plan.transcription_seconds
    };
    if used.saturating_add(amount) > max {
        return Err(ApiError::PaymentRequired);
    }
    let bucket_id = bucket.id;
    let mut model: usage_buckets::ActiveModel = bucket.into();
    if kind == "ai" {
        model.ai_spent = Set(used + amount)
    } else {
        model.speech_seconds = Set(used + amount)
    }
    model.update(&tx).await?;
    let id = Uuid::new_v4();
    reservations::ActiveModel {
        id: Set(id),
        account_id: Set(account),
        bucket_id: Set(bucket_id),
        kind: Set(kind.into()),
        amount: Set(amount),
        settled: Set(false),
        created_at: Set(Utc::now()),
    }
    .insert(&tx)
    .await?;
    tx.commit().await?;
    Ok(id)
}
/// Unknown provider outcomes keep the reservation charged rather than enabling free usage.
pub async fn settle(state: &AppState, id: Uuid, actual: i64) -> Result<()> {
    let tx = state.db.begin().await?;
    let reservation = reservations::Entity::find_by_id(id)
        .one(&tx)
        .await?
        .ok_or(ApiError::NotFound)?;
    lock_account(&tx, reservation.account_id).await?;
    let reservation = reservations::Entity::find_by_id(id)
        .one(&tx)
        .await?
        .ok_or(ApiError::NotFound)?;
    if reservation.settled {
        tx.commit().await?;
        return Ok(());
    }
    if actual > reservation.amount {
        tracing::error!(reservation=%id,reserved=reservation.amount,actual,"provider usage exceeded reservation; review model pricing and limits")
    }
    let refund = reservation.amount.saturating_sub(actual.max(0));
    let bucket = usage_buckets::Entity::find_by_id(reservation.bucket_id)
        .one(&tx)
        .await?
        .ok_or(ApiError::NotFound)?;
    let mut bucket: usage_buckets::ActiveModel = bucket.into();
    if reservation.kind == "ai" {
        bucket.ai_spent = Set(bucket.ai_spent.clone().unwrap().saturating_sub(refund))
    } else {
        bucket.speech_seconds = Set(bucket
            .speech_seconds
            .clone()
            .unwrap()
            .saturating_sub(refund))
    }
    bucket.update(&tx).await?;
    let mut r: reservations::ActiveModel = reservation.into();
    r.settled = Set(true);
    r.update(&tx).await?;
    tx.commit().await?;
    Ok(())
}

pub async fn usage(
    State(state): State<AppState>,
    Extension(p): Extension<Principal>,
) -> Result<Json<Value>> {
    let sub = subscriptions::Entity::find_by_id(p.account_id)
        .one(&state.db)
        .await?;
    let bucket = if let Some(s) = &sub {
        usage_buckets::Entity::find()
            .filter(usage_buckets::Column::AccountId.eq(p.account_id))
            .filter(usage_buckets::Column::PeriodStart.eq(s.period_start))
            .one(&state.db)
            .await?
    } else {
        None
    };
    let account = accounts::Entity::find_by_id(p.account_id)
        .one(&state.db)
        .await?
        .ok_or(ApiError::Unauthorized)?;
    Ok(Json(
        json!({"subscription":sub.map(|s|json!({"plan":s.plan,"status":s.status,"period_start":s.period_start,"period_end":s.period_end})),"ai_used_microdollars":bucket.as_ref().map_or(0,|b|b.ai_spent),"transcription_seconds":bucket.as_ref().map_or(0,|b|b.speech_seconds),"storage_bytes":account.storage_bytes}),
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Checkout {
    pub plan: String,
}
pub async fn checkout(
    State(state): State<AppState>,
    Extension(p): Extension<Principal>,
    Json(request): Json<Checkout>,
) -> Result<Json<Value>> {
    state
        .limit(format!("checkout:{}", p.account_id), 5, 60)
        .await?;
    let product = match request.plan.as_str() {
        "managed" => &state.config.dodo_standard_product,
        "managed_plus" => &state.config.dodo_plus_product,
        _ => return Err(ApiError::BadRequest("unknown plan".into())),
    };
    if product.is_empty() || state.config.dodo_key.expose_secret().is_empty() {
        return Err(ApiError::Unavailable);
    }
    if entitlement(&state.db, p.account_id).await.is_ok() {
        return Err(ApiError::BadRequest(
            "manage the existing subscription in the billing portal".into(),
        ));
    }
    let account = accounts::Entity::find_by_id(p.account_id)
        .one(&state.db)
        .await?
        .ok_or(ApiError::Unauthorized)?;
    let return_url = state
        .config
        .website_url
        .join("/downloads")
        .map_err(anyhow::Error::from)?;
    let response:Value=state.http.post(state.config.dodo_url.join("checkouts").map_err(anyhow::Error::from)?).bearer_auth(state.config.dodo_key.expose_secret())
        .json(&json!({"product_cart":[{"product_id":product,"quantity":1}],"customer":{"email":account.email,"name":account.display_name},"metadata":{"account_id":p.account_id.to_string()},"return_url":return_url.as_str()})).send().await?.error_for_status()?.json().await?;
    Ok(Json(
        json!({"checkout_url":response["checkout_url"],"session_id":response["session_id"]}),
    ))
}
pub async fn portal(
    State(state): State<AppState>,
    Extension(p): Extension<Principal>,
) -> Result<Json<Value>> {
    let sub = subscriptions::Entity::find_by_id(p.account_id)
        .one(&state.db)
        .await?
        .ok_or(ApiError::NotFound)?;
    if state.config.dodo_key.expose_secret().is_empty() {
        return Err(ApiError::Unavailable);
    }
    let url = state
        .config
        .dodo_url
        .join(&format!(
            "customers/{}/customer-portal/session",
            sub.customer_id
        ))
        .map_err(anyhow::Error::from)?;
    let response: Value = state
        .http
        .post(url)
        .bearer_auth(state.config.dodo_key.expose_secret())
        .json(&json!({}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(Json(json!({"url":response["link"]})))
}

pub fn verify_webhook(secret: &str, headers: &HeaderMap, body: &[u8], now: i64) -> Result<String> {
    let header = |name| {
        headers
            .get(name)
            .and_then(|h| h.to_str().ok())
            .ok_or(ApiError::Unauthorized)
    };
    let id = header("webhook-id")?;
    let timestamp = header("webhook-timestamp")?;
    let seconds: i64 = timestamp.parse().map_err(|_| ApiError::Unauthorized)?;
    if now.abs_diff(seconds) > 300 || id.len() > 200 {
        return Err(ApiError::Unauthorized);
    }
    let key = STANDARD
        .decode(secret.strip_prefix("whsec_").unwrap_or(secret))
        .map_err(|_| ApiError::Unauthorized)?;
    let mut mac =
        <Hmac<Sha256> as KeyInit>::new_from_slice(&key).map_err(|_| ApiError::Unauthorized)?;
    mac.update(format!("{id}.{timestamp}.").as_bytes());
    mac.update(body);
    for signature in header("webhook-signature")?.split_whitespace() {
        if let Some(encoded) = signature.strip_prefix("v1,")
            && let Ok(bytes) = STANDARD.decode(encoded)
            && mac.clone().verify_slice(&bytes).is_ok()
        {
            return Ok(id.into());
        }
    }
    Err(ApiError::Unauthorized)
}

pub async fn webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Value>> {
    let secret = state.config.dodo_webhook_secret.expose_secret();
    if secret.is_empty() {
        return Err(ApiError::Unavailable);
    }
    let id = verify_webhook(secret, &headers, &body, Utc::now().timestamp())?;
    let payload: Value = serde_json::from_slice(&body)
        .map_err(|_| ApiError::BadRequest("invalid webhook JSON".into()))?;
    let event = payload["type"]
        .as_str()
        .ok_or_else(|| ApiError::BadRequest("missing event type".into()))?;
    if webhook_events::Entity::find_by_id(&id)
        .one(&state.db)
        .await?
        .is_some()
    {
        return Ok(Json(json!({"received":true})));
    }
    let hold = event == "refund.succeeded" || event.starts_with("dispute.");
    if !event.starts_with("subscription.") && !hold {
        return Ok(Json(json!({"received":true})));
    }
    let mut resource = payload["data"].clone();
    if hold && resource["subscription_id"].as_str().is_none() {
        let payment = resource["payment_id"]
            .as_str()
            .filter(|s| s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
            .ok_or_else(|| ApiError::BadRequest("missing billing payment id".into()))?;
        resource = state
            .http
            .get(
                state
                    .config
                    .dodo_url
                    .join(&format!("payments/{payment}"))
                    .map_err(anyhow::Error::from)?,
            )
            .bearer_auth(state.config.dodo_key.expose_secret())
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        if resource["subscription_id"].is_null() {
            return Ok(Json(json!({"received":true})));
        }
    }
    // Re-fetch current subscription so delayed/out-of-order webhook snapshots cannot restore access.
    let subscription_id = resource["subscription_id"]
        .as_str()
        .filter(|s| s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
        .ok_or_else(|| ApiError::BadRequest("missing subscription id".into()))?;
    if state.config.dodo_key.expose_secret().is_empty() {
        return Err(ApiError::Unavailable);
    }
    let tx = state.db.begin().await?;
    use sea_orm::ConnectionTrait;
    tx.execute_raw(sea_orm::Statement::from_sql_and_values(
        sea_orm::DbBackend::Postgres,
        "SELECT pg_advisory_xact_lock(hashtextextended($1,0))",
        [subscription_id.into()],
    ))
    .await?;
    if webhook_events::Entity::find_by_id(&id)
        .one(&tx)
        .await?
        .is_some()
    {
        return Ok(Json(json!({"received":true})));
    }
    let current: Value = state
        .http
        .get(
            state
                .config
                .dodo_url
                .join(&format!("subscriptions/{subscription_id}"))
                .map_err(anyhow::Error::from)?,
        )
        .bearer_auth(state.config.dodo_key.expose_secret())
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    if hold
        && billing_holds::Entity::find_by_id(subscription_id)
            .one(&tx)
            .await?
            .is_none()
    {
        billing_holds::ActiveModel {
            subscription_id: Set(subscription_id.into()),
            reason: Set(event.into()),
            created_at: Set(Utc::now()),
        }
        .insert(&tx)
        .await?;
    }
    let owner = current["metadata"]["account_id"]
        .as_str()
        .and_then(|s| s.parse::<Uuid>().ok());
    if let Some(owner) = owner
        && accounts::Entity::find_by_id(owner)
            .one(&tx)
            .await?
            .is_some()
    {
        apply_subscription(&state, &tx, &current).await?;
    }
    webhook_events::ActiveModel {
        id: Set(id),
        event_type: Set(event.into()),
        received_at: Set(Utc::now()),
    }
    .insert(&tx)
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"received":true})))
}
pub async fn apply_subscription(
    state: &AppState,
    tx: &DatabaseTransaction,
    value: &Value,
) -> Result<()> {
    let string = |key: &str| {
        value[key]
            .as_str()
            .ok_or_else(|| ApiError::BadRequest(format!("missing subscription {key}")))
    };
    let account: Uuid = value["metadata"]["account_id"]
        .as_str()
        .ok_or_else(|| ApiError::BadRequest("subscription has no account metadata".into()))?
        .parse()
        .map_err(|_| ApiError::BadRequest("invalid account metadata".into()))?;
    lock_account(tx, account).await?;
    let product = string("product_id")?;
    let plan = if !state.config.dodo_standard_product.is_empty()
        && product == state.config.dodo_standard_product
    {
        "managed"
    } else if !state.config.dodo_plus_product.is_empty()
        && product == state.config.dodo_plus_product
    {
        "managed_plus"
    } else {
        return Err(ApiError::BadRequest(
            "unexpected subscription product".into(),
        ));
    };
    let id = string("subscription_id")?;
    if let Some(existing) = subscriptions::Entity::find_by_id(account).one(tx).await?
        && existing.subscription_id != id
        && existing.period_end > Utc::now()
        && existing.status == "active"
        && billing_holds::Entity::find_by_id(&existing.subscription_id)
            .one(tx)
            .await?
            .is_none()
    {
        return Err(ApiError::Conflict);
    }
    let start: DateTime<Utc> = string("previous_billing_date")?
        .parse()
        .map_err(|_| ApiError::BadRequest("invalid billing period".into()))?;
    let end: DateTime<Utc> = string("next_billing_date")?
        .parse()
        .map_err(|_| ApiError::BadRequest("invalid billing period".into()))?;
    if end <= start {
        return Err(ApiError::BadRequest("invalid billing period".into()));
    }
    let model = subscriptions::ActiveModel {
        account_id: Set(account),
        subscription_id: Set(id.into()),
        customer_id: Set(value["customer"]["customer_id"]
            .as_str()
            .ok_or_else(|| ApiError::BadRequest("missing customer".into()))?
            .into()),
        product_id: Set(product.into()),
        plan: Set(plan.into()),
        status: Set(string("status")?.into()),
        period_start: Set(start),
        period_end: Set(end),
        updated_at: Set(Utc::now()),
    };
    use sea_orm::sea_query::OnConflict;
    subscriptions::Entity::insert(model)
        .on_conflict(
            OnConflict::column(subscriptions::Column::AccountId)
                .update_columns([
                    subscriptions::Column::SubscriptionId,
                    subscriptions::Column::CustomerId,
                    subscriptions::Column::ProductId,
                    subscriptions::Column::Plan,
                    subscriptions::Column::Status,
                    subscriptions::Column::PeriodStart,
                    subscriptions::Column::PeriodEnd,
                    subscriptions::Column::UpdatedAt,
                ])
                .to_owned(),
        )
        .exec(tx)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn signed(body: &[u8], timestamp: i64) -> HeaderMap {
        let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(b"test-secret").unwrap();
        mac.update(format!("evt_1.{timestamp}.").as_bytes());
        mac.update(body);
        let mut h = HeaderMap::new();
        h.insert("webhook-id", "evt_1".parse().unwrap());
        h.insert("webhook-timestamp", timestamp.to_string().parse().unwrap());
        h.insert(
            "webhook-signature",
            format!("v1,{}", STANDARD.encode(mac.finalize().into_bytes()))
                .parse()
                .unwrap(),
        );
        h
    }
    #[test]
    fn signatures_reject_tampering_and_stale_delivery() {
        let h = signed(b"{}", 1000);
        let secret = format!("whsec_{}", STANDARD.encode(b"test-secret"));
        assert!(verify_webhook(&secret, &h, b"{}", 1000).is_ok());
        assert!(verify_webhook(&secret, &h, b"{ }", 1000).is_err());
        assert!(verify_webhook(&secret, &h, b"{}", 1301).is_err());
        assert!(verify_webhook(&secret, &HeaderMap::new(), b"{}", 1000).is_err());
    }
}
