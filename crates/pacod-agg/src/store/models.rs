//! The catalogue: models and their prices.

use super::{Result, Store};

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
}
