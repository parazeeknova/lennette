use sea_orm_migration::prelude::*;
pub struct Migrator;
#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(Initial),
            Box::new(BillingHolds),
            Box::new(ObjectUploads),
            Box::new(AccountEvents),
            Box::new(AttachmentRefs),
            Box::new(PlanUpgrades),
            Box::new(Workspace),
        ]
    }
}
struct Workspace;
impl MigrationName for Workspace {
    fn name(&self) -> &str {
        "m20261105_007_workspace"
    }
}
#[async_trait::async_trait]
impl MigrationTrait for Workspace {
    fn use_transaction(&self) -> Option<bool> {
        Some(false)
    }

    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let connection = manager.get_connection();
        let sql = include_str!("../../migrations/007_workspace.sql")
            .lines()
            .filter(|line| !line.trim_start().starts_with("--"))
            .collect::<Vec<_>>()
            .join("\n");
        for statement in sql.split(';').map(str::trim).filter(|s| !s.is_empty()) {
            // Names come only from the embedded migration, never external input.
            let name = statement
                .split_whitespace()
                .nth(6)
                .ok_or_else(|| DbErr::Custom("missing workspace index name".into()))?;
            let invalid = connection.query_one_raw(sea_orm::Statement::from_sql_and_values(
                sea_orm::DbBackend::Postgres,
                "SELECT NOT indisvalid AS invalid FROM pg_index WHERE indexrelid=to_regclass($1)",
                [name.into()],
            )).await?.map(|row| row.try_get::<bool>("", "invalid")).transpose()?.unwrap_or(false);
            if invalid {
                connection
                    .execute_unprepared(&format!("DROP INDEX CONCURRENTLY IF EXISTS {name}"))
                    .await?;
            }
            connection.execute_unprepared(statement).await?;
        }
        Ok(())
    }
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for name in [
            "records_workspace_list",
            "records_workspace_conversation",
            "records_workspace_meeting",
            "attachments_workspace_list",
        ] {
            manager
                .get_connection()
                .execute_unprepared(&format!("DROP INDEX CONCURRENTLY IF EXISTS {name}"))
                .await?;
        }
        Ok(())
    }
}
struct PlanUpgrades;
impl MigrationName for PlanUpgrades {
    fn name(&self) -> &str {
        "m20261104_006_plan_upgrades"
    }
}
#[async_trait::async_trait]
impl MigrationTrait for PlanUpgrades {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(include_str!("../../migrations/006_plan_upgrades.sql"))
            .await?;
        Ok(())
    }
    async fn down(&self, _: &SchemaManager) -> Result<(), DbErr> {
        Err(DbErr::Custom(
            "payment attempt audit rollback disabled".into(),
        ))
    }
}
struct AttachmentRefs;
impl MigrationName for AttachmentRefs {
    fn name(&self) -> &str {
        "m20261104_005_attachment_refs"
    }
}
#[async_trait::async_trait]
impl MigrationTrait for AttachmentRefs {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(include_str!("../../migrations/005_attachment_refs.sql"))
            .await?;
        Ok(())
    }
    async fn down(&self, _: &SchemaManager) -> Result<(), DbErr> {
        Err(DbErr::Custom(
            "attachment reference rollback disabled".into(),
        ))
    }
}
struct AccountEvents;
impl MigrationName for AccountEvents {
    fn name(&self) -> &str {
        "m20261104_004_account_events"
    }
}
#[async_trait::async_trait]
impl MigrationTrait for AccountEvents {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(include_str!("../../migrations/004_account_events.sql"))
            .await?;
        Ok(())
    }
    async fn down(&self, _: &SchemaManager) -> Result<(), DbErr> {
        Err(DbErr::Custom("event rollback disabled".into()))
    }
}
struct ObjectUploads;
impl MigrationName for ObjectUploads {
    fn name(&self) -> &str {
        "m20261104_003_object_uploads"
    }
}
#[async_trait::async_trait]
impl MigrationTrait for ObjectUploads {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(include_str!("../../migrations/003_object_uploads.sql"))
            .await?;
        Ok(())
    }
    async fn down(&self, _: &SchemaManager) -> Result<(), DbErr> {
        Err(DbErr::Custom(
            "pending object cleanup must not be discarded".into(),
        ))
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
            .execute_unprepared(include_str!("../../migrations/001_initial.sql"))
            .await?;
        Ok(())
    }
    async fn down(&self, _: &SchemaManager) -> Result<(), DbErr> {
        Err(DbErr::Custom(
            "destructive rollback is disabled; restore a verified backup".into(),
        ))
    }
}
