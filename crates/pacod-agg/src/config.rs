//! The server's settings, from the environment (`AGG_*`). `deploy/.env.example` lists them; a test
//! checks that the two agree.
//!
//! Secrets come from a variable or from the file a `_FILE` variable names (a mounted secret);
//! both set is refused, so that it is clear which one is used. They are never command-line
//! arguments, where `ps` would show them.

use std::net::SocketAddr;

use anyhow::{Context, bail, ensure};

use crate::settings::{Mode, Settings};

/// A shorter development token is refused.
const MIN_DEV_TOKEN: usize = 32;
/// A shorter internal token is refused.
const MIN_INTERNAL_TOKEN: usize = 16;
/// The longest agent pacod-gw takes in `x-gw-agent-id`.
const MAX_AGENT: usize = 200;

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
    "AGG_PUBLIC_URL",
    "AGG_INTERNAL_TOKEN",
    "AGG_LLM_URL",
    "AGG_LLM_KEY",
    "AGG_LLM_MODEL",
    "AGG_LLM_MAX_TOKENS_FIELD",
    "AGG_GW_URL",
    "AGG_GW_KEY",
    "AGG_GW_MODEL",
    "AGG_GW_AGENT",
    "AGG_EGRESS_ALLOW_PRIVATE",
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

/// The name of the limit on an answer's length, where an endpoint is not told otherwise.
const DEFAULT_LIMIT_FIELD: &str = "max_tokens";

/// Where the extraction model is reached: an OpenAI-compatible endpoint, a provider's own or a
/// gateway of pacod-gw, with a key of its own. The address and the key are the operator's, here
/// alone, because the key goes to that address with every call. They stay one value for the same
/// reason: the console chooses a link, so it can never put a key and another address together.
#[derive(Clone)]
pub struct LlmLink {
    pub url: String,
    pub key: Option<String>,
    /// What the endpoint calls the limit on an answer's length. A gateway translates it for each
    /// provider; called directly, a provider takes its own name for it or refuses the call.
    pub limit_field: &'static str,
    /// The agent the calls are made as, in `x-gw-agent-id`: the gateway's link only. It is the
    /// agent the key is bound to at the gateway, or none, because a gateway refuses a key's call that
    /// names another agent; a key bound to one agent needs none.
    pub agent: Option<String>,
}

impl LlmLink {
    pub fn new(url: String, key: Option<String>) -> Self {
        Self { url, key, limit_field: DEFAULT_LIMIT_FIELD, agent: None }
    }
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
    /// Where consumers reach the catalogue, as it says of itself.
    pub public_url: Option<String>,
    /// What the scraper presents on the internal API. Unset: the internal API answers no one.
    pub internal_token: Option<String>,
    /// The endpoint of standalone mode.
    pub llm: Option<LlmLink>,
    /// The operator's model, used while the console names none. Kept apart from `llm`, so that an
    /// address left empty to pause reading does not also lose the name.
    pub llm_model: Option<String>,
    /// The endpoint of gateway mode.
    pub gateway: Option<LlmLink>,
    /// The operator's model at the gateway, kept apart as `llm_model` is. A gateway's names are not
    /// a provider's, so the two modes never share one.
    pub gateway_model: Option<String>,
    /// Hosts and ranges that are not public and that pages may still be read from.
    pub egress_allow_private: Vec<String>,
}

fn limit_field(env: Env) -> anyhow::Result<&'static str> {
    match set(env, "AGG_LLM_MAX_TOKENS_FIELD").as_deref() {
        None | Some("max_tokens") => Ok(DEFAULT_LIMIT_FIELD),
        Some("max_completion_tokens") => Ok("max_completion_tokens"),
        Some(other) => bail!("AGG_LLM_MAX_TOKENS_FIELD must be max_tokens or max_completion_tokens, not {other:?}"),
    }
}

