use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Lineage events (denormalized from redb graph for relational queries)
        manager
            .create_table(
                Table::create()
                    .table(LineageEvents::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(LineageEvents::EventId)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(LineageEvents::ChildHash).binary().not_null())
                    .col(ColumnDef::new(LineageEvents::Parent1Hash).binary())
                    .col(ColumnDef::new(LineageEvents::Parent2Hash).binary())
                    .col(ColumnDef::new(LineageEvents::MutationType).string().not_null())
                    .col(ColumnDef::new(LineageEvents::Operator).string())
                    .col(ColumnDef::new(LineageEvents::Epoch).integer().not_null())
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_lineage_child")
                    .table(LineageEvents::Table)
                    .col(LineageEvents::ChildHash)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_lineage_epoch")
                    .table(LineageEvents::Table)
                    .col(LineageEvents::Epoch)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_lineage_type")
                    .table(LineageEvents::Table)
                    .col(LineageEvents::MutationType)
                    .to_owned(),
            )
            .await?;

        // Symbol dependency summary (materialized from redb symbol_deps graph)
        manager
            .create_table(
                Table::create()
                    .table(SymbolDeps::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(SymbolDeps::SymbolId).integer().not_null())
                    .col(ColumnDef::new(SymbolDeps::DependsOn).integer().not_null())
                    .col(
                        ColumnDef::new(SymbolDeps::Depth)
                            .integer()
                            .not_null()
                            .default(1),
                    )
                    .primary_key(
                        Index::create()
                            .col(SymbolDeps::SymbolId)
                            .col(SymbolDeps::DependsOn),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(SymbolDeps::Table, SymbolDeps::SymbolId)
                            .to(Library::Table, Library::SymbolId),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(SymbolDeps::Table, SymbolDeps::DependsOn)
                            .to(Library::Table, Library::SymbolId),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_symdeps_dep")
                    .table(SymbolDeps::Table)
                    .col(SymbolDeps::DependsOn)
                    .to_owned(),
            )
            .await?;

        // Proof dependency summary (materialized from redb proof_deps graph)
        manager
            .create_table(
                Table::create()
                    .table(ProofDeps::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(ProofDeps::ProofId).integer().not_null())
                    .col(ColumnDef::new(ProofDeps::DependsOn).integer().not_null())
                    .primary_key(
                        Index::create()
                            .col(ProofDeps::ProofId)
                            .col(ProofDeps::DependsOn),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(ProofDeps::Table, ProofDeps::ProofId)
                            .to(Proofs::Table, Proofs::ProofId),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(ProofDeps::Table, ProofDeps::DependsOn)
                            .to(Proofs::Table, Proofs::ProofId),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_proofdeps_dep")
                    .table(ProofDeps::Table)
                    .col(ProofDeps::DependsOn)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(ProofDeps::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(SymbolDeps::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(LineageEvents::Table).to_owned()).await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum LineageEvents {
    Table,
    EventId,
    ChildHash,
    Parent1Hash,
    Parent2Hash,
    MutationType,
    Operator,
    Epoch,
}

// Reference to Library table from V001
#[derive(DeriveIden)]
enum Library {
    Table,
    SymbolId,
}

// Reference to Proofs table from V001
#[derive(DeriveIden)]
enum Proofs {
    Table,
    ProofId,
}

#[derive(DeriveIden)]
enum SymbolDeps {
    Table,
    SymbolId,
    DependsOn,
    Depth,
}

#[derive(DeriveIden)]
enum ProofDeps {
    Table,
    ProofId,
    DependsOn,
}
