-- Keyset pagination and account-scoped parent lookups for the browser workspace.
CREATE INDEX records_workspace_list ON records
 (account_id,kind,(LEFT(COALESCE(data->>'updated_at',data->>'started_at',data->>'created_at',''),64)) DESC,id DESC)
 WHERE NOT deleted;
CREATE INDEX records_workspace_conversation ON records
 (account_id,(data->>'conversation_id'),kind,(CASE WHEN jsonb_typeof(data->'timestamp')='number' THEN LEAST(GREATEST((data->>'timestamp')::numeric,0),1e15)::double precision ELSE 0 END) DESC,id DESC)
 WHERE NOT deleted;
CREATE INDEX records_workspace_meeting ON records
 (account_id,(data->>'meeting_id'),kind,(CASE WHEN jsonb_typeof(data->'start_time')='number' THEN LEAST(GREATEST((data->>'start_time')::numeric,0),1e15)::double precision ELSE 0 END) DESC,id DESC)
 WHERE NOT deleted;
CREATE INDEX attachments_workspace_list ON attachments (account_id,created_at DESC,id DESC);
