use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "exercise_muscles")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub exercise_id: String,
    #[sea_orm(primary_key, auto_increment = false)]
    pub muscle_id: String,
    pub role: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}
impl ActiveModelBehavior for ActiveModel {}
