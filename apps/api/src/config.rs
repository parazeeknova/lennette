use secrecy::{ExposeSecret, SecretString};
use std::{env, net::SocketAddr, path::PathBuf};
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
pub struct Config {
    pub bind: SocketAddr,
    pub public_url: Url,
    pub website_url: Url,
    pub database_url: SecretString,
    pub redis_url: SecretString,
    pub google: Option<OAuthConfig>,
    pub github: Option<OAuthConfig>,
    pub merge_key: SecretString,
    pub merge_url: Url,
    pub deepgram_key: SecretString,
    pub deepgram_url: Url,
    pub dodo_key: SecretString,
    pub dodo_webhook_secret: SecretString,
    pub dodo_url: Url,
    pub dodo_standard_product: String,
    pub dodo_plus_product: String,
    pub attachment_dir: PathBuf,
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
            database_url: SecretString::from(env::var("DATABASE_URL")?),
            redis_url: SecretString::from(env::var("REDIS_URL")?),
            google: oauth("GOOGLE"),
            github: oauth("GITHUB"),
            merge_key: secret("MERGE_API_KEY"),
            merge_url: get(
                "MERGE_API_URL",
                "https://api-gateway.merge.dev/v1/chat/completions",
            )
            .parse()?,
            deepgram_key: secret("DEEPGRAM_API_KEY"),
            deepgram_url: get("DEEPGRAM_API_URL", "wss://api.deepgram.com/v1/listen").parse()?,
            dodo_key: secret("DODO_API_KEY"),
            dodo_webhook_secret: secret("DODO_WEBHOOK_SECRET"),
            dodo_url: get("DODO_API_URL", "https://test.dodopayments.com/").parse()?,
            dodo_standard_product: get("DODO_STANDARD_PRODUCT_ID", ""),
            dodo_plus_product: get("DODO_PLUS_PRODUCT_ID", ""),
            attachment_dir: get("ATTACHMENT_DIR", "./data/attachments").into(),
            mobile_redirects: get("MOBILE_REDIRECT_URIS", "")
                .split(',')
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect(),
        };
        for url in [
            &config.public_url,
            &config.website_url,
            &config.merge_url,
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
