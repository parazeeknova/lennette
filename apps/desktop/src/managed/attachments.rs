use super::*;
use crate::db::connection::{OptionalExtension, params};
use base64::{Engine, engine::general_purpose::STANDARD};
use lennette_protocol::{Change, Mutation, RecordKind};

pub fn cache_attachment(
    id: &str,
    name: &str,
    content_type: &str,
    bytes: &[u8],
) -> Result<Option<String>, String> {
    let Some(path) = DATABASE.get() else {
        return Ok(None);
    };
    let db = Database::open(path).map_err(|e| e.to_string())?;
    db.connection()
        .execute(
            "INSERT OR IGNORE INTO cloud_blobs(id,name,content_type,payload) VALUES(?1,?2,?3,?4)",
            params![id, name, content_type, STANDARD.encode(bytes)],
        )
        .map_err(|e| e.to_string())?;
    Ok(Some(id.into()))
}
pub(super) async fn upload_pending(client: &Client, db: &Database) -> Result<(), String> {
    let rows:Vec<(String,String,String,String)>=db.connection().prepare("SELECT b.id,b.name,b.content_type,b.payload FROM cloud_blob_queue q JOIN cloud_blobs b ON b.id=q.id LIMIT 4").map_err(|e|e.to_string())?.query_map([],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).map_err(|e|e.to_string())?.collect::<crate::db::Result<_>>().map_err(|e|e.to_string())?;
    let account = client.account_id().await;
    for (id, name, content_type, payload) in rows {
        let remote = Uuid::new_v5(&account, format!("blob:{id}").as_bytes());
        let mut url = client.url("/v1/attachments").map_err(|e| e.to_string())?;
        url.query_pairs_mut()
            .append_pair("id", &remote.to_string())
            .append_pair("name", &name)
            .append_pair("content_type", &content_type);
        let bytes = STANDARD.decode(&payload).map_err(|e| e.to_string())?;
        let token = client.access_token().await.map_err(|e| e.to_string())?;
        let response = client
            .http
            .post(url)
            .bearer_auth(token)
            .body(bytes)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        lennette_sync_client::checked(response)
            .await
            .map_err(|e| e.to_string())?;
        let tx = db
            .connection()
            .unchecked_transaction()
            .map_err(|e| e.to_string())?;
        tx.execute(
            "UPDATE cloud_blobs SET remote_id=?1 WHERE id=?2",
            params![remote.to_string(), id],
        )
        .map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM cloud_blob_queue WHERE id=?1", [id])
            .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
    }
    Ok(())
}
pub(super) fn prepare(db: &Database, mutation: &mut Mutation) -> Result<(), String> {
    if mutation.kind != RecordKind::Context || mutation.deleted {
        return Ok(());
    }
    let Some(raw) = mutation.data["context_json"].as_str() else {
        return Ok(());
    };
    let mut context: Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    if let Some(sources) = context["sources"].as_array_mut() {
        for source in sources {
            if let Some(id) = source["blob_id"].as_str() {
                let remote: Option<String> = db
                    .connection()
                    .query_row("SELECT remote_id FROM cloud_blobs WHERE id=?1", [id], |r| {
                        r.get(0)
                    })
                    .optional()
                    .map_err(|e| e.to_string())?
                    .flatten();
                let remote = remote
                    .ok_or("Uploading attached files before syncing conversation context…")?;
                source["remote_blob_id"] = json!(remote);
                source["image"] = Value::Null;
            } else if source["image"]
                .as_str()
                .is_some_and(|s| s.len() > 128 * 1024)
            {
                return Err("Reattach this older image to sync its original file.".into());
            }
            if source["id"]
                .as_str()
                .is_some_and(|s| s.starts_with("file:"))
            {
                source["id"] = json!(Uuid::new_v5(
                    &Uuid::NAMESPACE_OID,
                    source["id"].as_str().unwrap().as_bytes()
                ));
            }
        }
    }
    mutation.data["context_json"] = json!(context.to_string());
    Ok(())
}
pub(super) async fn hydrate(
    client: &Client,
    db: &Database,
    change: &mut Change,
) -> Result<(), String> {
    if change.kind != RecordKind::Context || change.deleted {
        return Ok(());
    }
    let Some(raw) = change.data["context_json"].as_str() else {
        return Ok(());
    };
    let mut context: Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    if let Some(sources) = context["sources"].as_array_mut() {
        for source in sources {
            let Some(remote) = source["remote_blob_id"].as_str().map(str::to_owned) else {
                continue;
            };
            let id = source["blob_id"].as_str().unwrap_or(&remote).to_owned();
            let cached: Option<(String, String)> = db
                .connection()
                .query_row(
                    "SELECT payload,content_type FROM cloud_blobs WHERE id=?1",
                    [id.clone()],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()
                .map_err(|e| e.to_string())?;
            let (payload, content_type) = match cached {
                Some(value) => value,
                None => {
                    let response = client
                        .request(
                            reqwest::Method::GET,
                            &format!("/v1/attachments/{remote}"),
                            &(),
                        )
                        .await
                        .map_err(|e| e.to_string())?;
                    if response
                        .content_length()
                        .is_some_and(|n| n > 10 * 1024 * 1024)
                    {
                        return Err("attachment exceeds size limit".into());
                    }
                    let content_type = response
                        .headers()
                        .get("content-type")
                        .and_then(|h| h.to_str().ok())
                        .unwrap_or("application/octet-stream")
                        .to_owned();
                    let bytes = response.bytes().await.map_err(|e| e.to_string())?;
                    if bytes.len() > 10 * 1024 * 1024 {
                        return Err("attachment exceeds size limit".into());
                    }
                    let payload = STANDARD.encode(&bytes);
                    db.connection()
                        .execute(
                            "INSERT OR IGNORE INTO cloud_blobs VALUES(?1,?2,?3,?4,?5)",
                            params![
                                id,
                                source["name"].as_str().unwrap_or("Attachment"),
                                content_type,
                                payload,
                                remote
                            ],
                        )
                        .map_err(|e| e.to_string())?;
                    (payload, content_type)
                }
            };
            if content_type.starts_with("image/") {
                let bytes = STANDARD.decode(&payload).map_err(|e| e.to_string())?;
                let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
                    .with_guessed_format()
                    .map_err(|e| e.to_string())?;
                let mut limits = image::Limits::default();
                limits.max_image_width = Some(8192);
                limits.max_image_height = Some(8192);
                limits.max_alloc = Some(128 * 1024 * 1024);
                reader.limits(limits);
                let image = reader
                    .decode()
                    .map_err(|e| e.to_string())?
                    .thumbnail(1024, 1024);
                let mut png = std::io::Cursor::new(Vec::new());
                image
                    .write_to(&mut png, image::ImageFormat::Png)
                    .map_err(|e| e.to_string())?;
                source["image"] = json!(STANDARD.encode(png.into_inner()));
            }
        }
    }
    change.data["context_json"] = json!(context.to_string());
    Ok(())
}