/// `AGG_GW_AGENT`, which goes into a header as it is written.
fn gateway_agent(env: Env) -> anyhow::Result<Option<String>> {
    let Some(agent) = set(env, "AGG_GW_AGENT") else { return Ok(None) };
    ensure!(
        agent.len() <= MAX_AGENT && agent.bytes().all(|b| b.is_ascii_graphic()),
        "AGG_GW_AGENT: {agent:?} is not an agent's name: at most {MAX_AGENT} visible characters, without spaces"
    );
    Ok(Some(agent))
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
        // Read whether or not there is an address: a mistyped name stops the start either way.
        let limit_field = limit_field(env)?;
        let agent = gateway_agent(env)?;
        let llm = match set(env, "AGG_LLM_URL") {
            None => None,
            Some(url) => Some(LlmLink {
                limit_field,
                ..LlmLink::new(check_url("AGG_LLM_URL", &url)?, secret(env, "AGG_LLM_KEY")?)
            }),
        };
        // Always the usual name: a gateway translates it for each provider, so the name a provider
        // asks for in AGG_LLM_MAX_TOKENS_FIELD does not apply here. The agent is the gateway's
        // alone: a provider's endpoint is told nothing of it.
        let gateway = match set(env, "AGG_GW_URL") {
            None => None,
            Some(url) => {
                Some(LlmLink { agent, ..LlmLink::new(check_url("AGG_GW_URL", &url)?, secret(env, "AGG_GW_KEY")?) })
            }
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
            public_url: match set(env, "AGG_PUBLIC_URL") {
                Some(u) => Some(check_url("AGG_PUBLIC_URL", &u)?),
                None => None,
            },
            internal_token,
            llm,
            llm_model: set(env, "AGG_LLM_MODEL"),
            gateway,
            gateway_model: set(env, "AGG_GW_MODEL"),
            egress_allow_private: set(env, "AGG_EGRESS_ALLOW_PRIVATE")
                .map(|v| v.split(',').map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()).collect())
                .unwrap_or_default(),
        })
    }
}

impl Config {
    /// The mode in use: the one chosen in the console. Until one is, an installation that set only
    /// the gateway uses it, and any other is standalone, so that one with only `AGG_LLM_URL` reads
    /// on as it did before there were modes, without anyone opening the settings.
    pub fn mode(&self, settings: &Settings) -> Mode {
        settings.mode.unwrap_or(if self.llm.is_none() && self.gateway.is_some() {
            Mode::Gateway
        } else {
            Mode::Standalone
        })
    }

    /// The endpoint of the mode in use, and never the other's: falling back would change who is
    /// paid and who sees the pages without anyone having chosen that.
    pub fn link(&self, settings: &Settings) -> Option<&LlmLink> {
        match self.mode(settings) {
            Mode::Standalone => self.llm.as_ref(),
            Mode::Gateway => self.gateway.as_ref(),
        }
    }

    /// The model that reads pages: the one named in the console for the mode in use, else the
    /// operator's for that mode.
    pub fn extraction_model<'a>(&'a self, settings: &'a Settings) -> Option<&'a str> {
        match self.mode(settings) {
            Mode::Standalone => settings.extraction_model.as_deref().or(self.llm_model.as_deref()),
            Mode::Gateway => settings.gateway_model.as_deref().or(self.gateway_model.as_deref()),
        }
    }

