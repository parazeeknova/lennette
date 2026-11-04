CREATE FUNCTION lennette_account_event() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE owner_id uuid;
BEGIN
    IF TG_TABLE_NAME = 'accounts' THEN
        IF TG_OP = 'DELETE' THEN owner_id := OLD.id; ELSE owner_id := NEW.id; END IF;
    ELSE
        IF TG_OP = 'DELETE' THEN owner_id := OLD.account_id; ELSE owner_id := NEW.account_id; END IF;
    END IF;
    PERFORM pg_notify('lennette_account_events', owner_id::text);
    RETURN NULL;
END $$;
CREATE TRIGGER account_events AFTER INSERT OR UPDATE OR DELETE ON accounts FOR EACH ROW EXECUTE FUNCTION lennette_account_event();
CREATE TRIGGER session_events AFTER INSERT OR UPDATE OR DELETE ON sessions FOR EACH ROW EXECUTE FUNCTION lennette_account_event();
CREATE TRIGGER subscription_events AFTER INSERT OR UPDATE OR DELETE ON subscriptions FOR EACH ROW EXECUTE FUNCTION lennette_account_event();
CREATE TRIGGER usage_events AFTER INSERT OR UPDATE OR DELETE ON usage_buckets FOR EACH ROW EXECUTE FUNCTION lennette_account_event();
