use sea_orm_migration::prelude::*;
pub struct Migrator;
#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(Initial), Box::new(BillingHolds)]
    }
}
struct BillingHolds;
impl MigrationName for BillingHolds {
    fn name(&self) -> &str {
        "m20261104_002_billing_holds"
    }
}
#[async_trait::async_trait]
impl MigrationTrait for BillingHolds {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.get_connection().execute_unprepared("CREATE TABLE billing_holds(subscription_id text PRIMARY KEY,reason text NOT NULL,created_at timestamptz NOT NULL DEFAULT now())").await?;
        Ok(())
    }
    async fn down(&self, _: &SchemaManager) -> Result<(), DbErr> {
        Err(DbErr::Custom("billing audit rollback is disabled".into()))
    }
}
struct Initial;
impl MigrationName for Initial {
    fn name(&self) -> &str {
        "m20261104_001_initial"
    }
}
#[async_trait::async_trait]
impl MigrationTrait for Initial {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(include_str!("../migrations/001_initial.sql"))
            .await?;
        Ok(())
    }
    async fn down(&self, _: &SchemaManager) -> Result<(), DbErr> {
        Err(DbErr::Custom(
            "destructive rollback is disabled; restore a verified backup".into(),
        ))
    }
}
