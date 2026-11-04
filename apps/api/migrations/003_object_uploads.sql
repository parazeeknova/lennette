-- No account FK: cleanup jobs must survive account deletion and rolled-back uploads.
CREATE TABLE object_uploads (
    account_id uuid NOT NULL,
    attachment_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (account_id, attachment_id)
);
CREATE INDEX object_uploads_age ON object_uploads(created_at);
