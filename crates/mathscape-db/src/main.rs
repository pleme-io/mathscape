use clap::{Parser, Subcommand};
use mathscape_migration::Migrator;
use sea_orm::{Database, DbErr};
use sea_orm_migration::MigratorTrait;

#[derive(Parser)]
#[command(name = "mathscape-db")]
#[command(about = "Database management for mathscape PostgreSQL metadata")]
struct Cli {
    /// PostgreSQL connection URL
    #[arg(
        long,
        env = "DATABASE_URL",
        default_value = "postgres://localhost/mathscape"
    )]
    database_url: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run all pending migrations
    Migrate,
    /// Roll back the last applied migration
    Rollback,
    /// Show migration status
    Status,
    /// Verify database schema matches expected state
    Verify,
    /// Reset database (drop all tables, re-run migrations)
    Reset,
}

#[tokio::main]
async fn main() -> Result<(), DbErr> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();
    let db = Database::connect(&cli.database_url).await?;

    match cli.command {
        Commands::Migrate => {
            tracing::info!("running pending migrations");
            Migrator::up(&db, None).await?;
            tracing::info!("migrations complete");
        }
        Commands::Rollback => {
            tracing::info!("rolling back last migration");
            Migrator::down(&db, Some(1)).await?;
            tracing::info!("rollback complete");
        }
        Commands::Status => {
            Migrator::status(&db).await?;
        }
        Commands::Verify => {
            tracing::info!("verifying schema");
            Migrator::status(&db).await?;
            tracing::info!("schema verification complete");
        }
        Commands::Reset => {
            tracing::warn!("resetting database — dropping all tables");
            Migrator::fresh(&db).await?;
            tracing::info!("database reset complete");
        }
    }

    Ok(())
}
