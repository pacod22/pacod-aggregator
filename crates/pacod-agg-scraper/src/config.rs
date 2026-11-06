//! The scraper's configuration, read from the environment.

use std::fmt;
use std::net::SocketAddr;

use anyhow::{Context, bail};

/// Shortest internal token the scraper accepts, as the server and the analytics service do.
pub const MIN_TOKEN_CHARS: usize = 16;

#[derive(Clone)]
pub struct Config {
    /// `AGG_SERVER_URL`: the server's admin listener, for example
    /// `http://pacod-aggregator-admin:8151`. Required: the scraper does nothing without the server.
    pub server_url: String,
    /// `AGG_INTERNAL_TOKEN`: the secret shared with the server. Required.
    pub internal_token: String,
    /// `AGG_EGRESS_ALLOW_PRIVATE`: hosts, addresses and ranges that are not public and that the
    /// vendors' pages may still reach, separated by commas. A page's scripts run in the browser,
    /// and reach what is named here: empty by default.
    pub egress_allow_private: Vec<String>,
    /// `AGG_SCRAPER_BROWSER_URL`: the browser's DevTools endpoint, default `http://agg-browser:9222`.
    pub browser_url: String,
    /// `AGG_SCRAPER_PROXY_LISTEN`: where the egress proxy the browser goes out through listens, as
    /// `host:port`, default `0.0.0.0:3128`. Compose names the alias the scraper has on the
    /// browser's network alone (`agg-scraper-proxy:3128`), so that nothing else reaches it.
    pub proxy_listen: String,
    /// `AGG_SCRAPER_PROXY_URL`: the proxy as the browser reaches it, default `http://agg-scraper-proxy:3128`.
    pub proxy_url: String,
    /// `AGG_SCRAPER_ID`: names this scraper as the holder of a lease, the same across restarts
    /// and unique among scrapers. Default: the host name.
    pub id: String,
    /// `AGG_SCRAPER_LISTEN`: `/healthz` and `/readyz`, default `0.0.0.0:8152`.
    pub listen: SocketAddr,
    /// `AGG_SCRAPER_USER_AGENT`: the user agent of the scraper's own requests (price lists,
    /// `robots.txt`, sitemaps), default `pacod-agg-scraper/<version>`. Unset, pages meet the
    /// browser's own, which its image makes Chromium's without headless mode's mark. Set, it
    /// replaces that too, in the page alone: the page's frames and workers keep the browser's,
    /// and the page sends no client hints.
    pub user_agent: Option<String>,
    /// `AGG_SCRAPER_PAGE_TIMEOUT_SECONDS`: how long a page may take to render, default 45.
    pub page_timeout_seconds: u64,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    /// Builds the configuration from any source of variables. Empty values count as unset.
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> anyhow::Result<Self> {
        let get = |name: &str| lookup(name).map(|v| v.trim().to_owned()).filter(|v| !v.is_empty());
        let addr = |name: &str, default: &str| -> anyhow::Result<SocketAddr> {
            let v = get(name).unwrap_or_else(|| default.to_owned());
            v.parse().with_context(|| format!("{name} is not a socket address: {v}"))
        };
        let page_timeout_seconds = match get("AGG_SCRAPER_PAGE_TIMEOUT_SECONDS") {
            Some(v) => v.parse().with_context(|| format!("AGG_SCRAPER_PAGE_TIMEOUT_SECONDS is not a number: {v}"))?,
            None => 45,
        };
        let config = Self {
            server_url: get("AGG_SERVER_URL").context("AGG_SERVER_URL is required")?,
            internal_token: get("AGG_INTERNAL_TOKEN").context("AGG_INTERNAL_TOKEN is required")?,
            egress_allow_private: get("AGG_EGRESS_ALLOW_PRIVATE")
                .map(|v| v.split(',').map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()).collect())
                .unwrap_or_default(),
            browser_url: get("AGG_SCRAPER_BROWSER_URL").unwrap_or_else(|| "http://agg-browser:9222".to_owned()),
            proxy_listen: get("AGG_SCRAPER_PROXY_LISTEN").unwrap_or_else(|| "0.0.0.0:3128".to_owned()),
            proxy_url: get("AGG_SCRAPER_PROXY_URL").unwrap_or_else(|| "http://agg-scraper-proxy:3128".to_owned()),
            listen: addr("AGG_SCRAPER_LISTEN", "0.0.0.0:8152")?,
            id: get("AGG_SCRAPER_ID")
                .or_else(|| get("HOSTNAME"))
                .unwrap_or_else(|| format!("agg-scraper-{}", uuid::Uuid::now_v7().simple())),
            user_agent: get("AGG_SCRAPER_USER_AGENT"),
            page_timeout_seconds,
        };
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        if self.internal_token.chars().count() < MIN_TOKEN_CHARS {
            bail!("AGG_INTERNAL_TOKEN must be at least {MIN_TOKEN_CHARS} characters");
        }
        if let Err(why) = pacod_agg_jobs::egress::EgressPolicy::parse(&self.egress_allow_private) {
            bail!("AGG_EGRESS_ALLOW_PRIVATE: {why}");
        }
        for (name, value) in [("AGG_SERVER_URL", &self.server_url), ("AGG_SCRAPER_BROWSER_URL", &self.browser_url)] {
            let url = url::Url::parse(value).with_context(|| format!("{name} is not a URL: {value}"))?;
            if url.scheme() != "http" {
                bail!("{name} is an http:// address on the gateway's own networks: {value}");
            }
        }
        if self.proxy_listen.rsplit_once(':').is_none_or(|(h, p)| h.is_empty() || p.parse::<u16>().is_err()) {
            bail!("AGG_SCRAPER_PROXY_LISTEN is host:port: {}", self.proxy_listen);
        }
        if !(5..=600).contains(&self.page_timeout_seconds) {
            bail!("AGG_SCRAPER_PAGE_TIMEOUT_SECONDS must be between 5 and 600");
        }
        Ok(())
    }
}

impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("server_url", &self.server_url)
            .field("internal_token", &"<redacted>")
            .field("egress_allow_private", &self.egress_allow_private)
            .field("browser_url", &self.browser_url)
            .field("proxy_listen", &self.proxy_listen)
            .field("proxy_url", &self.proxy_url)
            .field("id", &self.id)
            .field("listen", &self.listen)
            .field("user_agent", &self.user_agent)
            .field("page_timeout_seconds", &self.page_timeout_seconds)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn from(vars: &[(&str, &str)]) -> anyhow::Result<Config> {
        Config::from_lookup(|name| vars.iter().find(|(k, _)| *k == name).map(|(_, v)| (*v).to_owned()))
    }

    #[test]
    fn the_core_and_its_token_are_required_and_the_rest_has_defaults() {
        let base =
            [("AGG_SERVER_URL", "http://pacod-aggregator-admin:8151"), ("AGG_INTERNAL_TOKEN", "0123456789abcdef")];
        let c = from(&base).unwrap();
        assert_eq!(
            (c.browser_url.as_str(), c.listen.port(), c.proxy_listen.as_str()),
            ("http://agg-browser:9222", 8152, "0.0.0.0:3128")
        );
        assert_eq!(c.page_timeout_seconds, 45);
        assert!(!format!("{c:?}").contains("0123456789abcdef"), "the token is not logged");
        assert!(from(&base[..1]).is_err());
        assert!(from(&[base[0], ("AGG_INTERNAL_TOKEN", "short")]).is_err());
        for bad in [
            ("AGG_EGRESS_ALLOW_PRIVATE", "10.0.0.0/99"),
            ("AGG_SCRAPER_BROWSER_URL", "https://agg-browser:9222"),
            ("AGG_SCRAPER_PAGE_TIMEOUT_SECONDS", "1"),
            ("AGG_SCRAPER_LISTEN", "nowhere"),
            ("AGG_SCRAPER_PROXY_LISTEN", "agg-scraper-proxy"),
        ] {
            assert!(from(&[base[0], base[1], bad]).is_err(), "{bad:?}");
        }
        let c = from(&[base[0], base[1], ("AGG_EGRESS_ALLOW_PRIVATE", "agg-mock, 10.0.0.0/8")]).unwrap();
        assert_eq!(c.egress_allow_private, ["agg-mock", "10.0.0.0/8"]);
        // Named by the operator, or by the host it runs on.
        assert_eq!(from(&[base[0], base[1], ("AGG_SCRAPER_ID", "agg-scraper-1")]).unwrap().id, "agg-scraper-1");
        assert_eq!(from(&[base[0], base[1], ("HOSTNAME", "host-7")]).unwrap().id, "host-7");
    }
}
