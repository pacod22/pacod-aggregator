//! The server's settings, from the environment (`AGG_*`). `deploy/.env.example` lists them; a test
//! checks that the two agree.
//!
//! Secrets come from a variable or from the file a `_FILE` variable names (a mounted secret);
//! both set is refused, so that it is clear which one is used. They are never command-line
//! arguments, where `ps` would show them.

use std::net::SocketAddr;

use anyhow::{Context, bail, ensure};

/// A shorter development token is refused.
const MIN_DEV_TOKEN: usize = 32;
/// A shorter internal token is refused.
const MIN_INTERNAL_TOKEN: usize = 16;

/// Every variable the server reads, for the check that `deploy/.env.example` names them all.
pub const VARIABLES: &[&str] = &[
    "AGG_LISTEN",
    "AGG_ADMIN_LISTEN",
    "AGG_METRICS_LISTEN",
    "AGG_AUTH",
    "AGG_DEV_TOKEN",
    "AGG_ID_ISSUER",
    "AGG_ID_INTERNAL_URL",
    "AGG_ID_CLIENT_ID",
    "AGG_ID_CLIENT_SECRET",
    "AGG_ID_AUDIENCE",
    "AGG_ID_MAX_STALENESS_SECONDS",
    "AGG_CATALOG_PUBLIC",
    "AGG_INTERNAL_TOKEN",
    "AGG_LLM_URL",
    "AGG_LLM_KEY",
    "AGG_LOG_FORMAT",
];

pub type Env<'a> = &'a dyn Fn(&str) -> Option<String>;

fn set(env: Env, name: &str) -> Option<String> {
    env(name).map(|v| v.trim().to_owned()).filter(|v| !v.is_empty())
}

/// `NAME`, or the content of the file `NAME_FILE` names.
pub fn secret(env: Env, name: &str) -> anyhow::Result<Option<String>> {
    let file = format!("{name}_FILE");
    match (set(env, name), set(env, &file)) {
        (Some(_), Some(_)) => bail!("{name} and {file} are both set; set one of them"),
        (Some(v), None) => Ok(Some(v)),
        (None, Some(path)) => {
            let v = std::fs::read_to_string(&path).with_context(|| format!("{file}: reading {path}"))?;
            let v = v.trim().to_owned();
            ensure!(!v.is_empty(), "{file}: {path} is empty");
            Ok(Some(v))
        }
        (None, None) => Ok(None),
    }
}

/// `host:port`, the host an address or a name.
fn check_listen(name: &str, text: &str) -> anyhow::Result<()> {
    if text.parse::<SocketAddr>().is_ok() {
        return Ok(());
    }
    let (host, port) = text.rsplit_once(':').with_context(|| format!("{name}: {text:?} is not host:port"))?;
    ensure!(!host.is_empty() && port.parse::<u16>().is_ok(), "{name}: {text:?} is not host:port");
    Ok(())
}

fn listen(env: Env, name: &str, default: &str) -> anyhow::Result<String> {
    let v = set(env, name).unwrap_or_else(|| default.to_owned());
    check_listen(name, &v)?;
    Ok(v)
}

/// http(s), a host, no user name or password (they would end up in logs), no query or fragment.
pub fn check_url(name: &str, text: &str) -> anyhow::Result<String> {
    let u = url::Url::parse(text).with_context(|| format!("{name}: not a URL"))?;
    ensure!(
        matches!(u.scheme(), "http" | "https") && u.host_str().is_some_and(|h| !h.is_empty()),
        "{name}: {text} is not http(s) with a host"
    );
    ensure!(u.username().is_empty() && u.password().is_none(), "{name}: the URL may not hold a user name or password");
    ensure!(u.query().is_none() && u.fragment().is_none(), "{name}: the URL may not hold a query or a fragment");
    Ok(text.trim_end_matches('/').to_owned())
}

fn flag(env: Env, name: &str, default: bool) -> anyhow::Result<bool> {
    Ok(match set(env, name).as_deref() {
        None => default,
        Some("true" | "1" | "yes" | "on") => true,
        Some("false" | "0" | "no" | "off") => false,
        Some(other) => bail!("{name} must be true or false, not {other:?}"),
    })
}

/// pacod-id, for the people who sign in to the console and the services that read the catalogue.
#[derive(Clone)]
pub struct IdSettings {
    /// The public issuers whose tokens the server takes, exactly as their `iss` claims carry them.
    pub issuers: Vec<String>,
    /// pacod-id's internal listener: JWKS, the feed, the catalogue of roles.
    pub internal_url: String,
    pub client_id: String,
    pub client_secret: String,
    pub audience: String,
    /// Older than this, the copy of the bindings is not trusted and calls answer 503.
    pub max_staleness_s: u64,
}

