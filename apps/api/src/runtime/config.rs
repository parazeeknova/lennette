use anyhow::Context;
use secrecy::{ExposeSecret, SecretString};
use std::{env, net::SocketAddr};
use url::Url;

#[derive(Clone)]
pub struct OAuthConfig {
    pub client_id: String,
    pub client_secret: SecretString,
    /// Explicit endpoints make provider transport testable without real credentials.
    pub token_url: String,
    pub userinfo_url: String,
    pub emails_url: Option<String>,
}

#[derive(Clone)]
pub struct R2Config {
    pub endpoint: Url,
    pub bucket: String,
    pub access_key_id: SecretString,
    pub secret_access_key: SecretString,
}

#[derive(Clone)]
pub struct Config {
    pub bind: SocketAddr,
    pub public_url: Url,
    pub website_url: Url,
    pub database_url: SecretString,
    pub database_url_unpooled: Option<SecretString>,
    pub redis_url: SecretString,
    pub google: Option<OAuthConfig>,
    pub github: Option<OAuthConfig>,
    pub gemini_key: SecretString,
    pub gemini_url: Url,
    pub deepseek_key: SecretString,
    pub deepseek_url: Url,
    pub deepgram_key: SecretString,
    pub deepgram_url: Url,
    pub dodo_key: SecretString,
    pub dodo_webhook_secret: SecretString,
    pub dodo_url: Url,
    pub dodo_standard_product: String,
    pub dodo_plus_product: String,
    pub r2: Option<R2Config>,
    pub clamd_address: Option<String>,
    pub trusted_proxy_ips: Vec<std::net::IpAddr>,
    pub mobile_redirects: Vec<String>,
}
impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let get = |name: &str, fallback: &str| env::var(name).unwrap_or_else(|_| fallback.into());
        let secret = |name: &str| SecretString::from(get(name, ""));
        let oauth = |prefix: &str| {
            let id = get(&format!("{prefix}_CLIENT_ID"), "");
            (!id.is_empty()).then(|| OAuthConfig {
                client_id: id,
                client_secret: secret(&format!("{prefix}_CLIENT_SECRET")),
                token_url: if prefix == "GOOGLE" {
                    "https://oauth2.googleapis.com/token"
                } else {
                    "https://github.com/login/oauth/access_token"
                }
                .into(),
                userinfo_url: if prefix == "GOOGLE" {
                    "https://openidconnect.googleapis.com/v1/userinfo"
                } else {
                    "https://api.github.com/user"
                }
                .into(),
                emails_url: (prefix == "GITHUB")
                    .then(|| "https://api.github.com/user/emails".into()),
            })
        };
        let config = Self {
            bind: get("API_BIND", "127.0.0.1:8090").parse()?,
            public_url: get("API_PUBLIC_URL", "http://127.0.0.1:8090").parse()?,
            website_url: get("WEBSITE_URL", "https://lennette.itssingularity.com").parse()?,
            database_url: SecretString::from(env::var("DATABASE_URL").context(
                "DATABASE_URL is required; use `bun run dev:api` to load development settings",
            )?),
            database_url_unpooled: env::var("DATABASE_URL_UNPOOLED")
                .ok()
                .filter(|url| !url.is_empty())
                .map(SecretString::from),
            redis_url: SecretString::from(env::var("REDIS_URL").context(
                "REDIS_URL is required; use `bun run dev:api` to load development settings",
            )?),
            google: oauth("GOOGLE"),
            github: oauth("GITHUB"),
            gemini_key: secret("GEMINI_API_KEY"),
            gemini_url: get(
                "GEMINI_API_URL",
                "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions",
            )
            .parse()?,
            deepgram_key: secret("DEEPGRAM_API_KEY"),
            deepseek_key: secret("DEEPSEEK_API_KEY"),
            deepseek_url: get("DEEPSEEK_API_URL", "https://parazeeknova--ep-deepseek-v4-1-flash-1017-server.us-west.modal.direct/v1/chat/completions").parse()?,
            deepgram_url: get("DEEPGRAM_API_URL", "wss://api.deepgram.com/v1/listen").parse()?,
            dodo_key: secret("DODO_API_KEY"),
            dodo_webhook_secret: secret("DODO_WEBHOOK_SECRET"),
            dodo_url: get("DODO_API_URL", "https://test.dodopayments.com/").parse()?,
            dodo_standard_product: get("DODO_STANDARD_PRODUCT_ID", ""),
            dodo_plus_product: get("DODO_PLUS_PRODUCT_ID", ""),
            r2: {
                let endpoint = get("R2_ENDPOINT", "");
                let bucket = get("R2_BUCKET", "");
                let access_key_id = secret("R2_ACCESS_KEY_ID");
                let secret_access_key = secret("R2_SECRET_ACCESS_KEY");
                if endpoint.is_empty()
                    && bucket.is_empty()
                    && access_key_id.expose_secret().is_empty()
                    && secret_access_key.expose_secret().is_empty()
                {
                    None
                } else {
                    anyhow::ensure!(
                        !endpoint.is_empty()
                            && !bucket.is_empty()
                            && !access_key_id.expose_secret().is_empty()
                            && !secret_access_key.expose_secret().is_empty(),
                        "R2_ENDPOINT, R2_BUCKET, R2_ACCESS_KEY_ID and R2_SECRET_ACCESS_KEY must all be set"
                    );
                    Some(R2Config {
                        endpoint: endpoint.parse().context("invalid R2_ENDPOINT")?,
                        bucket,
                        access_key_id,
                        secret_access_key,
                    })
                }
            },
            clamd_address: {
                let address = get("CLAMD_ADDRESS", "");
                if address.is_empty() {
                    None
                } else {
                    anyhow::ensure!(
                        address.len() <= 255
                            && address
                                .rsplit_once(':')
                                .is_some_and(|(host, port)| !host.is_empty()
                                    && port.parse::<u16>().is_ok_and(|p| p != 0))
                            && address.chars().all(|c| c.is_ascii_alphanumeric()
                                || matches!(c, '.' | '-' | ':' | '[' | ']')),
                        "CLAMD_ADDRESS must be a private host:port"
                    );
                    Some(address)
                }
            },
            trusted_proxy_ips: get("TRUSTED_PROXY_IPS", "")
                .split(',')
                .filter(|v| !v.trim().is_empty())
                .map(|v| {
                    v.trim()
                        .parse()
                        .context("TRUSTED_PROXY_IPS must contain comma-separated IP addresses")
                })
                .collect::<anyhow::Result<Vec<_>>>()?,
            mobile_redirects: get("MOBILE_REDIRECT_URIS", "")
                .split(',')
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect(),
        };
        for url in [
            &config.public_url,
            &config.website_url,
            &config.gemini_url,
            &config.deepseek_url,
            &config.dodo_url,
        ] {
            anyhow::ensure!(
                url.scheme() == "https"
                    || (url.scheme() == "http"
                        && matches!(url.host_str(), Some("127.0.0.1" | "localhost"))),
                "URLs must use HTTPS except loopback development"
            );
        }
        anyhow::ensure!(
            !config.database_url.expose_secret().is_empty(),
            "DATABASE_URL is required"
        );
        Ok(config)
    }
}
