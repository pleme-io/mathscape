use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "lineage_events")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub event_id: i32,
    
    pub child_hash: Vec<u8>,
    
    pub parent1_hash: Option<Vec<u8>>,
    
    pub parent2_hash: Option<Vec<u8>>,
    pub mutation_type: String,
    pub operator: Option<String>,
    pub epoch: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
