CREATE TABLE plan_upgrades (
    id uuid PRIMARY KEY,
    account_id uuid NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    subscription_id text NOT NULL,
    period_start timestamptz NOT NULL,
    amount bigint NOT NULL CHECK (amount >= 0),
    currency text NOT NULL,
    status text NOT NULL CHECK (status IN ('quoted','submitting','submitted','failed')),
    response jsonb,
    expires_at timestamptz NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);
-- An ambiguous payment attempt must never be automatically charged again.
CREATE UNIQUE INDEX plan_upgrade_once ON plan_upgrades(subscription_id, period_start)
    WHERE status IN ('submitting', 'submitted');
CREATE INDEX plan_upgrades_account ON plan_upgrades(account_id);
