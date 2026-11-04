CREATE TABLE accounts (
 id uuid PRIMARY KEY, email text NOT NULL, display_name text NOT NULL,
 created_at timestamptz NOT NULL DEFAULT now(), sync_sequence bigint NOT NULL DEFAULT 0,
 storage_bytes bigint NOT NULL DEFAULT 0 CHECK(storage_bytes >= 0)
);
CREATE TABLE identities (
 id uuid PRIMARY KEY, account_id uuid NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
 provider text NOT NULL, subject text NOT NULL, UNIQUE(provider, subject)
);
CREATE TABLE sessions (
 id uuid PRIMARY KEY, account_id uuid NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
 device_id uuid NOT NULL, device_name text NOT NULL,
 access_hash text NOT NULL UNIQUE, refresh_hash text NOT NULL UNIQUE,
 access_expires_at timestamptz NOT NULL, refresh_expires_at timestamptz NOT NULL,
 revoked_at timestamptz, created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX sessions_account ON sessions(account_id);
CREATE TABLE subscriptions (
 account_id uuid PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
 subscription_id text NOT NULL UNIQUE, customer_id text NOT NULL, product_id text NOT NULL,
 plan text NOT NULL, status text NOT NULL,
 period_start timestamptz NOT NULL, period_end timestamptz NOT NULL,
 updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE usage_buckets (
 id uuid PRIMARY KEY, account_id uuid NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
 period_start timestamptz NOT NULL, ai_spent bigint NOT NULL DEFAULT 0 CHECK(ai_spent >= 0),
 speech_seconds bigint NOT NULL DEFAULT 0 CHECK(speech_seconds >= 0),
 UNIQUE(account_id, period_start)
);
CREATE TABLE reservations (
 id uuid PRIMARY KEY, account_id uuid NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
 bucket_id uuid NOT NULL REFERENCES usage_buckets(id) ON DELETE CASCADE,
 kind text NOT NULL, amount bigint NOT NULL CHECK(amount >= 0),
 settled boolean NOT NULL DEFAULT false, created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE records (
 account_id uuid NOT NULL REFERENCES accounts(id) ON DELETE CASCADE, id uuid NOT NULL,
 kind text NOT NULL, revision bigint NOT NULL, deleted boolean NOT NULL,
 data jsonb NOT NULL, device_id uuid NOT NULL, size_bytes bigint NOT NULL,
 PRIMARY KEY(account_id,id)
);
CREATE TABLE changes (
 account_id uuid NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
 sequence bigint NOT NULL, record_id uuid NOT NULL, kind text NOT NULL,
 revision bigint NOT NULL, deleted boolean NOT NULL, data jsonb NOT NULL, device_id uuid NOT NULL,
 PRIMARY KEY(account_id,sequence)
);
CREATE TABLE mutations (
 account_id uuid NOT NULL REFERENCES accounts(id) ON DELETE CASCADE, id uuid NOT NULL,
 request_hash text NOT NULL, response jsonb NOT NULL,
 PRIMARY KEY(account_id,id)
);
CREATE TABLE attachments (
 id uuid PRIMARY KEY, account_id uuid NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
 name text NOT NULL, content_type text NOT NULL, size_bytes bigint NOT NULL,
 sha256 text NOT NULL, created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX attachments_account ON attachments(account_id);
CREATE TABLE webhook_events (
 id text PRIMARY KEY, event_type text NOT NULL, received_at timestamptz NOT NULL DEFAULT now()
);
