use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Txo::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Txo::Idx)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Txo::Txid).string().not_null())
                    .col(ColumnDef::new(Txo::Vout).unsigned().not_null())
                    .col(ColumnDef::new(Txo::BtcAmount).string().not_null())
                    .col(ColumnDef::new(Txo::Colorable).boolean().not_null())
                    .col(ColumnDef::new(Txo::Spent).boolean().not_null())
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                sea_query::Index::create()
                    .name("idx-txo-txid-vout")
                    .table(Txo::Table)
                    .col(Txo::Txid)
                    .col(Txo::Vout)
                    .unique()
                    .clone(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(AssetRgb20::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(AssetRgb20::Idx)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(AssetRgb20::AssetId)
                            .string()
                            .not_null()
                            .unique_key(),
                    )
                    .col(ColumnDef::new(AssetRgb20::Ticker).string().not_null())
                    .col(ColumnDef::new(AssetRgb20::Name).string().not_null())
                    .col(
                        ColumnDef::new(AssetRgb20::Precision)
                            .small_unsigned()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(AssetRgb25::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(AssetRgb25::Idx)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(AssetRgb25::AssetId)
                            .string()
                            .not_null()
                            .unique_key(),
                    )
                    .col(ColumnDef::new(AssetRgb25::Name).string().not_null())
                    .col(
                        ColumnDef::new(AssetRgb25::Precision)
                            .small_unsigned()
                            .not_null(),
                    )
                    .col(ColumnDef::new(AssetRgb25::Description).string())
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(BatchTransfer::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(BatchTransfer::Idx)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(BatchTransfer::Txid).string())
                    .col(
                        ColumnDef::new(BatchTransfer::Status)
                            .small_unsigned()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(BatchTransfer::CreatedAt)
                            .big_unsigned()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(BatchTransfer::UpdatedAt)
                            .big_unsigned()
                            .not_null(),
                    )
                    .col(ColumnDef::new(BatchTransfer::Expiration).big_unsigned())
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(AssetTransfer::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(AssetTransfer::Idx)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(AssetTransfer::UserDriven)
                            .boolean()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(AssetTransfer::BatchTransferIdx)
                            .big_integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(AssetTransfer::AssetRgb20Id).string())
                    .col(ColumnDef::new(AssetTransfer::AssetRgb25Id).string())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-assettransfer-batchtransfer")
                            .from(AssetTransfer::Table, AssetTransfer::BatchTransferIdx)
                            .to(BatchTransfer::Table, BatchTransfer::Idx)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-assettransfer-assetrgb20")
                            .from(AssetTransfer::Table, AssetTransfer::AssetRgb20Id)
                            .to(AssetRgb20::Table, AssetRgb20::AssetId)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-assettransfer-assetrgb25")
                            .from(AssetTransfer::Table, AssetTransfer::AssetRgb25Id)
                            .to(AssetRgb25::Table, AssetRgb25::AssetId)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Coloring::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Coloring::Idx)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Coloring::TxoIdx).big_integer().not_null())
                    .col(
                        ColumnDef::new(Coloring::AssetTransferIdx)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Coloring::ColoringType)
                            .tiny_unsigned()
                            .not_null(),
                    )
                    .col(ColumnDef::new(Coloring::Amount).string().not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-coloring-assettransfer")
                            .from(Coloring::Table, Coloring::AssetTransferIdx)
                            .to(AssetTransfer::Table, AssetTransfer::Idx)
                            .on_delete(ForeignKeyAction::Restrict)
                            .on_update(ForeignKeyAction::Restrict),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-coloring-txo")
                            .from(Coloring::Table, Coloring::TxoIdx)
                            .to(Txo::Table, Txo::Idx)
                            .on_delete(ForeignKeyAction::Restrict)
                            .on_update(ForeignKeyAction::Restrict),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Transfer::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Transfer::Idx)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(Transfer::AssetTransferIdx)
                            .big_integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(Transfer::Amount).string().not_null())
                    .col(ColumnDef::new(Transfer::BlindedUtxo).string())
                    .col(ColumnDef::new(Transfer::BlindingSecret).string())
                    .col(ColumnDef::new(Transfer::Ack).boolean())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-transfer-assettransfer")
                            .from(Transfer::Table, Transfer::AssetTransferIdx)
                            .to(AssetTransfer::Table, AssetTransfer::Idx)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(TransportEndpoint::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(TransportEndpoint::Idx)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(TransportEndpoint::TransportType)
                            .tiny_unsigned()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TransportEndpoint::Endpoint)
                            .string()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                sea_query::Index::create()
                    .name("idx-ce-transport-type-endpoint")
                    .table(TransportEndpoint::Table)
                    .col(TransportEndpoint::TransportType)
                    .col(TransportEndpoint::Endpoint)
                    .unique()
                    .clone(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(TransferTransportEndpoint::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(TransferTransportEndpoint::Idx)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(TransferTransportEndpoint::TransferIdx)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TransferTransportEndpoint::TransportEndpointIdx)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TransferTransportEndpoint::Used)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-transferTransportEndpoint-transfer")
                            .from(
                                TransferTransportEndpoint::Table,
                                TransferTransportEndpoint::TransferIdx,
                            )
                            .to(Transfer::Table, Transfer::Idx)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-transferTransportEndpoint-TransportEndpoint")
                            .from(
                                TransferTransportEndpoint::Table,
                                TransferTransportEndpoint::TransportEndpointIdx,
                            )
                            .to(TransportEndpoint::Table, TransportEndpoint::Idx)
                            .on_delete(ForeignKeyAction::Restrict)
                            .on_update(ForeignKeyAction::Restrict),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                sea_query::Index::create()
                    .name("idx-tce-transfer_idx-transport_endpoint_idx")
                    .table(TransferTransportEndpoint::Table)
                    .col(TransferTransportEndpoint::TransferIdx)
                    .col(TransferTransportEndpoint::TransportEndpointIdx)
                    .unique()
                    .clone(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(WalletTransaction::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(WalletTransaction::Idx)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(WalletTransaction::Txid).string().not_null())
                    .col(
                        ColumnDef::new(WalletTransaction::WalletTransactionType)
                            .tiny_unsigned()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                sea_query::Index::create()
                    .name("idx-coloring-asset_transfer_idx-txo_idx")
                    .table(Coloring::Table)
                    .col(Coloring::AssetTransferIdx)
                    .col(Coloring::TxoIdx)
                    .unique()
                    .clone(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Txo::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(AssetRgb20::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(AssetRgb25::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(BatchTransfer::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(AssetTransfer::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(Coloring::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(Transfer::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(TransportEndpoint::Table).to_owned())
            .await?;

        manager
            .drop_table(
                Table::drop()
                    .table(TransferTransportEndpoint::Table)
                    .to_owned(),
            )
            .await?;

        manager
            .drop_table(Table::drop().table(WalletTransaction::Table).to_owned())
            .await
    }
}

/// Learn more at https://docs.rs/sea-query#iden
#[derive(Iden)]
pub enum Txo {
    Table,
    Idx,
    Txid,
    Vout,
    BtcAmount,
    Colorable,
    Spent,
}

/// Learn more at https://docs.rs/sea-query#iden
#[derive(Iden)]
pub enum AssetRgb20 {
    Table,
    Idx,
    AssetId,
    Ticker,
    Name,
    Precision,
}

/// Learn more at https://docs.rs/sea-query#iden
#[derive(Iden)]
pub enum AssetRgb25 {
    Table,
    Idx,
    AssetId,
    Name,
    Precision,
    Description,
}

/// Learn more at https://docs.rs/sea-query#iden
#[derive(Iden)]
pub enum BatchTransfer {
    Table,
    Idx,
    Txid,
    Status,
    CreatedAt,
    UpdatedAt,
    Expiration,
}

/// Learn more at https://docs.rs/sea-query#iden
#[derive(Iden)]
pub enum AssetTransfer {
    Table,
    Idx,
    UserDriven,
    BatchTransferIdx,
    AssetRgb20Id,
    AssetRgb25Id,
}

/// Learn more at https://docs.rs/sea-query#iden
#[derive(Iden)]
pub enum Coloring {
    Table,
    Idx,
    TxoIdx,
    AssetTransferIdx,
    ColoringType,
    Amount,
}

/// Learn more at https://docs.rs/sea-query#iden
#[derive(Iden)]
pub enum Transfer {
    Table,
    Idx,
    AssetTransferIdx,
    Amount,
    BlindedUtxo,
    BlindingSecret,
    Ack,
}

/// Learn more at https://docs.rs/sea-query#iden
#[derive(Iden)]
pub enum TransportEndpoint {
    Table,
    Idx,
    TransportType,
    Endpoint,
}

/// Learn more at https://docs.rs/sea-query#iden
#[derive(Iden)]
pub enum TransferTransportEndpoint {
    Table,
    Idx,
    TransferIdx,
    TransportEndpointIdx,
    Used,
}

/// Learn more at https://docs.rs/sea-query#iden
#[derive(Iden)]
enum WalletTransaction {
    Table,
    Idx,
    Txid,
    WalletTransactionType,
}
