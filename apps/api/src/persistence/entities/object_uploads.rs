use sea_orm::entity::prelude::*;
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "object_uploads")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub account_id: Uuid,
    #[sea_orm(primary_key, auto_increment = false)]
    pub attachment_id: Uuid,
    pub created_at: DateTimeUtc,
}
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}
impl ActiveModelBehavior for ActiveModel {}
