//! The OpenAPI 3.1 document of the APIs, built from the same types the handlers use (schemars,
//! JSON Schema 2020-12), so it cannot drift from the code. `pacod-agg openapi` prints it; the
//! console's typed client is generated from it (`make openapi`), and a test fails when the
//! committed copy, `docs/openapi.json`, is out of date.

use schemars::generate::SchemaSettings;
use schemars::{JsonSchema, SchemaGenerator};
use serde_json::{Map, Value, json};

use crate::api::ErrorBody;
use crate::api::admin::{ModelDetail, ModelList, Session};
use crate::api::resources::{PageView, SettingsInput, SettingsView};
use crate::jobs::StatusView;
use crate::store::jobs::JobRun;
use crate::store::models::{Change, PriceRow};
use crate::store::pages::PageInput;
use crate::store::vendors::{Vendor, VendorInput};

struct Doc {
    generator: SchemaGenerator,
    paths: Map<String, Value>,
}

impl Doc {
    fn schema<T: JsonSchema>(&mut self) -> Value {
        serde_json::to_value(self.generator.subschema_for::<T>()).unwrap_or_default()
    }

    /// One operation. `params` are query parameters: name, type, description.
    fn op(&mut self, method: &str, path: &str, spec: Op) {
        let error = self.schema::<ErrorBody>();
        let name = format!("{method}_{}", path.trim_start_matches('/').replace(['/', '{', '}', '.', '-'], "_"));
        let mut o = json!({
            "tags": [spec.tag],
            "summary": spec.summary,
            "operationId": name.replace("__", "_").trim_end_matches('_'),
            "responses": {
                spec.status.to_string(): match &spec.response {
                    Some(s) => json!({ "description": "OK", "content": { "application/json": { "schema": s } } }),
                    None => json!({ "description": "Done" }),
                },
                "default": { "description": "A problem", "content": { "application/json": { "schema": error } } }
            }
        });
        if let Some(b) = spec.body {
            o["requestBody"] = json!({ "required": true, "content": { "application/json": { "schema": b } } });
        }
        let mut params: Vec<Value> = path
            .split('/')
            .filter_map(|seg| seg.strip_prefix('{').and_then(|s| s.strip_suffix('}')))
            .map(|n| json!({ "name": n, "in": "path", "required": true, "schema": { "type": "string" } }))
            .collect();
        for (name, kind, description) in spec.query {
            params.push(json!({ "name": name, "in": "query", "required": false, "description": description,
                                "schema": { "type": kind } }));
        }
        if !params.is_empty() {
            o["parameters"] = Value::Array(params);
        }
        self.paths
            .entry(path.to_owned())
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .expect("a path is an object")
            .insert(method.to_owned(), o);
    }
}

/// What an operation takes and answers.
struct Op {
    tag: &'static str,
    summary: &'static str,
    status: u16,
    body: Option<Value>,
    response: Option<Value>,
    query: &'static [(&'static str, &'static str, &'static str)],
}

