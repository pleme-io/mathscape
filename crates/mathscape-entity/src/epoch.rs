use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "epochs")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub epoch: i32,
    pub compression_ratio: f64,
    pub description_length: i32,
    pub raw_length: i32,
    pub novelty_total: f64,
    pub meta_compression: f64,
    pub library_size: i32,
    pub population_diversity: Option<f64>,
    pub expression_count: Option<i32>,
    pub alpha: f64,
    pub beta: f64,
    pub gamma: f64,
    pub phase: Option<String>,
    pub duration_ms: Option<i32>,
    pub started_at: Option<DateTimeWithTimeZone>,
    pub completed_at: Option<DateTimeWithTimeZone>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
