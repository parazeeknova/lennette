//! Server-priced, explicitly confirmed upgrades with durable double-charge protection.
use super::*;
use crate::entities::plan_upgrades;
use sea_orm::ConnectionTrait;

fn change(state: &AppState) -> Value {
    json!({"product_id":state.config.dodo_plus_product,"quantity":1,
        "proration_billing_mode":"difference_immediately","effective_at":"immediately",
        "on_payment_failure":"prevent_change"})
}

async fn eligible(
    state: &AppState,
    account: Uuid,
    db: &impl ConnectionTrait,
) -> Result<subscriptions::Model> {
    if state.config.dodo_key.expose_secret().is_empty() || state.config.dodo_plus_product.is_empty()
    {
        return Err(ApiError::Unavailable);
    }
    let (_, sub) = entitlement(db, account).await?;
    if sub.plan != "managed"
        || sub.status != "active"
        || sub.product_id != state.config.dodo_standard_product
    {
        return Err(ApiError::BadRequest(
            "Only an active Managed subscription can upgrade to Managed Plus.".into(),
        ));
    }
    Ok(sub)
}

async fn current(state: &AppState, sub: &subscriptions::Model) -> Result<Value> {
    let response: Value = state
        .http
        .get(
            state
                .config
                .dodo_url
                .join(&format!("subscriptions/{}", sub.subscription_id))
                .map_err(anyhow::Error::from)?,
        )
        .bearer_auth(state.config.dodo_key.expose_secret())
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    if response["metadata"]["account_id"].as_str() != Some(sub.account_id.to_string().as_str())
        || response["subscription_id"] != sub.subscription_id
    {
        return Err(ApiError::Unauthorized);
    }
    Ok(response)
}

async fn preview(state: &AppState, sub: &subscriptions::Model) -> Result<(i64, String)> {
    let active = current(state, sub).await?;
    if active["status"] != "active"
        || active["product_id"] != state.config.dodo_standard_product
        || active["cancel_at_next_billing_date"] == true
    {
        return Err(ApiError::BadRequest("This subscription cannot be upgraded now. Refresh your account or manage it in the billing portal.".into()));
    }
    let value: Value = state
        .http
        .post(
            state
                .config
                .dodo_url
                .join(&format!(
                    "subscriptions/{}/change-plan/preview",
                    sub.subscription_id
                ))
                .map_err(anyhow::Error::from)?,
        )
        .bearer_auth(state.config.dodo_key.expose_secret())
        .timeout(std::time::Duration::from_secs(20))
        .json(&change(state))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let summary = &value["immediate_charge"]["summary"];
    let amount = summary["total_amount"]
        .as_i64()
        .filter(|v| *v >= 0)
        .ok_or(ApiError::Unavailable)?;
    let currency = summary["currency"]
        .as_str()
        .filter(|v| v.len() == 3 && v.bytes().all(|c| c.is_ascii_uppercase()))
        .ok_or(ApiError::Unavailable)?
        .to_owned();
    Ok((amount, currency))
}

pub async fn quote(
    State(state): State<AppState>,
    Extension(p): Extension<Principal>,
) -> Result<Json<Value>> {
    state
        .limit(format!("upgrade-quote:{}", p.account_id), 5, 60)
        .await?;
    let sub = eligible(&state, p.account_id, &state.db).await?;
    let (amount, currency) = preview(&state, &sub).await?;
    let id = Uuid::new_v4();
    plan_upgrades::ActiveModel {
        id: Set(id),
        account_id: Set(p.account_id),
        subscription_id: Set(sub.subscription_id),
        period_start: Set(sub.period_start),
        amount: Set(amount),
        currency: Set(currency.clone()),
        status: Set("quoted".into()),
        response: Set(None),
        expires_at: Set(Utc::now() + chrono::Duration::minutes(5)),
        created_at: Set(Utc::now()),
    }
    .insert(&state.db)
    .await?;
    Ok(Json(
        json!({"quote_id":id,"amount":amount,"currency":currency,"base_difference_cents":crate::plans::PLUS.monthly_cents-crate::plans::STANDARD.monthly_cents,"next_monthly_cents":crate::plans::PLUS.monthly_cents}),
    ))
}

#[derive(Deserialize)]
pub struct Confirmation {
    pub quote_id: Uuid,
}

