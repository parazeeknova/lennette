use sea_orm::entity::prelude::*;
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "plan_upgrades")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub account_id: Uuid,
    pub subscription_id: String,
    pub period_start: DateTimeUtc,
    pub amount: i64,
    pub currency: String,
    pub status: String,
    pub response: Option<Json>,
    pub expires_at: DateTimeUtc,
    pub created_at: DateTimeUtc,
}
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}
impl ActiveModelBehavior for ActiveModel {}
