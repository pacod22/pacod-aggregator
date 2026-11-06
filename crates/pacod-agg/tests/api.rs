//! The server end to end: a database of its own on the test PostgreSQL (`make test-services`, or
//! AGG_TEST_DATABASE_URL). Callers use the development token.

mod common;

use common::{DEV, Server, database};
use serde_json::json;

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
async fn a_new_installation_starts_from_the_vendors_and_their_lists() {
    let db = database().await;
    let s = Server::start(|c| c.database_url = db.clone()).await;
    let (st, vendors) = s.api("GET", "/vendors", None).await;
    assert_eq!(st, 200);
    let ids: Vec<&str> = vendors.as_array().unwrap().iter().map(|v| v["id"].as_str().unwrap()).collect();
    for id in pacod_agg_catalog::vendors::VENDORS.iter().map(|v| v.id) {
        assert!(ids.contains(&id), "{id} is missing: {ids:?}");
    }
    let openai = vendors.as_array().unwrap().iter().find(|v| v["id"] == "openai").unwrap();
    assert_eq!(openai["complete"], true, "it has pages of its own: {openai}");
    assert!(openai["sites"][0].as_str().unwrap().starts_with("https://"), "{openai}");
    let mistral = vendors.as_array().unwrap().iter().find(|v| v["id"] == "mistral").unwrap();
    assert_eq!(mistral["complete"], false, "known from the lists alone: {mistral}");

    // The lists are read from the first check, below any page; the vendors' pages are not added
    // yet: the scan proposes them for an administrator to confirm.
    let (_, pages) = s.api("GET", "/pages?vendor_id=openai", None).await;
    let list = pages.as_array().unwrap();
    assert_eq!(list.len(), 2, "{pages}");
    assert!(list.iter().all(|p| p["kind"] == "feed" && p["status"] == "active" && p["found_by"] == "seed"), "{pages}");
    assert!(list.iter().all(|p| p["priority"].as_i64().unwrap() >= 100), "{pages}");

    // What an administrator removed does not come back.
    let id = list[0]["id"].as_str().unwrap();
    assert_eq!(s.api("DELETE", &format!("/pages/{id}"), None).await.0, 204);
    assert_eq!(s.api("DELETE", "/vendors/mistral", None).await.0, 204);
    drop(s);
    let s = Server::start(|c| c.database_url = db.clone()).await;
    assert_eq!(s.api("GET", "/pages?vendor_id=openai", None).await.1.as_array().unwrap().len(), 1);
    assert_eq!(s.api("GET", "/vendors/mistral", None).await.0, 404);
}

#[tokio::test]
async fn a_page_is_normalised_checked_and_decided_on() {
    let s = Server::start(|_| {}).await;
    let url = "https://Example.COM/pricing/?utm_source=news#table";
    let (st, page) = s.api("POST", "/pages", Some(json!({ "vendor_id": "openai", "url": url }))).await;
    assert_eq!(st, 201, "{page}");
    assert_eq!(
        (page["url"].as_str(), page["status"].as_str(), page["found_by"].as_str(), page["vendor"].as_str()),
        (Some("https://example.com/pricing"), Some("active"), Some("admin"), Some("OpenAI")),
        "{page}"
    );
    assert_eq!(page["state"]["check_requested"], true, "a new page is checked at the next run: {page}");
    let id = page["id"].as_str().unwrap();

    // The same address again, written otherwise.
    let (st, v) =
        s.api("POST", "/pages", Some(json!({ "vendor_id": "openai", "url": "https://example.com/pricing" }))).await;
    assert_eq!(st, 409, "{v}");
    // Another vendor may read the same address.
    let (st, v) =
        s.api("POST", "/pages", Some(json!({ "vendor_id": "xai", "url": "https://example.com/pricing" }))).await;
    assert_eq!(st, 201, "{v}");

    for (bad, why) in [
        (json!({ "vendor_id": "nobody", "url": "https://example.com/x" }), "no vendor"),
        (json!({ "vendor_id": "openai", "url": "http://169.254.169.254/latest/meta-data" }), "cloud metadata"),
        (json!({ "vendor_id": "openai", "url": "http://10.0.0.1/pricing" }), "a private address"),
        (json!({ "vendor_id": "openai", "url": "ftp://example.com/pricing" }), "not http"),
        (json!({ "vendor_id": "openai", "url": "https://example.com/y", "kind": "other" }), "no such kind"),
        (json!({ "vendor_id": "openai", "url": "https://example.com/y", "kind": "feed" }), "a feed without a format"),
        (json!({ "vendor_id": "openai", "url": "https://example.com/y", "format": "litellm" }), "a page with a format"),
    ] {
        let (st, v) = s.api("POST", "/pages", Some(bad)).await;
        assert_eq!(st, 422, "{why}: {v}");
    }

    // Dismissed, it is not checked; confirmed, it is.
    let (st, v) = s.api("POST", &format!("/pages/{id}/dismiss"), None).await;
    assert_eq!((st, v["status"].as_str(), v["decided_by"].as_str()), (200, Some("dismissed"), Some("dev")), "{v}");
    assert_eq!(s.api("POST", &format!("/pages/{id}/dismiss"), None).await.0, 409);
    assert_eq!(s.api("POST", &format!("/pages/{id}/check"), None).await.0, 409);
    assert_eq!(s.api("POST", &format!("/pages/{id}/confirm"), None).await.0, 200);
    assert_eq!(s.api("POST", &format!("/pages/{id}/check"), None).await.0, 202);

    let (st, list) = s.api("GET", "/pages?vendor_id=xai&status=active", None).await;
    assert_eq!(st, 200);
    assert!(list.as_array().unwrap().iter().any(|p| p["url"] == "https://example.com/pricing"), "{list}");
    assert_eq!(s.api("GET", "/pages/nonesuch", None).await.0, 404);
}

