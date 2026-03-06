use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Population snapshot (bulk-replaced each epoch)
        manager
            .create_table(
                Table::create()
                    .table(Population::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Population::Epoch).integer().not_null())
                    .col(ColumnDef::new(Population::Individual).integer().not_null())
                    .col(ColumnDef::new(Population::RootHash).binary().not_null())
                    .col(ColumnDef::new(Population::Fitness).double().not_null())
                    .col(ColumnDef::new(Population::CrContrib).double())
                    .col(ColumnDef::new(Population::Novelty).double())
                    .col(ColumnDef::new(Population::DepthBin).integer())
                    .col(ColumnDef::new(Population::OpDiversity).integer())
                    .col(ColumnDef::new(Population::CrBin).integer())
                    .primary_key(
                        Index::create()
                            .col(Population::Epoch)
                            .col(Population::Individual),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_pop_epoch")
                    .table(Population::Table)
                    .col(Population::Epoch)
                    .to_owned(),
            )
            .await?;

        // Library of discovered symbols (append-only)
        manager
            .create_table(
                Table::create()
                    .table(Library::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Library::SymbolId)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Library::Name).string().not_null())
                    .col(ColumnDef::new(Library::EpochDiscovered).integer().not_null())
                    .col(ColumnDef::new(Library::LhsHash).binary().not_null())
                    .col(ColumnDef::new(Library::RhsHash).binary().not_null())
                    .col(ColumnDef::new(Library::Arity).integer().not_null())
                    .col(ColumnDef::new(Library::Generality).double())
                    .col(ColumnDef::new(Library::Irreducibility).double())
                    .col(
                        ColumnDef::new(Library::IsMeta)
                            .boolean()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(Library::Status)
                            .string()
                            .not_null()
                            .default("active"),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_library_epoch")
                    .table(Library::Table)
                    .col(Library::EpochDiscovered)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_library_name")
                    .table(Library::Table)
                    .col(Library::Name)
                    .to_owned(),
            )
            .await?;

        // Epoch-level metrics (one row per epoch, append-only)
        manager
            .create_table(
                Table::create()
                    .table(Epochs::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Epochs::Epoch)
                            .integer()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Epochs::CompressionRatio).double().not_null())
                    .col(ColumnDef::new(Epochs::DescriptionLength).integer().not_null())
                    .col(ColumnDef::new(Epochs::RawLength).integer().not_null())
                    .col(ColumnDef::new(Epochs::NoveltyTotal).double().not_null())
                    .col(ColumnDef::new(Epochs::MetaCompression).double().not_null())
                    .col(ColumnDef::new(Epochs::LibrarySize).integer().not_null())
                    .col(ColumnDef::new(Epochs::PopulationDiversity).double())
                    .col(ColumnDef::new(Epochs::ExpressionCount).integer())
                    .col(ColumnDef::new(Epochs::Alpha).double().not_null())
                    .col(ColumnDef::new(Epochs::Beta).double().not_null())
                    .col(ColumnDef::new(Epochs::Gamma).double().not_null())
                    .col(ColumnDef::new(Epochs::Phase).string())
                    .col(ColumnDef::new(Epochs::DurationMs).integer())
                    .col(ColumnDef::new(Epochs::StartedAt).timestamp_with_time_zone())
                    .col(ColumnDef::new(Epochs::CompletedAt).timestamp_with_time_zone())
                    .to_owned(),
            )
            .await?;

        // Evaluation traces (atomic proof steps)
        manager
            .create_table(
                Table::create()
                    .table(EvalTraces::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(EvalTraces::TraceId)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(EvalTraces::ExprHash).binary().not_null())
                    .col(ColumnDef::new(EvalTraces::StepIndex).integer().not_null())
                    .col(ColumnDef::new(EvalTraces::RuleApplied).string().not_null())
                    .col(ColumnDef::new(EvalTraces::BeforeHash).binary().not_null())
                    .col(ColumnDef::new(EvalTraces::AfterHash).binary().not_null())
                    .col(ColumnDef::new(EvalTraces::Epoch).integer().not_null())
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_traces_expr")
                    .table(EvalTraces::Table)
                    .col(EvalTraces::ExprHash)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_traces_epoch")
                    .table(EvalTraces::Table)
                    .col(EvalTraces::Epoch)
                    .to_owned(),
            )
            .await?;

        // Proof certificates
        manager
            .create_table(
                Table::create()
                    .table(Proofs::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Proofs::ProofId)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Proofs::SymbolId).integer().not_null())
                    .col(ColumnDef::new(Proofs::ProofType).string().not_null())
                    .col(ColumnDef::new(Proofs::Status).string().not_null())
                    .col(ColumnDef::new(Proofs::LhsHash).binary().not_null())
                    .col(ColumnDef::new(Proofs::RhsHash).binary().not_null())
                    .col(ColumnDef::new(Proofs::TraceIds).binary().not_null())
                    .col(ColumnDef::new(Proofs::EpochFound).integer().not_null())
                    .col(ColumnDef::new(Proofs::EpochVerified).integer())
                    .col(ColumnDef::new(Proofs::LeanExport).text())
                    .foreign_key(
                        ForeignKey::create()
                            .from(Proofs::Table, Proofs::SymbolId)
                            .to(Library::Table, Library::SymbolId),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_proofs_symbol")
                    .table(Proofs::Table)
                    .col(Proofs::SymbolId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_proofs_status")
                    .table(Proofs::Table)
                    .col(Proofs::Status)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_proofs_epoch")
                    .table(Proofs::Table)
                    .col(Proofs::EpochFound)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(Proofs::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(EvalTraces::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(Epochs::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(Library::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(Population::Table).to_owned()).await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum Population {
    Table,
    Epoch,
    Individual,
    RootHash,
    Fitness,
    CrContrib,
    Novelty,
    DepthBin,
    OpDiversity,
    CrBin,
}

#[derive(DeriveIden)]
enum Library {
    Table,
    SymbolId,
    Name,
    EpochDiscovered,
    LhsHash,
    RhsHash,
    Arity,
    Generality,
    Irreducibility,
    IsMeta,
    Status,
}

#[derive(DeriveIden)]
enum Epochs {
    Table,
    Epoch,
    CompressionRatio,
    DescriptionLength,
    RawLength,
    NoveltyTotal,
    MetaCompression,
    LibrarySize,
    PopulationDiversity,
    ExpressionCount,
    Alpha,
    Beta,
    Gamma,
    Phase,
    DurationMs,
    StartedAt,
    CompletedAt,
}

#[derive(DeriveIden)]
enum EvalTraces {
    Table,
    TraceId,
    ExprHash,
    StepIndex,
    RuleApplied,
    BeforeHash,
    AfterHash,
    Epoch,
}

#[derive(DeriveIden)]
enum Proofs {
    Table,
    ProofId,
    SymbolId,
    ProofType,
    Status,
    LhsHash,
    RhsHash,
    TraceIds,
    EpochFound,
    EpochVerified,
    LeanExport,
}
