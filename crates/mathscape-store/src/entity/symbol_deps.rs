use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "symbol_deps")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub symbol_id: i32,
    #[sea_orm(primary_key, auto_increment = false)]
    pub depends_on: i32,
    pub depth: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::library::Entity",
        from = "Column::SymbolId",
        to = "super::library::Column::SymbolId"
    )]
    Symbol,
    #[sea_orm(
        belongs_to = "super::library::Entity",
        from = "Column::DependsOn",
        to = "super::library::Column::SymbolId"
    )]
    Dependency,
}

impl Related<super::library::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Symbol.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
