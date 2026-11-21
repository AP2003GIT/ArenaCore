use async_trait::async_trait;
use sqlx::PgPool;
use thiserror::Error;

#[async_trait]
pub trait ReadinessProbe: Send + Sync {
    async fn check(&self) -> Result<(), ReadinessError>;
}

pub struct InMemoryReadiness;

#[async_trait]
impl ReadinessProbe for InMemoryReadiness {
    async fn check(&self) -> Result<(), ReadinessError> {
        Ok(())
    }
}

pub struct PostgresReadiness {
    pool: PgPool,
}

impl PostgresReadiness {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ReadinessProbe for PostgresReadiness {
    async fn check(&self) -> Result<(), ReadinessError> {
        sqlx::query("SELECT 1")
            .execute(&self.pool)
            .await
            .map_err(ReadinessError::Database)?;
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum ReadinessError {
    #[error("database readiness check failed")]
    Database(#[source] sqlx::Error),
}