#[tokio::test]
async fn a_private_host_named_in_the_policy_may_be_read() {
    let s = Server::start(|c| c.egress_allow_private = vec!["10.0.0.0/8".into()]).await;
    let page = json!({ "vendor_id": "openai", "url": "http://10.0.0.1/pricing" });
    assert_eq!(s.api("POST", "/pages", Some(page)).await.0, 201);
}

#[tokio::test]
async fn the_settings_keep_their_version() {
    let s = Server::start(|_| {}).await;
    let (st, mut view) = s.api("GET", "/settings", None).await;
    assert_eq!((st, view["version"].as_i64(), view["enabled"].as_bool()), (200, Some(0), Some(true)), "{view}");
    view["schedule"] = json!("5 1 * * *");
    view["extraction_model"] = json!("  reader  ");
    let (st, saved) = s.api("PUT", "/settings", Some(view.clone())).await;
    assert_eq!((st, saved["version"].as_i64()), (200, Some(1)), "{saved}");
    assert_eq!(saved["extraction_model"], "reader");
    // The change was made from the version before: another came first.
    assert_eq!(s.api("PUT", "/settings", Some(view.clone())).await.0, 409);
    view["version"] = json!(1);
    view["schedule"] = json!("every day");
    assert_eq!(s.api("PUT", "/settings", Some(view)).await.0, 422);
    assert_eq!(s.api("GET", "/settings", None).await.1["schedule"], "5 1 * * *");
}

#[tokio::test]
async fn the_operators_model_shows_and_is_not_stored() {
    let without = Server::start(|_| {}).await;
    assert!(without.api("GET", "/settings", None).await.1.get("extraction_model_default").is_none());

    let s = Server::start(|c| c.llm_model = Some("from-env".into())).await;
    let (_, mut view) = s.api("GET", "/settings", None).await;
    assert_eq!(view["extraction_model_default"], "from-env", "{view}");
    assert!(view.get("extraction_model").is_none(), "{view}");
    // A client that sends the view back changes nothing with it.
    view["extraction_model_default"] = json!("from-a-caller");
    let (st, saved) = s.api("PUT", "/settings", Some(view)).await;
    assert_eq!((st, saved["extraction_model_default"].as_str()), (200, Some("from-env")), "{saved}");
    let stored: serde_json::Value =
        sqlx::query_scalar("SELECT doc FROM settings").fetch_one(s.store.pool()).await.unwrap();
    assert!(stored.get("extraction_model_default").is_none(), "{stored}");
}

#[tokio::test]
async fn vendors_are_named_as_the_catalogue_names_them() {
    let s = Server::start(|_| {}).await;
    let v = |id: &str| json!({ "id": id, "display_name": "New", "sites": ["https://example.com/pricing/"] });
    let (st, made) = s.api("POST", "/vendors", Some(v("new-vendor"))).await;
    assert_eq!(st, 201, "{made}");
    assert_eq!(made["sites"][0], "https://example.com/pricing", "{made}");
    assert_eq!(s.api("POST", "/vendors", Some(v("new-vendor"))).await.0, 409);
    assert_eq!(s.api("POST", "/vendors", Some(v("New Vendor"))).await.0, 422);
    let (st, changed) =
        s.api("PUT", "/vendors/new-vendor", Some(json!({ "id": "x", "display_name": "Renamed" }))).await;
    assert_eq!(
        (st, changed["id"].as_str(), changed["display_name"].as_str()),
        (200, Some("new-vendor"), Some("Renamed"))
    );
    assert_eq!(s.api("DELETE", "/vendors/new-vendor", None).await.0, 204);
    assert_eq!(s.api("DELETE", "/vendors/new-vendor", None).await.0, 404);
}

#[tokio::test]
async fn migrating_twice_changes_nothing() {
    let url = database().await;
    for _ in 0..2 {
        pacod_agg::connect(&url).await.unwrap().migrate().await.unwrap();
    }
}
