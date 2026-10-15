use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "population")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub epoch: i32,
    #[sea_orm(primary_key, auto_increment = false)]
    pub individual: i32,
    
    pub root_hash: Vec<u8>,
    pub fitness: f64,
    pub cr_contrib: Option<f64>,
    pub novelty: Option<f64>,
    pub depth_bin: Option<i32>,
    pub op_diversity: Option<i32>,
    pub cr_bin: Option<i32>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
