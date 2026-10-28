//! Application identity generated from Cargo.toml for every desktop build.
include!(concat!(env!("OUT_DIR"), "/identity.rs"));

/// Shared data location. Move pre-identity-change data as one directory so the
/// encrypted vault and its key always travel together. Never overwrite a target.
pub fn data_dir() -> Option<std::path::PathBuf> {
    let directory = directories::ProjectDirs::from("com", "itssingularity", "lennette")?
        .data_dir()
        .to_path_buf();
    // This historical tuple is solely for migrating existing installations.
    let legacy = directories::ProjectDirs::from("dev", "lennette", "lennette")?
        .data_dir()
        .to_path_buf();
    migrate_legacy_data(directory, legacy)
}

fn migrate_legacy_data(
    directory: std::path::PathBuf,
    legacy: std::path::PathBuf,
) -> Option<std::path::PathBuf> {
    if directory != legacy && !directory.exists() && legacy.is_dir() {
        if let Some(parent) = directory.parent() {
            std::fs::create_dir_all(parent).ok()?;
        }
        if let Err(error) = std::fs::rename(&legacy, &directory)
            && !directory.exists()
        {
            tracing::warn!(%error, "could not migrate legacy storage; keeping the existing vault and history");
            return Some(legacy);
        }
    }
    Some(directory)
}

pub fn info() -> serde_json::Value {
    serde_json::json!({
        "identifier": APP_ID, "name": NAME, "version": VERSION,
        "description": DESCRIPTION, "publisher": PUBLISHER, "website": WEBSITE,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity_migration_keeps_the_vault_key_and_history_together_and_never_overwrites() {
        let root =
            std::env::temp_dir().join(format!("lennette-migration-{}", uuid::Uuid::new_v4()));
        let old = root.join("old");
        let new = root.join("new/data");
        std::fs::create_dir_all(&old).unwrap();
        for name in ["vault.key", "vault.bin", "lennette.db"] {
            std::fs::write(old.join(name), name).unwrap();
        }
        assert_eq!(
            migrate_legacy_data(new.clone(), old.clone()),
            Some(new.clone())
        );
        assert!(!old.exists());
        for name in ["vault.key", "vault.bin", "lennette.db"] {
            assert_eq!(std::fs::read_to_string(new.join(name)).unwrap(), name);
        }
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("vault.key"), "legacy key").unwrap();
        assert_eq!(
            migrate_legacy_data(new.clone(), old.clone()),
            Some(new.clone())
        );
        assert_eq!(
            std::fs::read_to_string(new.join("vault.key")).unwrap(),
            "vault.key"
        );
        assert_eq!(
            std::fs::read_to_string(old.join("vault.key")).unwrap(),
            "legacy key"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn identity_uses_the_cargo_version_and_description() {
        assert_eq!(VERSION, env!("CARGO_PKG_VERSION"));
        assert_eq!(DESCRIPTION, env!("CARGO_PKG_DESCRIPTION"));
        assert_eq!(APP_ID, "com.itssingularity.lennette");
        assert_eq!(NAME, "Lennette");
        assert_eq!(PUBLISHER, "Singularity Works");
        assert_eq!(WEBSITE, "https://itssingularity.com");
    }
}
