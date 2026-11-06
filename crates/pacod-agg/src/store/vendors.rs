//! Vendors.

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{Result, Store};

/// A company whose API serves models.
#[derive(Debug, Clone, PartialEq, Serialize, sqlx::FromRow, JsonSchema)]
pub struct Vendor {
    pub id: String,
    pub display_name: String,
    /// Where the weekly scan starts for a vendor with no page there yet.
    pub sites: Vec<String>,
    pub currency: String,
    /// A vendor switched off is not read, and not published.
    pub enabled: bool,
    /// An administrator's word that its pages list all of its models.
    pub complete: bool,
    /// Whether every active page was read without failure at the last check. The catalogue says
    /// a vendor is complete only then.
    pub read_complete: bool,
    pub checked_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A vendor as an administrator adds or changes it.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct VendorInput {
    pub id: String,
    pub display_name: String,
    #[serde(default)]
    pub sites: Vec<String>,
    #[serde(default = "usd")]
    pub currency: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub complete: bool,
}

fn usd() -> String {
    "USD".to_owned()
}

fn yes() -> bool {
    true
}

impl Store {
    pub async fn vendors(&self) -> Result<Vec<Vendor>> {
        sqlx::query_as("SELECT * FROM vendors ORDER BY id").fetch_all(self.pool()).await
    }

    pub async fn vendor(&self, id: &str) -> Result<Option<Vendor>> {
        sqlx::query_as("SELECT * FROM vendors WHERE id = $1").bind(id).fetch_optional(self.pool()).await
    }

    /// Creates a vendor; `None` when the id is taken.
    pub async fn vendor_create(&self, v: &VendorInput) -> Result<Option<Vendor>> {
        sqlx::query_as(
            "INSERT INTO vendors (id, display_name, sites, currency, enabled, complete)
             VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (id) DO NOTHING RETURNING *",
        )
        .bind(&v.id)
        .bind(&v.display_name)
        .bind(&v.sites)
        .bind(&v.currency)
        .bind(v.enabled)
        .bind(v.complete)
        .fetch_optional(self.pool())
        .await
    }

    pub async fn vendor_update(&self, id: &str, v: &VendorInput) -> Result<Option<Vendor>> {
        sqlx::query_as(
            "UPDATE vendors SET display_name = $2, sites = $3, currency = $4, enabled = $5, complete = $6,
                 updated_at = now()
             WHERE id = $1 RETURNING *",
        )
        .bind(id)
        .bind(&v.display_name)
        .bind(&v.sites)
        .bind(&v.currency)
        .bind(v.enabled)
        .bind(v.complete)
        .fetch_optional(self.pool())
        .await
    }

    pub async fn vendor_delete(&self, id: &str) -> Result<bool> {
        Ok(sqlx::query("DELETE FROM vendors WHERE id = $1").bind(id).execute(self.pool()).await?.rows_affected() > 0)
    }
}
