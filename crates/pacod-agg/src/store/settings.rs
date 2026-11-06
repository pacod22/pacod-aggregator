//! The settings document.

use serde_json::Value;

use super::{Result, Store};

impl Store {
    /// The stored document and its version.
    pub async fn settings(&self) -> Result<(Value, i32)> {
        sqlx::query_as("SELECT doc, version FROM settings").fetch_one(self.pool()).await
    }

    /// The vendors already seeded.
    pub async fn seeded(&self) -> Result<Vec<String>> {
        sqlx::query_scalar("SELECT seeded FROM settings").fetch_one(self.pool()).await
    }

    /// Notes that a vendor was seeded.
    pub async fn seeded_add(&self, vendor: &str) -> Result<()> {
        sqlx::query("UPDATE settings SET seeded = array_append(seeded, $1)").bind(vendor).execute(self.pool()).await?;
        Ok(())
    }

    /// Writes the document when `version` is the current one, and answers the new version.
    pub async fn settings_save(&self, doc: &Value, version: i32) -> Result<Option<i32>> {
        sqlx::query_scalar("UPDATE settings SET doc = $1, version = version + 1 WHERE version = $2 RETURNING version")
            .bind(doc)
            .bind(version)
            .fetch_optional(self.pool())
            .await
    }
}
