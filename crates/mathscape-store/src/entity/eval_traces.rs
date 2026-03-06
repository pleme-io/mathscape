use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "eval_traces")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub trace_id: i32,
    pub expr_hash: Vec<u8>,
    pub step_index: i32,
    pub rule_applied: String,
    pub before_hash: Vec<u8>,
    pub after_hash: Vec<u8>,
    pub epoch: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