    /// Whether pages can be read: a model is named and there is an endpoint to call it on.
    pub fn can_read(&self, settings: &Settings) -> bool {
        self.link(settings).is_some() && self.extraction_model(settings).is_some()
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
            ("AGG_LLM_MAX_TOKENS_FIELD", "max_output_tokens"),
            ("AGG_GW_URL", "ftp://example.com"),
            ("AGG_GW_URL", "https://user:secret@example.com"),
            ("AGG_GW_AGENT", "two words"),
            ("AGG_GW_AGENT", "agent\u{7f}"),
        ] {
            assert!(with(&[bad]).is_err(), "{bad:?}");
        }
        assert!(config(&[("AGG_AUTH", "dev"), ("AGG_DEV_TOKEN", DEV)]).is_err(), "no database");
        assert!(config(&[("DATABASE_URL", "postgres://x/y")]).is_err(), "oidc without its settings");
    }

    #[test]
    fn the_model_comes_from_the_console_and_else_from_the_environment() {
        let c = with(&[("AGG_LLM_URL", "https://api.example.com/v1"), ("AGG_LLM_MODEL", "from-env")]).unwrap();
        let mut settings = Settings::default();
        assert_eq!(c.extraction_model(&settings), Some("from-env"));
        assert!(c.can_read(&settings));
        settings.extraction_model = Some("from-console".into());
        assert_eq!(c.extraction_model(&settings), Some("from-console"));
    }

    #[test]
    fn a_name_without_an_address_is_kept_but_reads_nothing() {
        let c = with(&[("AGG_LLM_MODEL", "from-env")]).unwrap();
        assert_eq!(c.extraction_model(&Settings::default()), Some("from-env"));
        assert!(!c.can_read(&Settings::default()));
    }

    #[test]
    fn the_limit_has_the_usual_name_until_told_otherwise() {
        let url = ("AGG_LLM_URL", "https://api.example.com/v1");
        assert_eq!(with(&[url]).unwrap().llm.unwrap().limit_field, "max_tokens");
        let named = with(&[url, ("AGG_LLM_MAX_TOKENS_FIELD", "max_completion_tokens")]).unwrap();
        assert_eq!(named.llm.unwrap().limit_field, "max_completion_tokens");
    }

    const DIRECT: (&str, &str) = ("AGG_LLM_URL", "https://api.example.com/v1");
    const GATEWAY: (&str, &str) = ("AGG_GW_URL", "http://pacod-gw:8080/v1/");

    #[test]
    fn the_gateway_has_its_own_address_and_key() {
        let c = with(&[
            DIRECT,
            ("AGG_LLM_KEY", "the-providers-key"),
            GATEWAY,
            ("AGG_GW_KEY", "the-gateways-key"),
            ("AGG_LLM_MAX_TOKENS_FIELD", "max_completion_tokens"),
        ])
        .unwrap();
        let (direct, gateway) = (c.llm.unwrap(), c.gateway.unwrap());
        assert_eq!(direct.url, "https://api.example.com/v1");
        assert_eq!(direct.key.as_deref(), Some("the-providers-key"));
        assert_eq!(gateway.url, "http://pacod-gw:8080/v1");
        assert_eq!(gateway.key.as_deref(), Some("the-gateways-key"));
        // The gateway translates the limit; the name a provider asks for is not for it.
        assert_eq!((direct.limit_field, gateway.limit_field), ("max_completion_tokens", "max_tokens"));
    }

    #[test]
    fn the_agent_goes_with_the_gateway_alone_and_is_none_by_default() {
        let agent = ("AGG_GW_AGENT", "pacod-aggregator");
        let c = with(&[DIRECT, GATEWAY, agent]).unwrap();
        assert_eq!(c.gateway.unwrap().agent.as_deref(), Some("pacod-aggregator"));
        assert_eq!(c.llm.unwrap().agent, None, "a provider is never told the gateway's agent");
        assert_eq!(with(&[GATEWAY]).unwrap().gateway.unwrap().agent, None);
        assert_eq!(with(&[GATEWAY, ("AGG_GW_AGENT", " ")]).unwrap().gateway.unwrap().agent, None);
        let named = |agent: &str| config(&[BASE, &[GATEWAY, ("AGG_GW_AGENT", agent)]].concat());
        assert!(named(&"a".repeat(MAX_AGENT)).is_ok());
        assert!(named(&"a".repeat(MAX_AGENT + 1)).is_err());
    }

    #[test]
    fn the_gateways_key_can_come_from_a_file() {
        let dir = std::env::temp_dir().join(format!("pagg-gw-key-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("key");
        std::fs::write(&file, "the-gateways-key\n").unwrap();
        let path = file.display().to_string();
        let keyed = [("AGG_GW_KEY_FILE", path.as_str())];
        let c = config(&[BASE, &[GATEWAY], &keyed].concat()).unwrap();
        assert_eq!(c.gateway.unwrap().key.as_deref(), Some("the-gateways-key"));
        assert!(config(&[BASE, &[GATEWAY, ("AGG_GW_KEY", "x")], &keyed].concat()).is_err(), "both are refused");
    }

    #[test]
    fn the_mode_is_the_consoles_and_else_follows_what_is_set() {
        let chosen = |mode| Settings { mode: Some(mode), ..Settings::default() };
        let none = Settings::default();
        // Until the console chooses: an endpoint of its own wins, as it did in the full stack.
        assert_eq!(with(&[]).unwrap().mode(&none), Mode::Standalone);
        assert_eq!(with(&[DIRECT]).unwrap().mode(&none), Mode::Standalone);
        assert_eq!(with(&[GATEWAY]).unwrap().mode(&none), Mode::Gateway);
        assert_eq!(with(&[DIRECT, GATEWAY]).unwrap().mode(&none), Mode::Standalone);
        // The console's choice stands, whatever is set.
        let c = with(&[DIRECT, GATEWAY]).unwrap();
        assert_eq!(c.mode(&chosen(Mode::Gateway)), Mode::Gateway);
        assert_eq!(with(&[GATEWAY]).unwrap().mode(&chosen(Mode::Standalone)), Mode::Standalone);
    }

    #[test]
    fn a_mode_without_its_endpoint_does_not_use_the_other() {
        let settings = Settings { mode: Some(Mode::Gateway), gateway_model: Some("m".into()), ..Settings::default() };
        let c = with(&[DIRECT, ("AGG_LLM_MODEL", "from-env")]).unwrap();
        assert!(c.link(&settings).is_none());
        assert!(!c.can_read(&settings));
        let settings = Settings { mode: Some(Mode::Standalone), ..Settings::default() };
        let c = with(&[GATEWAY, ("AGG_GW_MODEL", "from-env")]).unwrap();
        assert!(c.link(&settings).is_none());
        assert_eq!(c.extraction_model(&settings), None);
        assert!(!c.can_read(&settings));
    }

    #[test]
    fn each_mode_has_its_own_model() {
        let c =
            with(&[DIRECT, GATEWAY, ("AGG_LLM_MODEL", "direct-env"), ("AGG_GW_MODEL", "vendor/gateway-env")]).unwrap();
        let in_mode = |mode, s: Settings| Settings { mode: Some(mode), ..s };
        let named = || Settings {
            extraction_model: Some("direct-console".into()),
            gateway_model: Some("vendor/gateway-console".into()),
            ..Settings::default()
        };
        assert_eq!(c.extraction_model(&in_mode(Mode::Standalone, Settings::default())), Some("direct-env"));
        assert_eq!(c.extraction_model(&in_mode(Mode::Gateway, Settings::default())), Some("vendor/gateway-env"));
        assert_eq!(c.extraction_model(&in_mode(Mode::Standalone, named())), Some("direct-console"));
        assert_eq!(c.extraction_model(&in_mode(Mode::Gateway, named())), Some("vendor/gateway-console"));
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
        // What only the deployment reads: compose's own settings, the console's, the edge's, the
        // database's, and the scraper's own.
        let deployment = |name: &str| {
            [
                "AGG_VERSION",
                "AGG_REGISTRY",
                "AGG_DB_PASSWORD",
                "AGG_UI_",
                "AGG_CONSOLE_",
                "AGG_API_",
                "AGG_SITE",
                "AGG_ID_SITE",
                "AGG_HTTP_PORT",
                "AGG_HTTPS_PORT",
                "AGG_CADDY_",
                "AGG_SCRAPER_",
            ]
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
