//! What the tests share: a database of their own on the test PostgreSQL (`make test-services`, or
//! AGG_TEST_DATABASE_URL), and a server on it. Callers use the development token.
#![allow(dead_code)]

use std::sync::Arc;

use axum::Router;
use pacod_agg::config::{Auth, Config};
use serde_json::{Value, json};
use tokio::sync::watch;

pub const DEV: &str = "a-development-token-of-32-characters";
pub const INTERNAL: &str = "an-internal-token-0123456789";

pub async fn database() -> String {
    let base =
        std::env::var("AGG_TEST_DATABASE_URL").unwrap_or_else(|_| "postgres://postgres:test@127.0.0.1:25435".into());
    let admin = sqlx::PgPool::connect(&format!("{base}/postgres"))
        .await
        .unwrap_or_else(|e| panic!("the test database does not answer ({e}): run make test-services"));
    let name = format!("agg_test_{}", uuid::Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE DATABASE {name}")).execute(&admin).await.unwrap();
    format!("{base}/{name}")
}

pub async fn serve(router: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    format!("http://{addr}")
}

/// A started server: its two listeners.
pub struct Server {
    pub public: String,
    pub admin: String,
    pub http: reqwest::Client,
    pub store: pacod_agg::store::Store,
    _stop: watch::Sender<bool>,
}

impl Server {
    pub async fn start(adjust: impl FnOnce(&mut Config)) -> Server {
        let mut config = Config {
            listen: "127.0.0.1:0".into(),
            admin_listen: "127.0.0.1:0".into(),
            metrics_listen: None,
            database_url: database().await,
            auth: Auth::Dev { token: DEV.into() },
            catalog_public: false,
            public_url: None,
            internal_token: Some(INTERNAL.into()),
            llm: None,
            egress_allow_private: vec![],
        };
        adjust(&mut config);
        let (stop, rx) = watch::channel(false);
        let app = pacod_agg::app(Arc::new(config), rx).await.unwrap();
        let store = app.state.store.clone();
        Server {
            public: serve(app.public).await,
            admin: serve(app.admin).await,
            http: reqwest::Client::new(),
            store,
            _stop: stop,
        }
    }

    /// A call of the admin API as an administrator.
    pub async fn api(&self, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
        let mut request =
            self.http.request(method.parse().unwrap(), format!("{}/api/v1{path}", self.admin)).bearer_auth(DEV);
        if let Some(b) = body {
            request = request.json(&b);
        }
        let r = request.send().await.unwrap();
        let status = r.status().as_u16();
        (status, r.json().await.unwrap_or(Value::Null))
    }

    pub async fn get(&self, url: String, token: Option<&str>) -> (u16, Value) {
        let mut request = self.http.get(url);
        if let Some(t) = token {
            request = request.bearer_auth(t);
        }
        let r = request.send().await.unwrap();
        let status = r.status().as_u16();
        (status, r.json().await.unwrap_or(Value::Null))
    }

    /// A call of the internal API, as the scraper makes it.
    pub async fn internal(&self, path: &str, body: Value) -> (u16, Value) {
        let r = self
            .http
            .post(format!("{}/internal/v1/jobs{path}", self.admin))
            .header("x-agg-internal-token", INTERNAL)
            .json(&body)
            .send()
            .await
            .unwrap();
        let status = r.status().as_u16();
        (status, r.json().await.unwrap_or(Value::Null))
    }

    /// Leaves one vendor, without the lists it was seeded with: what a test looks at alone.
    pub async fn only_vendor(&self, id: &str) {
        for sql in ["DELETE FROM vendors WHERE id <> $1", "DELETE FROM pages WHERE vendor_id = $1"] {
            sqlx::query(sql).bind(id).execute(self.store.pool()).await.unwrap();
        }
    }

    /// Lets a lease run out, as it does when the scraper that holds it stops.
    pub async fn lapse(&self, job: &str) {
        sqlx::query("UPDATE jobs SET lease_until = now() - interval '1 minute' WHERE name = $1")
            .bind(job)
            .execute(self.store.pool())
            .await
            .unwrap();
    }

    pub async fn add_page(&self, vendor: &str, url: &str) -> String {
        let (st, v) = self.api("POST", "/pages", Some(json!({ "vendor_id": vendor, "url": url }))).await;
        assert_eq!(st, 201, "{v}");
        v["id"].as_str().unwrap().to_owned()
    }
}
