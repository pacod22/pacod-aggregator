//! The catalogue as the server publishes it (`pacod.catalog/v1`), from what the vendors settled
//! into. A gateway reads it with the same crate this writes it with.

use chrono::Utc;
use pacod_agg_catalog::{Catalog, Evidence, Model, Price, SCHEMA_V1, Source, SourceKind, Vendor as CatalogVendor};
use serde_json::Value;

use crate::store::Store;
use crate::store::models::Published;
use crate::store::vendors::Vendor;

/// A stored value read into the type it was written from. What cannot be read stays unknown: one
/// odd row must not take the catalogue down.
fn read<T: serde::de::DeserializeOwned + Default>(v: &Value) -> T {
    serde_json::from_value(v.clone()).unwrap_or_default()
}

/// A stored string as an enum of the catalogue, which reads what it does not know as `Other`.
fn named<T: serde::de::DeserializeOwned>(name: &str) -> Option<T> {
    serde_json::from_value(Value::String(name.to_owned())).ok()
}

/// A model as the catalogue writes it.
pub fn model_of(p: &Published) -> Model {
    let m = &p.model;
    let price = p.price.as_ref().and_then(|v| serde_json::from_value::<Price>(v.clone()).ok()).map(|mut price| {
        price.effective_from = p.effective_from;
        price.changed_at = p.effective_from;
        price.checked_at = p.checked_at;
        // The evidence was fetched when its source was last read as it should.
        price.evidence = price.evidence.map(|e| Evidence { fetched_at: p.checked_at, ..e });
        price
    });
    Model {
        id: Some(m.id.clone()),
        vendor: Some(m.vendor_id.clone()),
        provider: None,
        name: None,
        api_id: m.api_id.clone(),
        aliases: m.aliases.clone(),
        api_id_confirmed: m.api_id_confirmed,
        display_name: m.display_name.clone(),
        kind: m.kind.as_deref().and_then(named),
        status: named(&m.status),
        deprecated_at: m.deprecated_at,
        sunset_at: m.sunset_at,
        successor: m.successor.clone(),
        first_seen_at: Some(m.first_seen_at),
        last_seen_at: Some(m.last_seen_at),
        properties: read(&m.properties),
        parameters: read(&m.parameters),
        price,
        provenance: read(&m.provenance),
    }
}

/// A vendor as the catalogue writes it: complete only when an administrator says its pages list
/// everything and every one of them was read at the last check.
pub fn vendor_of(v: &Vendor) -> CatalogVendor {
    CatalogVendor {
        id: v.id.clone(),
        display_name: Some(v.display_name.clone()),
        currency: Some(v.currency.clone()),
        complete: v.complete && v.read_complete,
        checked_at: v.checked_at,
    }
}

/// The catalogue, for the vendors named or for every vendor that is switched on.
pub async fn catalog(store: &Store, vendors: Option<&[String]>, url: Option<String>) -> sqlx::Result<Catalog> {
    let listed = store.vendors().await?;
    let vendors_out: Vec<CatalogVendor> = listed
        .iter()
        .filter(|v| v.enabled && vendors.is_none_or(|names| names.contains(&v.id)))
        .map(vendor_of)
        .collect();
    let models = store.published(vendors).await?;
    Ok(Catalog {
        schema: SCHEMA_V1.to_owned(),
        generated_at: Some(Utc::now()),
        source: Source { kind: SourceKind::PacodAggregator, url, version: Some(env!("CARGO_PKG_VERSION").to_owned()) },
        vendors: vendors_out,
        models: models.iter().map(model_of).collect(),
    })
}
