use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "proof_deps")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub proof_id: i32,
    #[sea_orm(primary_key, auto_increment = false)]
    pub depends_on: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::proof::Entity",
        from = "Column::ProofId",
        to = "super::proof::Column::ProofId"
    )]
    Proof,
}

impl Related<super::proof::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Proof.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
