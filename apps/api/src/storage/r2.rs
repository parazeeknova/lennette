use crate::config::R2Config;
use object_store::{ObjectStore, aws::AmazonS3Builder, client::ClientOptions};
use secrecy::ExposeSecret;
use std::{sync::Arc, time::Duration};

pub fn connect(config: &R2Config) -> anyhow::Result<Arc<dyn ObjectStore>> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let endpoint = &config.endpoint;
    let loopback = endpoint.scheme() == "http"
        && matches!(endpoint.host_str(), Some("127.0.0.1" | "localhost"));
    anyhow::ensure!(
        (endpoint.scheme() == "https" || loopback)
            && endpoint.username().is_empty()
            && endpoint.password().is_none()
            && endpoint.query().is_none()
            && endpoint.fragment().is_none()
            && endpoint.path() == "/",
        "R2_ENDPOINT must be an HTTPS S3 origin (HTTP allowed only for loopback tests)"
    );
    anyhow::ensure!(
        (3..=63).contains(&config.bucket.len())
            && config
                .bucket
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphanumeric)
            && config
                .bucket
                .as_bytes()
                .last()
                .is_some_and(u8::is_ascii_alphanumeric)
            && config
                .bucket
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
        "invalid R2_BUCKET"
    );
    anyhow::ensure!(
        !config.access_key_id.expose_secret().is_empty()
            && !config.secret_access_key.expose_secret().is_empty(),
        "R2 credentials must not be empty"
    );
    let store = AmazonS3Builder::new()
        .with_endpoint(endpoint.as_str().trim_end_matches('/'))
        .with_bucket_name(&config.bucket)
        .with_region("auto")
        .with_access_key_id(config.access_key_id.expose_secret())
        .with_secret_access_key(config.secret_access_key.expose_secret())
        .with_virtual_hosted_style_request(false)
        .with_disable_bulk_delete(true)
        .with_client_options(
            ClientOptions::new()
                .with_allow_http(loopback)
                .with_connect_timeout(Duration::from_secs(10))
                .with_timeout(Duration::from_secs(30)),
        )
        .with_retry(object_store::RetryConfig {
            max_retries: 2,
            retry_timeout: Duration::from_secs(30),
            ..Default::default()
        })
        .build()?;
    Ok(Arc::new(store))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(endpoint: &str) -> R2Config {
        R2Config {
            endpoint: endpoint.parse().unwrap(),
            bucket: "lennette-test".into(),
            access_key_id: "test-access".to_owned().into(),
            secret_access_key: "test-secret".to_owned().into(),
        }
    }

    #[test]
    fn accepts_r2_jurisdictions_and_loopback_but_rejects_unsafe_origins() {
        for endpoint in [
            "https://example.r2.cloudflarestorage.com",
            "https://example.eu.r2.cloudflarestorage.com",
            "http://127.0.0.1:9000",
        ] {
            assert!(connect(&config(endpoint)).is_ok());
        }
        for endpoint in [
            "http://example.r2.cloudflarestorage.com",
            "https://user:password@example.com",
            "https://example.com/bucket",
            "https://example.com/?credential=unsafe",
            "https://example.com/#fragment",
        ] {
            assert!(connect(&config(endpoint)).is_err());
        }
    }

    #[test]
    fn rejects_missing_credentials_and_invalid_buckets() {
        let mut c = config("https://example.r2.cloudflarestorage.com");
        for bucket in ["ab", "../private", "UPPERCASE", "-bad", "bad-"] {
            c.bucket = bucket.into();
            assert!(connect(&c).is_err());
        }
        c.bucket = "lennette-test".into();
        c.secret_access_key = String::new().into();
        assert!(connect(&c).is_err());
    }
}
