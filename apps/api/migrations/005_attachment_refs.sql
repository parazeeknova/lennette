CREATE TABLE attachment_refs (
 account_id uuid NOT NULL,
 record_id uuid NOT NULL,
 attachment_id uuid NOT NULL REFERENCES attachments(id) ON DELETE CASCADE,
 PRIMARY KEY(account_id,record_id,attachment_id),
 FOREIGN KEY(account_id,record_id) REFERENCES records(account_id,id) ON DELETE CASCADE
);
CREATE INDEX attachment_refs_blob ON attachment_refs(attachment_id);
