//! Real daemon checks run through the integration suite after bun run infra:up.
use lennette_server::{error::ApiError, security::malware};

#[tokio::test]
#[ignore = "requires the local ClamAV Docker profile"]
async fn real_clamav_accepts_clean_bytes_and_blocks_eicar_without_writing_files() {
    let address = std::env::var("TEST_CLAMD_ADDRESS")
        .or_else(|_| std::env::var("CLAMD_ADDRESS"))
        .expect("set CLAMD_ADDRESS");
    malware::scan(
        Some(&address),
        b"Meeting agenda: review the project budget.",
    )
    .await
    .expect("clean document should be accepted");
    use base64::Engine;
    // Standard harmless antivirus test pattern, decoded only in memory.
    let eicar = base64::engine::general_purpose::STANDARD.decode(
        "WDVPIVAlQEFQWzRcUFpYNTQoUF4pN0NDKTd9JEVJQ0FSLVNUQU5EQVJELUFOVElWSVJVUy1URVNULUZJTEUhJEgrSCo="
    ).unwrap();
    assert!(matches!(
        malware::scan(Some(&address), &eicar).await,
        Err(ApiError::UnsafeAttachment)
    ));
}
