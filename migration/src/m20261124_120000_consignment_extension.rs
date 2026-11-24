//! Record that the consignment files saved before the `.rgb` extension have been renamed to have
//! it: the received ones, which had the `.rgbc` extension, and the sent ones and the issued
//! contracts, which had none.
//!
//! The DB schema is untouched: the files are renamed by rgb-lib, which can reach the wallet
//! directory, right before this migration is applied, so that it's applied only once they all are.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}
