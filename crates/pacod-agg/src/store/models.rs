//! The catalogue: models, their prices, and the log of what changed.

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;

use super::{Result, Store, new_id};

/// A model as it is stored.
#[derive(Debug, Clone, PartialEq, Serialize, sqlx::FromRow)]
pub struct ModelRow {
    pub id: String,
    pub vendor_id: String,
    pub api_id: String,
    pub key: String,
    pub aliases: Vec<String>,
    pub api_id_confirmed: bool,
    pub display_name: Option<String>,
    pub kind: Option<String>,
    pub status: String,
    pub deprecated_at: Option<DateTime<Utc>>,
    pub sunset_at: Option<DateTime<Utc>>,
    pub successor: Option<String>,
    pub properties: Value,
    pub parameters: Value,
    pub provenance: Value,
    pub first_seen_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub gone_at: Option<DateTime<Utc>>,
}

/// One version of a price.
#[derive(Debug, Clone, PartialEq, Serialize, sqlx::FromRow, JsonSchema)]
pub struct PriceRow {
    pub id: i64,
    pub model_id: String,
    /// The catalogue's price, without its dates.
    pub price: Value,
    pub effective_from: DateTime<Utc>,
    /// When its source last stated it.
    pub checked_at: DateTime<Utc>,
    /// When the next version replaced it. Absent for the price in effect.
    pub superseded_at: Option<DateTime<Utc>>,
}

/// One entry of the log of what changed.
#[derive(Debug, Clone, PartialEq, Serialize, sqlx::FromRow, JsonSchema)]
pub struct Change {
    pub id: i64,
    pub at: DateTime<Utc>,
    pub vendor_id: String,
    pub model_id: String,
    /// `created`, `price`, `gone`, `back`, or a property's name.
    pub field: String,
    pub old: Option<Value>,
    pub new: Option<Value>,
    pub run_id: Option<String>,
}

/// A model with the price in effect, for the catalogue.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Published {
    #[sqlx(flatten)]
    pub model: ModelRow,
    pub price: Option<Value>,
    pub effective_from: Option<DateTime<Utc>>,
    pub checked_at: Option<DateTime<Utc>>,
}

/// What becomes of a price when a vendor settles.
#[derive(Debug, Clone, PartialEq)]
pub enum PriceWrite {
    /// The source states it again. `checked_at` never moves back.
    Confirm { checked_at: DateTime<Utc> },
    /// A new version; the one in effect is closed.
    New { price: Value, checked_at: DateTime<Utc> },
}

/// What becomes of one model when a vendor settles.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ModelWrite {
    /// The model's id; `None` for one seen for the first time.
    pub id: Option<String>,
    pub api_id: String,
    pub key: String,
    pub aliases: Vec<String>,
    pub api_id_confirmed: bool,
    pub display_name: Option<String>,
    pub kind: Option<String>,
    pub properties: Value,
    pub parameters: Value,
    pub provenance: Value,
    pub price: Option<PriceWrite>,
    /// What changed: the field, what it was, what it is.
    pub changes: Vec<(String, Option<Value>, Option<Value>)>,
}

/// The filters of a listing of models.
#[derive(Debug, Clone, Default)]
pub struct ModelFilter<'a> {
    pub vendor_id: Option<&'a str>,
    pub status: Option<&'a str>,
    /// The id to continue after.
    pub cursor: Option<&'a str>,
    pub limit: i64,
}

const PUBLISHED: &str = "SELECT m.*, p.price, p.effective_from, p.checked_at
     FROM models m
     JOIN vendors v ON v.id = m.vendor_id
     LEFT JOIN price_versions p ON p.model_id = m.id AND p.superseded_at IS NULL
     WHERE v.enabled AND m.gone_at IS NULL";

