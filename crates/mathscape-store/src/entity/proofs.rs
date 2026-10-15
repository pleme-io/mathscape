use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "proofs")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub proof_id: i32,
    pub symbol_id: i32,
    pub proof_type: String,
    pub status: String,
    pub lhs_hash: Vec<u8>,
    pub rhs_hash: Vec<u8>,
    pub trace_ids: Vec<u8>,
    pub epoch_found: i32,
    pub epoch_verified: Option<i32>,
    pub lean_export: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::library::Entity",
        from = "Column::SymbolId",
        to = "super::library::Column::SymbolId"
    )]
    Library,
    #[sea_orm(has_many = "super::proof_deps::Entity")]
    ProofDeps,
}

impl Related<super::library::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Library.def()
    }
}

impl Related<super::proof_deps::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ProofDeps.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
