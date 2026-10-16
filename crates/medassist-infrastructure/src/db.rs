//! Shared PostgreSQL pool wrapper and migration runner.

use sqlx::postgres::{PgPool, PgPoolOptions};

#[derive(Clone)]
pub struct Db(pub PgPool);

impl Db {
    pub async fn connect(url: &str, max_conns: u32) -> Result<Self, sqlx::Error> {
        let pool = PgPoolOptions::new()
            .max_connections(max_conns)
            .acquire_timeout(std::time::Duration::from_secs(10))
            .connect(url)
            .await?;
        Ok(Self(pool))
    }

    /// Run embedded migrations from the `migrations/` directory at compile-time
    /// path relative to the workspace root (resolved via env or CWD).
    pub async fn migrate(&self, dir: &std::path::Path) -> Result<(), sqlx::migrate::MigrateError> {
        sqlx::migrate::Migrator::new(dir.to_path_buf())
            .await?
            .run(&self.0)
            .await
    }

    pub fn pool(&self) -> &PgPool {
        &self.0
    }
}
