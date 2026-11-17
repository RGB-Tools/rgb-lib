//! Add a `bridge_location` JSON column to the `asset` table, holding the location of the bridge
//! contract the genesis of a bridged asset commits to (see `RgbBridgeLocationInfo`).

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Asset::Table)
                    .add_column(ColumnDef::new(Asset::BridgeLocation).json().null())
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Asset::Table)
                    .drop_column(Asset::BridgeLocation)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
pub enum Asset {
    Table,
    BridgeLocation,
}