pub fn document() -> Value {
    let mut doc = Doc {
        generator: SchemaGenerator::new(SchemaSettings::draft2020_12().with(|s| {
            s.definitions_path = "/components/schemas".into();
        })),
        paths: Map::new(),
    };
    let op = |tag, summary, status, body, response, query| Op { tag, summary, status, body, response, query };
    let list = |item: Value| Some(json!({ "type": "array", "items": item }));

    let session = doc.schema::<Session>();
    doc.op("get", "/api/v1/session", op("admin", "Who is calling", 200, None, Some(session), &[]));

    let status = doc.schema::<StatusView>();
    doc.op("get", "/api/v1/status", op("admin", "Jobs, the scraper and what is wrong", 200, None, Some(status), &[]));
    let run = doc.schema::<JobRun>();
    let filters: &[(&str, &str, &str)] =
        &[("job", "string", "check or scan; both by default."), ("limit", "integer", "How many, 30 by default.")];
    doc.op("get", "/api/v1/runs", op("runs", "Runs, newest first", 200, None, list(run), filters));
    doc.op("post", "/api/v1/runs/check", op("runs", "Check every page now", 202, None, None, &[]));
    doc.op("post", "/api/v1/runs/scan", op("runs", "Scan every vendor's site now", 202, None, None, &[]));

    let (models, model) = (doc.schema::<ModelList>(), doc.schema::<ModelDetail>());
    let filters: &[(&str, &str, &str)] = &[
        ("vendor", "string", "Only this vendor's."),
        ("status", "string", "Only models of this status."),
        ("cursor", "string", "Continue a listing."),
        ("limit", "integer", "Page size."),
    ];
    doc.op("get", "/api/v1/models", op("models", "The catalogue's models", 200, None, Some(models), filters));
    doc.op("get", "/api/v1/models/{id}", op("models", "One model and its history", 200, None, Some(model), &[]));
    let change = doc.schema::<Change>();
    let limit: &[(&str, &str, &str)] = &[("limit", "integer", "How many, 100 by default.")];
    doc.op("get", "/api/v1/changes", op("models", "What changed, newest first", 200, None, list(change), limit));

    // The public API. A catalogue and its models are `pacod.catalog/v1`, whose reference is the
    // crate `pacod-agg-catalog`: it has its own version, and readers use the crate.
    let object =
        json!({ "type": "object", "description": "pacod.catalog/v1, as crates/pacod-agg-catalog defines it." });
    let vendors: &[(&str, &str, &str)] = &[("vendor", "string", "Vendor ids, comma separated.")];
    doc.op(
        "get",
        "/v1/catalog",
        op("catalog", "The catalogue (answers 304 to If-None-Match)", 200, None, Some(object.clone()), vendors),
    );
    doc.op("get", "/v1/vendors", op("catalog", "The vendors", 200, None, list(object.clone()), &[]));
    doc.op("get", "/v1/models", op("catalog", "A page of models", 200, None, Some(object.clone()), filters));
    doc.op("get", "/v1/models/{id}", op("catalog", "One model", 200, None, Some(object), &[]));
    let price = doc.schema::<PriceRow>();
    doc.op(
        "get",
        "/v1/models/{id}/prices",
        op("catalog", "Every version of a model's price", 200, None, list(price), &[]),
    );

    let (vendor, vendor_in) = (doc.schema::<Vendor>(), doc.schema::<VendorInput>());
    doc.op("get", "/api/v1/vendors", op("vendors", "The vendors", 200, None, list(vendor.clone()), &[]));
    doc.op(
        "post",
        "/api/v1/vendors",
        op("vendors", "Add a vendor", 201, Some(vendor_in.clone()), Some(vendor.clone()), &[]),
    );
    doc.op("get", "/api/v1/vendors/{id}", op("vendors", "One vendor", 200, None, Some(vendor.clone()), &[]));
    doc.op("put", "/api/v1/vendors/{id}", op("vendors", "Change a vendor", 200, Some(vendor_in), Some(vendor), &[]));
    doc.op("delete", "/api/v1/vendors/{id}", op("vendors", "Remove a vendor and its pages", 204, None, None, &[]));

    let (page, page_in) = (doc.schema::<PageView>(), doc.schema::<PageInput>());
    let filters: &[(&str, &str, &str)] =
        &[("vendor_id", "string", "Only this vendor's."), ("status", "string", "proposed, active or dismissed.")];
    doc.op(
        "get",
        "/api/v1/pages",
        op("pages", "Pages, with what the last check found", 200, None, list(page.clone()), filters),
    );
    doc.op("post", "/api/v1/pages", op("pages", "Add a page", 201, Some(page_in.clone()), Some(page.clone()), &[]));
    doc.op("get", "/api/v1/pages/{id}", op("pages", "One page", 200, None, Some(page.clone()), &[]));
    doc.op("put", "/api/v1/pages/{id}", op("pages", "Change a page", 200, Some(page_in), Some(page.clone()), &[]));
    doc.op("delete", "/api/v1/pages/{id}", op("pages", "Remove a page", 204, None, None, &[]));
    doc.op("post", "/api/v1/pages/{id}/confirm", op("pages", "Confirm a page", 200, None, Some(page.clone()), &[]));
    doc.op("post", "/api/v1/pages/{id}/dismiss", op("pages", "Dismiss a page", 200, None, Some(page), &[]));
    doc.op("post", "/api/v1/pages/{id}/check", op("pages", "Check a page at the next run", 202, None, None, &[]));

    let (settings, settings_in) = (doc.schema::<SettingsView>(), doc.schema::<SettingsInput>());
    doc.op("get", "/api/v1/settings", op("settings", "The settings", 200, None, Some(settings.clone()), &[]));
    doc.op(
        "put",
        "/api/v1/settings",
        op("settings", "Change the settings", 200, Some(settings_in), Some(settings), &[]),
    );

    let schemas = doc.generator.take_definitions(true);
    json!({
        "openapi": "3.1.0",
        "info": { "title": "pacod-aggregator", "version": env!("CARGO_PKG_VERSION"),
                  "description": "Model data for AI gateways: the public catalogue and the console's API." },
        "paths": doc.paths,
        "components": { "schemas": schemas },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_committed_document_is_the_codes() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/openapi.json");
        let now = serde_json::to_string_pretty(&document()).unwrap() + "\n";
        if std::env::var_os("UPDATE_OPENAPI").is_some() {
            std::fs::write(&path, &now).unwrap();
        }
        let committed = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(committed == now, "docs/openapi.json is out of date: make openapi");
    }
}
