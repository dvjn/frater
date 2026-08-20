use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "exercise_sets")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub session_exercise_id: String,
    pub user_id: String,
    pub contraction_type: String,
    pub position: i64,
    pub set_type: String,
    pub reps: Option<i64>,
    pub hold_sec: Option<i64>,
    pub load_g: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}
impl ActiveModelBehavior for ActiveModel {}
