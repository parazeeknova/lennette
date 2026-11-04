use sea_orm::entity::prelude::*;
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "sessions")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub account_id: Uuid,
    pub device_id: Uuid,
    pub device_name: String,
    pub access_hash: String,
    pub refresh_hash: String,
    pub access_expires_at: DateTimeUtc,
    pub refresh_expires_at: DateTimeUtc,
    pub revoked_at: Option<DateTimeUtc>,
    pub created_at: DateTimeUtc,
}
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}
impl ActiveModelBehavior for ActiveModel {}
