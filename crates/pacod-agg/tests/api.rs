//! The server end to end: a database of its own on the test PostgreSQL (`make test-services`, or
//! AGG_TEST_DATABASE_URL). Callers use the development token.

use std::sync::Arc;

use axum::Router;
use pacod_agg::config::{Auth, Config};
use serde_json::{Value, json};
use tokio::sync::watch;

const DEV: &str = "a-development-token-of-32-characters";

async fn database() -> String {
    let base =
        std::env::var("AGG_TEST_DATABASE_URL").unwrap_or_else(|_| "postgres://postgres:test@127.0.0.1:25435".into());
    let admin = sqlx::PgPool::connect(&format!("{base}/postgres"))
        .await
        .unwrap_or_else(|e| panic!("the test database does not answer ({e}): run make test-services"));
    let name = format!("agg_test_{}", uuid::Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE DATABASE {name}")).execute(&admin).await.unwrap();
    format!("{base}/{name}")
}

async fn serve(router: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    format!("http://{addr}")
}

/// A started server: its two listeners.
struct Server {
    public: String,
    admin: String,
    http: reqwest::Client,
    _stop: watch::Sender<bool>,
}

impl Server {
    async fn start(adjust: impl FnOnce(&mut Config)) -> Server {
        let mut config = Config {
            listen: "127.0.0.1:0".into(),
            admin_listen: "127.0.0.1:0".into(),
            metrics_listen: None,
            database_url: database().await,
            auth: Auth::Dev { token: DEV.into() },
            catalog_public: false,
            internal_token: Some("an-internal-token-0123456789".into()),
            llm: None,
        };
        adjust(&mut config);
        let (stop, rx) = watch::channel(false);
        let app = pacod_agg::app(Arc::new(config), rx).await.unwrap();
        Server {
            public: serve(app.public).await,
            admin: serve(app.admin).await,
            http: reqwest::Client::new(),
            _stop: stop,
        }
    }

    async fn get(&self, url: String, token: Option<&str>) -> (u16, Value) {
        let mut request = self.http.get(url);
        if let Some(t) = token {
            request = request.bearer_auth(t);
        }
        let r = request.send().await.unwrap();
        let status = r.status().as_u16();
        (status, r.json().await.unwrap_or(Value::Null))
    }
}

#[tokio::test]
async fn the_probes_answer_on_both_listeners_without_a_token() {
    let s = Server::start(|_| {}).await;
    for base in [&s.public, &s.admin] {
        assert_eq!(s.http.get(format!("{base}/healthz")).send().await.unwrap().status(), 200);
        let (st, v) = s.get(format!("{base}/readyz"), None).await;
        assert_eq!((st, &v), (200, &json!({ "ok": true, "database": true, "identity": true })));
    }
}

#[tokio::test]
async fn the_admin_api_is_not_on_the_public_listener_and_wants_a_token() {
    let s = Server::start(|_| {}).await;
    let session = format!("{}/api/v1/session", s.admin);
    let (st, v) = s.get(session.clone(), None).await;
    assert_eq!((st, v["error"]["type"].as_str()), (401, Some("unauthorized")), "{v}");
    assert_eq!(s.get(session.clone(), Some("wrong-token")).await.0, 401);
    let (st, v) = s.get(session, Some(DEV)).await;
    assert_eq!((st, v["display"].as_str()), (200, Some("development")), "{v}");
    assert_eq!(v["permissions"].as_array().unwrap().len(), 4, "{v}");
    assert_eq!(s.get(format!("{}/api/v1/session", s.public), Some(DEV)).await.0, 404);
}

#[tokio::test]
async fn migrating_twice_changes_nothing() {
    let url = database().await;
    for _ in 0..2 {
        pacod_agg::connect(&url).await.unwrap().migrate().await.unwrap();
    }
}