/// How callers are checked.
#[derive(Clone)]
pub enum Auth {
    /// Tokens of pacod-id, roles from its feed: every installation.
    Oidc(IdSettings),
    /// One static token that holds every aggregator role: development only, without pacod-id.
    Dev { token: String },
}

/// Where the extraction model is reached: an OpenAI-compatible endpoint, in the family a gateway
/// of pacod-gw with a key of its own. The model's name is a setting; the address and the key are
/// the operator's, here alone, because the key goes to that address with every call.
#[derive(Clone)]
pub struct LlmLink {
    pub url: String,
    pub key: Option<String>,
}

#[derive(Clone)]
pub struct Config {
    pub listen: String,
    pub admin_listen: String,
    pub metrics_listen: Option<String>,
    pub database_url: String,
    pub auth: Auth,
    /// The catalogue is open to anyone, without a token.
    pub catalog_public: bool,
    /// What the scraper presents on the internal API. Unset: the internal API answers no one.
    pub internal_token: Option<String>,
    pub llm: Option<LlmLink>,
}

fn id_settings(env: Env) -> anyhow::Result<IdSettings> {
    let issuers: Vec<String> = set(env, "AGG_ID_ISSUER")
        .context("AGG_AUTH=oidc needs AGG_ID_ISSUER")?
        .split(',')
        .map(|i| i.trim().to_owned())
        .filter(|i| !i.is_empty())
        .collect();
    ensure!(!issuers.is_empty(), "AGG_ID_ISSUER names no issuer");
    let internal_url = set(env, "AGG_ID_INTERNAL_URL").context("AGG_AUTH=oidc needs AGG_ID_INTERNAL_URL")?;
    let max_staleness_s = match set(env, "AGG_ID_MAX_STALENESS_SECONDS") {
        None => 120,
        Some(v) => v.parse().map_err(|_| anyhow::anyhow!("AGG_ID_MAX_STALENESS_SECONDS: {v:?} is not a number"))?,
    };
    Ok(IdSettings {
        issuers,
        internal_url: check_url("AGG_ID_INTERNAL_URL", &internal_url)?,
        client_id: set(env, "AGG_ID_CLIENT_ID").unwrap_or_else(|| "pacod-agg-service".into()),
        client_secret: secret(env, "AGG_ID_CLIENT_SECRET")?.context("AGG_AUTH=oidc needs AGG_ID_CLIENT_SECRET")?,
        audience: set(env, "AGG_ID_AUDIENCE").unwrap_or_else(|| "pacod-aggregator".into()),
        max_staleness_s,
    })
}

