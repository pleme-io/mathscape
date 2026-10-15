use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
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
        belongs_to = "super::proofs::Entity",
        from = "Column::ProofId",
        to = "super::proofs::Column::ProofId"
    )]
    Proof,
    #[sea_orm(
        belongs_to = "super::proofs::Entity",
        from = "Column::DependsOn",
        to = "super::proofs::Column::ProofId"
    )]
    Dependency,
}

impl Related<super::proofs::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Proof.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