pub async fn confirm(
    State(state): State<AppState>,
    Extension(p): Extension<Principal>,
    Json(body): Json<Confirmation>,
) -> Result<Json<Value>> {
    state
        .limit(format!("upgrade-confirm:{}", p.account_id), 5, 60)
        .await?;
    let tx = state.db.begin().await?;
    let row = plan_upgrades::Entity::find_by_id(body.quote_id)
        .lock_exclusive()
        .one(&tx)
        .await?
        .ok_or(ApiError::NotFound)?;
    if row.account_id != p.account_id {
        return Err(ApiError::NotFound);
    }
    if row.status == "submitted" {
        return Ok(Json(row.response.unwrap_or(json!({"status":"processing"}))));
    }
    if row.status != "quoted" {
        return Err(ApiError::BadRequest("This upgrade has already been attempted. Check your subscription status before trying again.".into()));
    }
    if row.expires_at <= Utc::now() {
        return Err(ApiError::BadRequest(
            "The quote expired. Review a new upgrade quote.".into(),
        ));
    }
    // Same lock order as webhook processing; serialize confirmations across replicas.
    tx.execute_raw(sea_orm::Statement::from_sql_and_values(
        sea_orm::DbBackend::Postgres,
        "SELECT pg_advisory_xact_lock(hashtextextended($1,0))",
        [row.subscription_id.clone().into()],
    ))
    .await?;
    let pending = plan_upgrades::Entity::find()
        .filter(plan_upgrades::Column::SubscriptionId.eq(&row.subscription_id))
        .filter(plan_upgrades::Column::PeriodStart.eq(row.period_start))
        .filter(plan_upgrades::Column::Status.is_in(["submitting", "submitted"]))
        .one(&tx)
        .await?;
    if pending.is_some() {
        return Err(ApiError::BadRequest("An upgrade is already processing for this billing period. Refresh your account to check its status.".into()));
    }
    let sub = eligible(&state, p.account_id, &tx).await?;
    if sub.subscription_id != row.subscription_id || sub.period_start != row.period_start {
        return Err(ApiError::BadRequest(
            "Your subscription changed. Review a new upgrade quote.".into(),
        ));
    }
    let (amount, currency) = preview(&state, &sub).await?;
    if amount != row.amount || currency != row.currency {
        return Err(ApiError::BadRequest(
            "The charge changed. Review a new upgrade quote before confirming.".into(),
        ));
    }
    let mut attempted: plan_upgrades::ActiveModel = row.clone().into();
    attempted.status = Set("submitting".into());
    attempted.update(&tx).await?;
    tx.commit().await?; // Persist before a billable call: a crash cannot cause a second charge.

    let response = state
        .http
        .post(
            state
                .config
                .dodo_url
                .join(&format!(
                    "subscriptions/{}/change-plan",
                    sub.subscription_id
                ))
                .map_err(anyhow::Error::from)?,
        )
        .bearer_auth(state.config.dodo_key.expose_secret())
        .header("Idempotency-Key", row.id.to_string())
        .timeout(std::time::Duration::from_secs(20))
        .json(&change(&state))
        .send()
        .await;
    let response = match response {
        Ok(response) => response,
        Err(_) => return Err(ApiError::BadRequest("The payment provider did not confirm the upgrade. Its status is uncertain; check your billing portal before retrying. Your request will not be charged again automatically.".into())),
    };
    if !response.status().is_success() {
        if response.status().is_client_error()
            && response.status() != reqwest::StatusCode::REQUEST_TIMEOUT
        {
            let mut failed: plan_upgrades::ActiveModel = row.into();
            failed.status = Set("failed".into());
            failed.update(&state.db).await?;
        }
        return Err(ApiError::BadRequest("The payment provider could not complete this upgrade. Check your payment method and subscription in the billing portal.".into()));
    }
    let payload: Value = response.json().await.unwrap_or(Value::Null);
    let result = json!({"status":"processing","payment_url":payload["payment_link"].as_str()});
    let mut submitted: plan_upgrades::ActiveModel = row.into();
    submitted.status = Set("submitted".into());
    submitted.response = Set(Some(result.clone()));
    submitted.update(&state.db).await?;
    // Only confirmed provider state grants Plus access; failed/pending payments do not.
    {
        let tx = state.db.begin().await?;
        tx.execute_raw(sea_orm::Statement::from_sql_and_values(
            sea_orm::DbBackend::Postgres,
            "SELECT pg_advisory_xact_lock(hashtextextended($1,0))",
            [sub.subscription_id.clone().into()],
        ))
        .await?;
        if let Ok(value) = current(&state, &sub).await
            && value["status"] == "active"
            && value["product_id"] == state.config.dodo_plus_product
        {
            apply_subscription(&state, &tx, &value).await?;
        }
        tx.commit().await?;
    }
    Ok(Json(result))
}
