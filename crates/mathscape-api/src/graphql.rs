//! GraphQL schema — query and mutation roots.
//!
//! The schema uses the shared API types (which derive `SimpleObject`),
//! so there is zero duplication between REST and GraphQL representations.

use async_graphql::{Context, Object, Result};

use crate::types::{
    ConfigUpdate, ControlResponse, EpochList, EngineConfig, LibraryList, Status,
};

/// Trait for the engine state provider — implemented by the service crate.
/// This decouples the GraphQL schema from any concrete engine implementation.
pub trait EngineProvider: Send + Sync + 'static {
    fn status(&self) -> impl std::future::Future<Output = Status> + Send;
    fn epochs(
        &self,
        limit: i32,
        offset: i32,
    ) -> impl std::future::Future<Output = EpochList> + Send;
    fn library(&self) -> impl std::future::Future<Output = LibraryList> + Send;
    fn config(&self) -> EngineConfig;
    fn update_config(&self, update: ConfigUpdate) -> ControlResponse;
    fn pause(&self) -> ControlResponse;
    fn resume(&self) -> ControlResponse;
}

pub struct QueryRoot;

#[Object]
impl QueryRoot {
    /// Current engine status including epoch, population stats, and latest reward.
    async fn status<'ctx>(&self, ctx: &Context<'ctx>) -> Result<Status> {
        let provider = ctx.data::<Box<dyn EngineProviderDyn>>()?;
        Ok(provider.status().await)
    }

    /// Epoch metrics history, ordered by epoch descending.
    async fn epochs<'ctx>(
        &self,
        ctx: &Context<'ctx>,
        #[graphql(default = 50)] limit: i32,
        #[graphql(default = 0)] offset: i32,
    ) -> Result<EpochList> {
        let provider = ctx.data::<Box<dyn EngineProviderDyn>>()?;
        Ok(provider.epochs(limit, offset).await)
    }

    /// All discovered library symbols.
    async fn library<'ctx>(&self, ctx: &Context<'ctx>) -> Result<LibraryList> {
        let provider = ctx.data::<Box<dyn EngineProviderDyn>>()?;
        Ok(provider.library().await)
    }

    /// Current engine configuration.
    async fn config<'ctx>(&self, ctx: &Context<'ctx>) -> Result<EngineConfig> {
        let provider = ctx.data::<Box<dyn EngineProviderDyn>>()?;
        Ok(provider.config())
    }
}

pub struct MutationRoot;

#[Object]
impl MutationRoot {
    /// Pause the engine — stop computing epochs.
    async fn pause<'ctx>(&self, ctx: &Context<'ctx>) -> Result<ControlResponse> {
        let provider = ctx.data::<Box<dyn EngineProviderDyn>>()?;
        Ok(provider.pause())
    }

    /// Resume the engine.
    async fn resume<'ctx>(&self, ctx: &Context<'ctx>) -> Result<ControlResponse> {
        let provider = ctx.data::<Box<dyn EngineProviderDyn>>()?;
        Ok(provider.resume())
    }

    /// Update engine configuration at runtime.
    async fn update_config<'ctx>(
        &self,
        ctx: &Context<'ctx>,
        input: ConfigUpdate,
    ) -> Result<ControlResponse> {
        let provider = ctx.data::<Box<dyn EngineProviderDyn>>()?;
        Ok(provider.update_config(input))
    }
}

/// Object-safe version of EngineProvider for use in GraphQL context.
#[async_trait::async_trait]
pub trait EngineProviderDyn: Send + Sync {
    async fn status(&self) -> Status;
    async fn epochs(&self, limit: i32, offset: i32) -> EpochList;
    async fn library(&self) -> LibraryList;
    fn config(&self) -> EngineConfig;
    fn update_config(&self, update: ConfigUpdate) -> ControlResponse;
    fn pause(&self) -> ControlResponse;
    fn resume(&self) -> ControlResponse;
}

pub type Schema = async_graphql::Schema<QueryRoot, MutationRoot, async_graphql::EmptySubscription>;

/// Build the GraphQL schema with the given engine provider.
pub fn build_schema(provider: Box<dyn EngineProviderDyn>) -> Schema {
    async_graphql::Schema::build(QueryRoot, MutationRoot, async_graphql::EmptySubscription)
        .data(provider)
        .finish()
}