impl Config {
    pub fn from_env(env: Env) -> anyhow::Result<Config> {
        let auth = match set(env, "AGG_AUTH").as_deref().unwrap_or("oidc") {
            "oidc" => Auth::Oidc(id_settings(env)?),
            "dev" => {
                let token = secret(env, "AGG_DEV_TOKEN")?.context("AGG_AUTH=dev needs AGG_DEV_TOKEN")?;
                ensure!(token.len() >= MIN_DEV_TOKEN, "AGG_DEV_TOKEN has fewer than {MIN_DEV_TOKEN} characters");
                Auth::Dev { token }
            }
            other => bail!("AGG_AUTH must be oidc or dev, not {other:?}"),
        };
        let internal_token = secret(env, "AGG_INTERNAL_TOKEN")?;
        if let Some(t) = &internal_token {
            ensure!(t.len() >= MIN_INTERNAL_TOKEN, "AGG_INTERNAL_TOKEN has fewer than {MIN_INTERNAL_TOKEN} characters");
        }
        let llm = match set(env, "AGG_LLM_URL") {
            None => None,
            Some(url) => Some(LlmLink { url: check_url("AGG_LLM_URL", &url)?, key: secret(env, "AGG_LLM_KEY")? }),
        };
        let metrics_listen = match set(env, "AGG_METRICS_LISTEN").as_deref() {
            Some("off") => None,
            Some(v) => {
                check_listen("AGG_METRICS_LISTEN", v)?;
                Some(v.to_owned())
            }
            None => Some("0.0.0.0:9150".to_owned()),
        };
        Ok(Config {
            listen: listen(env, "AGG_LISTEN", "0.0.0.0:8150")?,
            admin_listen: listen(env, "AGG_ADMIN_LISTEN", "0.0.0.0:8151")?,
            metrics_listen,
            database_url: secret(env, "DATABASE_URL")?.context("DATABASE_URL is not set")?,
            auth,
            catalog_public: flag(env, "AGG_CATALOG_PUBLIC", false)?,
            internal_token,
            llm,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    const DEV: &str = "a-development-token-of-32-characters";

    fn config(vars: &[(&str, &str)]) -> anyhow::Result<Config> {
        let map: HashMap<String, String> = vars.iter().map(|(k, v)| ((*k).into(), (*v).into())).collect();
        Config::from_env(&|n| map.get(n).cloned())
    }

    const BASE: &[(&str, &str)] = &[("DATABASE_URL", "postgres://x/y"), ("AGG_AUTH", "dev"), ("AGG_DEV_TOKEN", DEV)];

    fn with(extra: &[(&'static str, &'static str)]) -> anyhow::Result<Config> {
        config(&[BASE, extra].concat())
    }

    #[test]
    fn the_defaults_are_the_documented_ports() {
        let c = with(&[]).unwrap();
        assert_eq!(
            (c.listen.as_str(), c.admin_listen.as_str(), c.metrics_listen.as_deref()),
            ("0.0.0.0:8150", "0.0.0.0:8151", Some("0.0.0.0:9150"))
        );
        assert!(!c.catalog_public && c.internal_token.is_none() && c.llm.is_none());
    }

    #[test]
    fn what_cannot_work_stops_the_start() {
        for bad in [
            ("AGG_AUTH", "none"),
            ("AGG_DEV_TOKEN", "short"),
            ("AGG_LISTEN", "nowhere"),
            ("AGG_ADMIN_LISTEN", "host:port"),
            ("AGG_METRICS_LISTEN", "x"),
            ("AGG_CATALOG_PUBLIC", "maybe"),
            ("AGG_INTERNAL_TOKEN", "short"),
            ("AGG_LLM_URL", "ftp://example.com"),
            ("AGG_LLM_URL", "https://user:secret@example.com"),
        ] {
            assert!(with(&[bad]).is_err(), "{bad:?}");
        }
        assert!(config(&[("AGG_AUTH", "dev"), ("AGG_DEV_TOKEN", DEV)]).is_err(), "no database");
        assert!(config(&[("DATABASE_URL", "postgres://x/y")]).is_err(), "oidc without its settings");
    }

    #[test]
    fn oidc_takes_its_settings_with_defaults_for_the_client() {
        let c = config(&[
            ("DATABASE_URL", "postgres://x/y"),
            ("AGG_ID_ISSUER", "https://id.example.com, https://id2.example.com"),
            ("AGG_ID_INTERNAL_URL", "http://pacod-id-internal:8071/"),
            ("AGG_ID_CLIENT_SECRET", "s3cret"),
        ])
        .unwrap();
        let Auth::Oidc(id) = c.auth else { panic!("oidc is the default") };
        assert_eq!(id.issuers, ["https://id.example.com", "https://id2.example.com"]);
        assert_eq!(id.internal_url, "http://pacod-id-internal:8071");
        assert_eq!((id.client_id.as_str(), id.audience.as_str()), ("pacod-agg-service", "pacod-aggregator"));
    }

    #[test]
    fn a_secret_comes_from_a_variable_or_a_file_not_both() {
        let dir = std::env::temp_dir().join(format!("pagg-secret-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("token");
        std::fs::write(&file, "from-a-mounted-file-0123456789\n").unwrap();
        let path = file.display().to_string();
        let vars: HashMap<String, String> = [("AGG_INTERNAL_TOKEN_FILE".to_owned(), path.clone())].into();
        assert_eq!(
            secret(&|n| vars.get(n).cloned(), "AGG_INTERNAL_TOKEN").unwrap().as_deref(),
            Some("from-a-mounted-file-0123456789")
        );
        let both: HashMap<String, String> =
            [("AGG_INTERNAL_TOKEN".to_owned(), "x".to_owned()), ("AGG_INTERNAL_TOKEN_FILE".to_owned(), path)].into();
        assert!(secret(&|n| both.get(n).cloned(), "AGG_INTERNAL_TOKEN").is_err());
    }

    #[test]
    fn the_example_file_names_every_variable_and_nothing_else() {
        let example = include_str!("../../../deploy/.env.example");
        for v in VARIABLES {
            assert!(example.contains(&format!("{v}=")), "{v} is missing in deploy/.env.example");
        }
        // What only the deployment reads: compose's own settings, the console's and the database's.
        let deployment = |name: &str| {
            ["AGG_VERSION", "AGG_REGISTRY", "AGG_DB_PASSWORD", "AGG_UI_", "AGG_CONSOLE_", "AGG_API_"]
                .iter()
                .any(|p| name.starts_with(p))
        };
        for line in example.lines() {
            let line = line.trim_start_matches('#').trim();
            let Some((name, _)) = line.split_once('=') else { continue };
            if name.starts_with("AGG_") && !name.contains(' ') && !deployment(name) {
                assert!(VARIABLES.contains(&name), "deploy/.env.example names {name}, which the server does not read");
            }
        }
    }
}
