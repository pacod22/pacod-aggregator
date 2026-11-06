//! The aggregator's database (PostgreSQL, database `pacod_agg`). The only SQL.

pub mod jobs;
pub mod models;
pub mod pages;
pub mod settings;
pub mod vendors;

use sqlx::postgres::{PgPool, PgPoolOptions};

pub type Result<T> = std::result::Result<T, sqlx::Error>;

#[derive(Clone)]
pub struct Store {
    pool: PgPool,
}

/// A new id: a prefix and a time-ordered UUID.
pub fn new_id(prefix: &str) -> String {
    format!("{prefix}_{}", uuid::Uuid::now_v7().simple())
}

impl Store {
    pub async fn connect(url: &str) -> Result<Store> {
        Ok(Store { pool: PgPoolOptions::new().max_connections(10).connect(url).await? })
    }

    pub async fn migrate(&self) -> std::result::Result<(), sqlx::migrate::MigrateError> {
        sqlx::migrate!("./migrations").run(&self.pool).await
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Whether the database answers.
    pub async fn ping(&self) -> bool {
        sqlx::query("SELECT 1").execute(&self.pool).await.is_ok()
    }
}
