use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "library")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub symbol_id: i32,
    pub name: String,
    pub epoch_discovered: i32,
    pub lhs_hash: Vec<u8>,
    pub rhs_hash: Vec<u8>,
    pub arity: i32,
    pub generality: Option<f64>,
    pub irreducibility: Option<f64>,
    pub is_meta: Option<bool>,
    pub status: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::proofs::Entity")]
    Proofs,
    #[sea_orm(has_many = "super::symbol_deps::Entity")]
    SymbolDeps,
}

impl Related<super::proofs::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Proofs.def()
    }
}

impl Related<super::symbol_deps::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::SymbolDeps.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
