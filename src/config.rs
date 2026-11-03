use std::env;

#[derive(Clone, Debug)]
pub struct Settings {
    pub pinterest_email: String,
    pub pinterest_password: String,
    pub pinterest_username: String,
    /// JSON object {"name": "value"} seeded into the cookie jar — browser-exported
    /// cookies let you skip the login flow entirely.
    pub pinterest_cookies: Option<String>,
    pub proxy: Option<String>,

    /// comma-separated API keys; empty = open (dev only)
    pub api_keys: Vec<String>,
    pub cred_root: String,
    pub upload_dir: String,
    pub max_pages: u32,
    pub bind: String,
}

impl Settings {
    pub fn from_env() -> Self {
        let env = |k: &str| env::var(k).unwrap_or_default();
        Self {
            pinterest_email: env("PINTEREST_EMAIL"),
            pinterest_password: env("PINTEREST_PASSWORD"),
            pinterest_username: env("PINTEREST_USERNAME"),
            pinterest_cookies: env::var("PINTEREST_COOKIES").ok().filter(|s| !s.is_empty()),
            proxy: env::var("PINTEREST_PROXY").ok().filter(|s| !s.is_empty()),
            api_keys: env("PINAPI_API_KEYS")
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            cred_root: if env("PINAPI_CRED_ROOT").is_empty() {
                "./data/creds".into()
            } else {
                env("PINAPI_CRED_ROOT")
            },
            upload_dir: if env("PINAPI_UPLOAD_DIR").is_empty() {
                "./data/uploads".into()
            } else {
                env("PINAPI_UPLOAD_DIR")
            },
            max_pages: env("PINAPI_MAX_PAGES").parse().unwrap_or(10),
            bind: if env("PINAPI_BIND").is_empty() {
                "0.0.0.0:8000".into()
            } else {
                env("PINAPI_BIND")
            },
        }
    }
}