impl Store {
    /// The ids the catalogue knows for a vendor, aliases included, for what the extraction model
    /// and the scan are told.
    pub async fn known_model_ids(&self, vendor_id: &str) -> Result<Vec<String>> {
        sqlx::query_scalar(
            "SELECT DISTINCT id FROM (SELECT api_id AS id FROM models WHERE vendor_id = $1 AND gone_at IS NULL
                                      UNION ALL
                                      SELECT unnest(aliases) FROM models WHERE vendor_id = $1 AND gone_at IS NULL) AS ids
             ORDER BY id",
        )
        .bind(vendor_id)
        .fetch_all(self.pool())
        .await
    }

    /// Every model of a vendor, gone ones too: a model that comes back keeps its row.
    pub async fn models_of(&self, vendor_id: &str) -> Result<Vec<ModelRow>> {
        sqlx::query_as("SELECT * FROM models WHERE vendor_id = $1 ORDER BY key")
            .bind(vendor_id)
            .fetch_all(self.pool())
            .await
    }

    /// The prices in effect of a vendor's models.
    pub async fn prices_of(&self, vendor_id: &str) -> Result<Vec<PriceRow>> {
        sqlx::query_as(
            "SELECT p.* FROM price_versions p JOIN models m ON m.id = p.model_id
             WHERE m.vendor_id = $1 AND p.superseded_at IS NULL",
        )
        .bind(vendor_id)
        .fetch_all(self.pool())
        .await
    }

    /// Applies what a vendor's pages say, in one transaction: a reader never sees half of it.
    /// `gone` names the models no page names any more. `read_complete` says whether every page
    /// was read.
    pub async fn settle(
        &self,
        vendor_id: &str,
        writes: &[ModelWrite],
        gone: &[String],
        read_complete: bool,
        run_id: Option<&str>,
    ) -> Result<()> {
        let mut tx = self.pool().begin().await?;
        for w in writes {
            let id = w.id.clone().unwrap_or_else(|| new_id("agm"));
            sqlx::query(
                "INSERT INTO models (id, vendor_id, api_id, key, aliases, api_id_confirmed, display_name, kind,
                     properties, parameters, provenance)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
                 ON CONFLICT (id) DO UPDATE SET api_id = $3, key = $4, aliases = $5, api_id_confirmed = $6,
                     display_name = $7, kind = $8, properties = $9, parameters = $10, provenance = $11,
                     last_seen_at = now(), gone_at = NULL",
            )
            .bind(&id)
            .bind(vendor_id)
            .bind(&w.api_id)
            .bind(&w.key)
            .bind(&w.aliases)
            .bind(w.api_id_confirmed)
            .bind(&w.display_name)
            .bind(&w.kind)
            .bind(&w.properties)
            .bind(&w.parameters)
            .bind(&w.provenance)
            .execute(&mut *tx)
            .await?;
            match &w.price {
                Some(PriceWrite::Confirm { checked_at }) => {
                    sqlx::query(
                        "UPDATE price_versions SET checked_at = greatest(checked_at, $2)
                         WHERE model_id = $1 AND superseded_at IS NULL",
                    )
                    .bind(&id)
                    .bind(checked_at)
                    .execute(&mut *tx)
                    .await?;
                }
                Some(PriceWrite::New { price, checked_at }) => {
                    sqlx::query(
                        "UPDATE price_versions SET superseded_at = now() WHERE model_id = $1 AND superseded_at IS NULL",
                    )
                    .bind(&id)
                    .execute(&mut *tx)
                    .await?;
                    sqlx::query("INSERT INTO price_versions (model_id, price, checked_at) VALUES ($1, $2, $3)")
                        .bind(&id)
                        .bind(price)
                        .bind(checked_at)
                        .execute(&mut *tx)
                        .await?;
                }
                None => {}
            }
            for (field, old, new) in &w.changes {
                sqlx::query(
                    "INSERT INTO changes (vendor_id, model_id, field, old, new, run_id) VALUES ($1, $2, $3, $4, $5, $6)",
                )
                .bind(vendor_id)
                .bind(&id)
                .bind(field)
                .bind(old)
                .bind(new)
                .bind(run_id)
                .execute(&mut *tx)
                .await?;
            }
        }
        for id in gone {
            sqlx::query("UPDATE models SET gone_at = now() WHERE id = $1").bind(id).execute(&mut *tx).await?;
            sqlx::query("INSERT INTO changes (vendor_id, model_id, field, run_id) VALUES ($1, $2, 'gone', $3)")
                .bind(vendor_id)
                .bind(id)
                .bind(run_id)
                .execute(&mut *tx)
                .await?;
        }
        sqlx::query("UPDATE vendors SET read_complete = $2, checked_at = now() WHERE id = $1")
            .bind(vendor_id)
            .bind(read_complete)
            .execute(&mut *tx)
            .await?;
        tx.commit().await
    }

    /// What the catalogue publishes, by vendor and id: the models of the vendors switched on that
    /// are not gone, with the price in effect. `vendors` limits it.
    pub async fn published(&self, vendors: Option<&[String]>) -> Result<Vec<Published>> {
        sqlx::query_as(&format!(
            "{PUBLISHED} AND ($1::text[] IS NULL OR m.vendor_id = ANY($1)) ORDER BY m.vendor_id, m.key"
        ))
        .bind(vendors)
        .fetch_all(self.pool())
        .await
    }

    /// A page of the published models, by id.
    pub async fn published_page(&self, f: &ModelFilter<'_>) -> Result<Vec<Published>> {
        sqlx::query_as(&format!(
            "{PUBLISHED} AND ($1::text IS NULL OR m.vendor_id = $1) AND ($2::text IS NULL OR m.status = $2)
             AND ($3::text IS NULL OR m.id > $3) ORDER BY m.id LIMIT $4"
        ))
        .bind(f.vendor_id)
        .bind(f.status)
        .bind(f.cursor)
        .bind(f.limit)
        .fetch_all(self.pool())
        .await
    }

    /// One published model.
    pub async fn published_one(&self, id: &str) -> Result<Option<Published>> {
        sqlx::query_as(&format!("{PUBLISHED} AND m.id = $1")).bind(id).fetch_optional(self.pool()).await
    }

    /// Every version of a model's price, newest first.
    pub async fn price_history(&self, model_id: &str) -> Result<Vec<PriceRow>> {
        sqlx::query_as("SELECT * FROM price_versions WHERE model_id = $1 ORDER BY id DESC")
            .bind(model_id)
            .fetch_all(self.pool())
            .await
    }

    /// What changed, newest first, for one model or for all.
    pub async fn changes(&self, model_id: Option<&str>, limit: i64) -> Result<Vec<Change>> {
        sqlx::query_as(
            "SELECT * FROM changes WHERE ($1::text IS NULL OR model_id = $1) ORDER BY at DESC, id DESC LIMIT $2",
        )
        .bind(model_id)
        .bind(limit)
        .fetch_all(self.pool())
        .await
    }
}
