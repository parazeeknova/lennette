//! Transactional offline outbox. SQLite triggers capture writes from every window.
//! Account binding and cursors live in encrypted history, never the credential payload.
use super::{
    DbError, Result,
    connection::{Connection, OptionalExtension, ToValue, params},
};
use lennette_protocol::{Change, Mutation, RecordKind};
use serde_json::{Value, json};
use uuid::Uuid;

const TABLES: &[(&str, RecordKind, &str)] = &[
    ("conversations", RecordKind::Conversation, "id"),
    ("messages", RecordKind::Message, "id"),
    ("meetings", RecordKind::Meeting, "id"),
    ("meeting_lines", RecordKind::Transcript, "id"),
    ("system_prompts", RecordKind::Prompt, "id"),
    ("context_profiles", RecordKind::Profile, "id"),
    ("files", RecordKind::Attachment, "id"),
    ("meeting_recaps", RecordKind::Recap, "meeting_id"),
    ("conversation_state", RecordKind::Context, "conversation_id"),
    ("saved_notes", RecordKind::Note, "kind,ref_id"),
    (
        "meeting_speakers",
        RecordKind::Speaker,
        "meeting_id,speaker_key",
    ),
    ("cached_summaries", RecordKind::Summary, "kind,ref_id,mode"),
    (
        "prompt_files",
        RecordKind::PromptAttachment,
        "prompt_id,file_id",
    ),
    (
        "meeting_conversations",
        RecordKind::MeetingConversation,
        "meeting_id,conversation_id",
    ),
    ("meeting_sentiment", RecordKind::Analysis, "meeting_id"),
];
const PREFS: &str = "'theme','response_length','response_language','chat_model','listen_mode','auto_answer','answer_pacing'";
pub fn install(db: &Connection) -> Result<()> {
    db.execute_batch("CREATE TABLE IF NOT EXISTS cloud_meta(key TEXT PRIMARY KEY,value TEXT NOT NULL);
      INSERT OR IGNORE INTO cloud_meta VALUES('applying','0');
      CREATE TABLE IF NOT EXISTS cloud_outbox(seq INTEGER PRIMARY KEY AUTOINCREMENT,table_name TEXT NOT NULL,local_id TEXT NOT NULL,payload TEXT NOT NULL,deleted INTEGER NOT NULL,generation TEXT NOT NULL,UNIQUE(table_name,local_id));
      CREATE TABLE IF NOT EXISTS cloud_revisions(cloud_id TEXT PRIMARY KEY,revision INTEGER NOT NULL,table_name TEXT NOT NULL,local_id TEXT NOT NULL,UNIQUE(table_name,local_id));
      CREATE TABLE IF NOT EXISTS cloud_conflicts(cloud_id TEXT PRIMARY KEY,payload TEXT NOT NULL);
      CREATE TABLE IF NOT EXISTS cloud_blobs(id TEXT PRIMARY KEY,name TEXT NOT NULL,content_type TEXT NOT NULL,payload TEXT NOT NULL,remote_id TEXT);
      CREATE TABLE IF NOT EXISTS cloud_blob_queue(id TEXT PRIMARY KEY REFERENCES cloud_blobs(id) ON DELETE CASCADE);
      CREATE TRIGGER IF NOT EXISTS cloud_blob_insert AFTER INSERT ON cloud_blobs
      WHEN (SELECT value FROM cloud_meta WHERE key='enabled')='1' AND NEW.remote_id IS NULL
      BEGIN INSERT OR IGNORE INTO cloud_blob_queue VALUES(NEW.id); END;
    ")?;
    for &(table, _, pk) in TABLES {
        let columns = columns(db, table)?;
        for (action, prefix, deleted) in [
            ("INSERT", "NEW", 0),
            ("UPDATE", "NEW", 0),
            ("DELETE", "OLD", 1),
        ] {
            let fields = columns
                .iter()
                .flat_map(|c| [format!("'{c}'"), format!("{prefix}.{c}")])
                .collect::<Vec<_>>()
                .join(",");
            let payload = format!("json_object('_table','{table}',{fields})");
            let key = if pk.contains(',') {
                format!(
                    "json_array({})",
                    pk.split(',')
                        .map(|k| format!("{prefix}.{k}"))
                        .collect::<Vec<_>>()
                        .join(",")
                )
            } else {
                format!("CAST({prefix}.{pk} AS TEXT)")
            };
            db.execute_batch(&format!("CREATE TRIGGER IF NOT EXISTS cloud_{table}_{action} AFTER {action} ON {table}
             WHEN (SELECT value FROM cloud_meta WHERE key='enabled')='1' AND (SELECT value FROM cloud_meta WHERE key='applying')='0'
             BEGIN INSERT INTO cloud_outbox(table_name,local_id,payload,deleted,generation) VALUES('{table}',{key},{payload},{deleted},lower(hex(randomblob(16))))
             ON CONFLICT(table_name,local_id) DO UPDATE SET payload=excluded.payload,deleted=excluded.deleted,generation=excluded.generation; END;"))?;
        }
    }
    for (action, prefix, deleted) in [
        ("INSERT", "NEW", 0),
        ("UPDATE", "NEW", 0),
        ("DELETE", "OLD", 1),
    ] {
        db.execute_batch(&format!("CREATE TRIGGER IF NOT EXISTS cloud_settings_{action} AFTER {action} ON app_settings
        WHEN {prefix}.key IN ({PREFS}) AND (SELECT value FROM cloud_meta WHERE key='enabled')='1' AND (SELECT value FROM cloud_meta WHERE key='applying')='0'
        BEGIN INSERT INTO cloud_outbox(table_name,local_id,payload,deleted,generation) VALUES('app_settings',{prefix}.key,json_object({prefix}.key,{prefix}.value),{deleted},lower(hex(randomblob(16))))
        ON CONFLICT(table_name,local_id) DO UPDATE SET payload=excluded.payload,deleted=excluded.deleted,generation=excluded.generation; END;"))?;
    }
    Ok(())
}
fn columns(db: &Connection, table: &str) -> Result<Vec<String>> {
    db.prepare(&format!("PRAGMA table_info({table})"))?
        .query_map([], |r| r.get(1))?
        .collect()
}
pub fn meta(db: &Connection, key: &str) -> Result<Option<String>> {
    db.query_row("SELECT value FROM cloud_meta WHERE key=?1", [key], |r| {
        r.get(0)
    })
    .optional()
}
pub fn set_meta(db: &Connection, key: &str, value: &str) -> Result<()> {
    db.execute(
        "INSERT INTO cloud_meta VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, value],
    )?;
    Ok(())
}
pub fn bind(db: &Connection, account: Uuid, import: bool) -> Result<()> {
    let tx = db.unchecked_transaction()?;
    if let Some(bound) = meta(&tx, "account")?
        && bound != account.to_string()
    {
        return Err(DbError::Storage(
            "this local history belongs to another account; use a separate device profile",
        ));
    }
    let first = meta(&tx, "account")?.is_none();
    set_meta(&tx, "account", &account.to_string())?;
    set_meta(&tx, "enabled", "1")?;
    if first && import {
        tx.execute_batch("INSERT OR IGNORE INTO cloud_blob_queue SELECT id FROM cloud_blobs WHERE remote_id IS NULL;")?;
        for &(table, _, pk) in TABLES {
            let fields = columns(&tx, table)?
                .iter()
                .flat_map(|c| [format!("'{c}'"), c.clone()])
                .collect::<Vec<_>>()
                .join(",");
            let key = if pk.contains(',') {
                format!("json_array({pk})")
            } else {
                format!("CAST({pk} AS TEXT)")
            };
            tx.execute_batch(&format!("INSERT OR IGNORE INTO cloud_outbox(table_name,local_id,payload,deleted,generation) SELECT '{table}',{key},json_object('_table','{table}',{fields}),0,lower(hex(randomblob(16))) FROM {table};"))?;
        }
        tx.execute_batch(&format!("INSERT OR IGNORE INTO cloud_outbox(table_name,local_id,payload,deleted,generation) SELECT 'app_settings',key,json_object(key,value),0,lower(hex(randomblob(16))) FROM app_settings WHERE key IN ({PREFS});"))?;
    }
    tx.commit()
}
fn cloud_id(account: Uuid, table: &str, id: &str) -> Uuid {
    Uuid::parse_str(id)
        .unwrap_or_else(|_| Uuid::new_v5(&account, format!("{table}:{id}").as_bytes()))
}
#[derive(Clone)]
pub struct Pending {
    pub table: String,
    pub local_id: String,
    pub generation: String,
    pub mutation: Mutation,
}
pub fn pending(db: &Connection, account: Uuid) -> Result<Vec<Pending>> {
    // New messages may belong to conversations created before sign-in. Queue only
    // the required parent metadata; existing messages remain local unless imported.
    for (table, field, children) in [
        (
            "conversations",
            "conversation_id",
            "'messages','conversation_state'",
        ),
        ("meetings", "meeting_id", "'meeting_lines','meeting_recaps'"),
    ] {
        let fields = columns(db, table)?
            .iter()
            .flat_map(|c| [format!("'{c}'"), format!("p.{c}")])
            .collect::<Vec<_>>()
            .join(",");
        db.execute_batch(&format!("INSERT OR IGNORE INTO cloud_outbox(table_name,local_id,payload,deleted,generation) SELECT '{table}',p.id,json_object('_table','{table}',{fields}),0,lower(hex(randomblob(16))) FROM {table} p WHERE p.id IN (SELECT json_extract(payload,'$.{field}') FROM cloud_outbox WHERE table_name IN ({children}) AND deleted=0) AND NOT EXISTS(SELECT 1 FROM cloud_revisions r WHERE r.table_name='{table}' AND r.local_id=p.id);"))?;
    }
    let rows:Vec<(String,String,String,i64,String)>=db.prepare("SELECT table_name,local_id,payload,deleted,generation FROM cloud_outbox ORDER BY CASE table_name WHEN 'conversations' THEN 0 WHEN 'meetings' THEN 0 WHEN 'system_prompts' THEN 0 WHEN 'context_profiles' THEN 0 WHEN 'files' THEN 0 WHEN 'conversation_state' THEN 2 ELSE 1 END,seq LIMIT 100")?.query_map([],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))?.collect::<Result<_>>()?;
    rows.into_iter()
        .map(|(table, id, payload, deleted, generation)| {
            let uuid = cloud_id(account, &table, &id);
            let revision = db
                .query_row(
                    "SELECT revision FROM cloud_revisions WHERE cloud_id=?1",
                    [uuid.to_string()],
                    |r| r.get(0),
                )
                .optional()?
                .unwrap_or(0);
            let kind = if table == "app_settings" {
                RecordKind::Settings
            } else {
                TABLES
                    .iter()
                    .find(|(t, _, _)| *t == table)
                    .ok_or(DbError::Storage("unknown outbox table"))?
                    .1
            };
            let data = serde_json::from_str(&payload)
                .map_err(|_| DbError::Storage("invalid outbox data"))?;
            let mutation = Mutation {
                mutation_id: Uuid::parse_str(&generation)
                    .map_err(|_| DbError::Storage("invalid outbox id"))?,
                id: uuid,
                kind,
                base_revision: revision,
                deleted: deleted != 0,
                data,
            };
            Ok(Pending {
                table,
                local_id: id,
                generation,
                mutation,
            })
        })
        .collect()
}
pub fn acknowledge(db: &Connection, pending: &Pending, change: &Change) -> Result<()> {
    let tx = db.unchecked_transaction()?;
    tx.execute("INSERT INTO cloud_revisions VALUES(?1,?2,?3,?4) ON CONFLICT(cloud_id) DO UPDATE SET revision=excluded.revision",params![change.id.to_string(),change.revision,pending.table,pending.local_id])?;
    tx.execute(
        "DELETE FROM cloud_outbox WHERE table_name=?1 AND local_id=?2 AND generation=?3",
        params![pending.table, pending.local_id, pending.generation],
    )?;
    tx.commit()
}
pub fn apply(db: &Connection, change: &Change) -> Result<()> {
    let tx = db.unchecked_transaction()?;
    let prior: Option<i64> = tx
        .query_row(
            "SELECT revision FROM cloud_revisions WHERE cloud_id=?1",
            [change.id.to_string()],
            |r| r.get(0),
        )
        .optional()?;
    if prior.is_some_and(|r| r >= change.revision) {
        return tx.commit();
    }
    let (table, pk, id, data) = if change.kind == RecordKind::Settings {
        let object = change
            .data
            .as_object()
            .ok_or(DbError::Storage("invalid remote settings"))?;
        let (key, value) = object
            .iter()
            .next()
            .ok_or(DbError::Storage("empty remote settings"))?;
        let data = json!({"key":key,"value":value.as_str().unwrap_or_default()});
        ("app_settings", "key", key.clone(), data)
    } else {
        let table = change.data["_table"]
            .as_str()
            .or_else(|| {
                TABLES
                    .iter()
                    .find(|(_, k, _)| *k == change.kind)
                    .map(|(t, _, _)| *t)
            })
            .ok_or(DbError::Storage("invalid remote record"))?;
        let &(table, _, pk) = TABLES
            .iter()
            .find(|(t, k, _)| *t == table && *k == change.kind)
            .ok_or(DbError::Storage("unknown remote table"))?;
        let mapped: Option<String> = tx
            .query_row(
                "SELECT local_id FROM cloud_revisions WHERE cloud_id=?1",
                [change.id.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        let id = if pk.contains(',') && pk.split(',').all(|k| !change.data[k].is_null()) {
            Some(Value::Array(pk.split(',').map(|k| change.data[k].clone()).collect()).to_string())
        } else {
            change.data[pk]
                .as_str()
                .map(str::to_owned)
                .or_else(|| change.data[pk].as_i64().map(|n| n.to_string()))
        }
        .or(mapped)
        .unwrap_or_else(|| change.id.to_string());
        (table, pk, id, change.data.clone())
    };
    let mut pending: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM cloud_outbox WHERE table_name=?1 AND local_id=?2)",
        params![table, id],
        |r| r.get(0),
    )?;
    if change.deleted && matches!(table, "conversations" | "meetings") {
        let field = if table == "conversations" {
            "conversation_id"
        } else {
            "meeting_id"
        };
        let children:bool=tx.query_row(&format!("SELECT EXISTS(SELECT 1 FROM cloud_outbox WHERE json_extract(payload,'$.{field}')=?1)"),[id.clone()],|r|r.get(0))?;
        pending |= children;
    }
    if pending {
        // Preserve both versions. Never silently overwrite an unsynced local edit.
        tx.execute("INSERT INTO cloud_conflicts VALUES(?1,?2) ON CONFLICT(cloud_id) DO UPDATE SET payload=excluded.payload",params![change.id.to_string(),serde_json::to_string(change).map_err(|_|DbError::Storage("invalid change"))?])?;
        return tx.commit();
    }
    set_meta(&tx, "applying", "1")?;
    if change.deleted {
        let keys = pk.split(',').collect::<Vec<_>>();
        let values = if keys.len() == 1 {
            vec![id.to_value()]
        } else {
            serde_json::from_str::<Vec<Value>>(&id)
                .map_err(|_| DbError::Storage("invalid composite key"))?
                .iter()
                .map(sql_value)
                .collect()
        };
        let predicate = keys
            .iter()
            .enumerate()
            .map(|(i, k)| format!("{k}=?{}", i + 1))
            .collect::<Vec<_>>()
            .join(" AND ");
        tx.execute(&format!("DELETE FROM {table} WHERE {predicate}"), values)?;
    } else {
        let columns = columns(&tx, table)?
            .into_iter()
            .filter(|c| data.get(c).is_some())
            .collect::<Vec<_>>();
        let values = columns
            .iter()
            .map(|c| sql_value(&data[c]))
            .collect::<Vec<_>>();
        let markers = (1..=columns.len())
            .map(|i| format!("?{i}"))
            .collect::<Vec<_>>()
            .join(",");
        let updates = columns
            .iter()
            .filter(|c| !pk.split(',').any(|key| key == c.as_str()))
            .map(|c| format!("{c}=excluded.{c}"))
            .collect::<Vec<_>>()
            .join(",");
        let action = if updates.is_empty() {
            "DO NOTHING".to_owned()
        } else {
            format!("DO UPDATE SET {updates}")
        };
        tx.execute(
            &format!(
                "INSERT INTO {table}({}) VALUES({markers}) ON CONFLICT({pk}) {action}",
                columns.join(",")
            ),
            values,
        )?;
    }
    tx.execute("INSERT INTO cloud_revisions VALUES(?1,?2,?3,?4) ON CONFLICT(cloud_id) DO UPDATE SET revision=excluded.revision",params![change.id.to_string(),change.revision,table,id])?;
    set_meta(&tx, "applying", "0")?;
    tx.commit()
}
pub fn cursor(db: &Connection) -> Result<i64> {
    Ok(meta(db, "cursor")?
        .and_then(|s| s.parse().ok())
        .unwrap_or(0))
}
fn sql_value(value: &Value) -> sea_orm::Value {
    match value {
        Value::Null => sea_orm::Value::String(None),
        Value::String(s) => s.to_value(),
        Value::Bool(b) => b.to_value(),
        Value::Number(n) => n
            .as_i64()
            .map(|n| n.to_value())
            .unwrap_or_else(|| n.as_f64().unwrap_or_default().to_value()),
        other => other.to_string().to_value(),
    }
}

pub fn resolve_conflicts(db: &Connection, keep_local: bool) -> Result<()> {
    let mut conflicts: Vec<(String, String)> = db
        .prepare("SELECT cloud_id,payload FROM cloud_conflicts")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_>>()?;
    conflicts.sort_by_key(|(_, payload)| {
        let parent = serde_json::from_str::<Change>(payload)
            .is_ok_and(|c| matches!(c.kind, RecordKind::Conversation | RecordKind::Meeting));
        if keep_local { !parent } else { parent }
    });
    for (id, payload) in conflicts {
        let change: Change =
            serde_json::from_str(&payload).map_err(|_| DbError::Storage("invalid conflict"))?;
        let pending = pending(
            db,
            meta(db, "account")?
                .and_then(|s| s.parse().ok())
                .ok_or(DbError::Storage("account not bound"))?,
        )?
        .into_iter()
        .find(|p| p.mutation.id == change.id);
        if let Some(p) = pending {
            if keep_local {
                let tx = db.unchecked_transaction()?;
                tx.execute("INSERT INTO cloud_revisions VALUES(?1,?2,?3,?4) ON CONFLICT(cloud_id) DO UPDATE SET revision=excluded.revision",params![id,change.revision,p.table,p.local_id])?;
                tx.execute("UPDATE cloud_outbox SET generation=lower(hex(randomblob(16))) WHERE table_name=?1 AND local_id=?2",params![p.table,p.local_id])?;
                tx.execute("DELETE FROM cloud_conflicts WHERE cloud_id=?1", [id])?;
                tx.commit()?;
            } else {
                db.execute(
                    "DELETE FROM cloud_outbox WHERE table_name=?1 AND local_id=?2",
                    params![p.table, p.local_id],
                )?;
                apply(db, &change)?;
                db.execute("DELETE FROM cloud_conflicts WHERE cloud_id=?1", [id])?;
            }
        } else {
            if keep_local {
                let mapping: Option<(String, String)> = db
                    .query_row(
                        "SELECT table_name,local_id FROM cloud_revisions WHERE cloud_id=?1",
                        [id.clone()],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .optional()?;
                if let Some((table, local_id)) = mapping {
                    let &(table, _, pk) = TABLES
                        .iter()
                        .find(|(t, _, _)| *t == table)
                        .ok_or(DbError::Storage("unknown conflict table"))?;
                    let keys = pk.split(',').collect::<Vec<_>>();
                    let values = if keys.len() == 1 {
                        vec![local_id.to_value()]
                    } else {
                        serde_json::from_str::<Vec<Value>>(&local_id)
                            .map_err(|_| DbError::Storage("invalid conflict key"))?
                            .iter()
                            .map(sql_value)
                            .collect()
                    };
                    let predicate = keys
                        .iter()
                        .enumerate()
                        .map(|(i, k)| format!("{k}=?{}", i + 1))
                        .collect::<Vec<_>>()
                        .join(" AND ");
                    let fields = columns(db, table)?
                        .iter()
                        .flat_map(|c| [format!("'{c}'"), c.clone()])
                        .collect::<Vec<_>>()
                        .join(",");
                    let payload:Option<String>=db.query_row(&format!("SELECT json_object('_table','{table}',{fields}) FROM {table} WHERE {predicate}"),values,|r|r.get(0)).optional()?;
                    if let Some(payload) = payload {
                        let tx = db.unchecked_transaction()?;
                        tx.execute(
                            "UPDATE cloud_revisions SET revision=?1 WHERE cloud_id=?2",
                            params![change.revision, id],
                        )?;
                        tx.execute("INSERT OR IGNORE INTO cloud_outbox(table_name,local_id,payload,deleted,generation) VALUES(?1,?2,?3,0,?4)",params![table,local_id,payload,Uuid::new_v4().simple().to_string()])?;
                        tx.commit()?;
                    }
                }
            } else {
                let mapping: Option<(String, String)> = db
                    .query_row(
                        "SELECT table_name,local_id FROM cloud_revisions WHERE cloud_id=?1",
                        [id.clone()],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .optional()?;
                if let Some((table, local_id)) = mapping {
                    db.execute(
                        "DELETE FROM cloud_outbox WHERE table_name=?1 AND local_id=?2",
                        params![table, local_id],
                    )?;
                }
                apply(db, &change)?;
            }
            db.execute("DELETE FROM cloud_conflicts WHERE cloud_id=?1", [id])?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn offline_writes_capture_after_consent_and_credentials_stay_local() {
        let db = crate::db::Database::in_memory().unwrap();
        let c = db.connection();
        crate::db::conversations::create(c, "Before", 1).unwrap();
        assert!(pending(c, Uuid::nil()).unwrap().is_empty());
        let account = Uuid::new_v4();
        bind(c, account, false).unwrap();
        crate::db::conversations::create(c, "After", 2).unwrap();
        crate::db::settings::set(c, "api_key", "secret").unwrap();
        assert_eq!(pending(c, account).unwrap().len(), 1);
        assert!(bind(c, Uuid::new_v4(), true).is_err());
    }
    #[test]
    fn remote_changes_do_not_echo_and_conflicts_keep_local_edits() {
        let db = crate::db::Database::in_memory().unwrap();
        let c = db.connection();
        let account = Uuid::new_v4();
        bind(c, account, false).unwrap();
        let id = Uuid::new_v4();
        let change = Change {
            sequence: 1,
            id,
            kind: RecordKind::Conversation,
            revision: 1,
            deleted: false,
            data: json!({"_table":"conversations","id":id.to_string(),"title":"Remote","created_at":1,"updated_at":1}),
            device_id: Uuid::new_v4(),
        };
        apply(c, &change).unwrap();
        assert!(pending(c, account).unwrap().is_empty());
        crate::db::conversations::rename(c, &id.to_string(), "Local").unwrap();
        let mut next = change.clone();
        next.revision = 2;
        apply(c, &next).unwrap();
        assert_eq!(
            crate::db::conversations::list(c, 10).unwrap()[0].title,
            "Local"
        );
        let conflicts: i64 = c
            .query_row("SELECT count(*) FROM cloud_conflicts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(conflicts, 1);
    }
}
