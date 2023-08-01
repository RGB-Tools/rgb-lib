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
                            .integer()
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
                    .table(Asset::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Asset::Idx)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(Asset::AssetId)
                            .string()
                            .not_null()
                            .unique_key(),
                    )
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
                            .integer()
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
                            .integer()
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
                            .integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(AssetTransfer::AssetId).string())
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
                            .name("fk-assettransfer-asset")
                            .from(AssetTransfer::Table, AssetTransfer::AssetId)
                            .to(Asset::Table, Asset::AssetId)
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
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Coloring::TxoIdx).integer().not_null())
                    .col(
                        ColumnDef::new(Coloring::AssetTransferIdx)
                            .integer()
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
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(Transfer::AssetTransferIdx)
                            .integer()
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
                            .integer()
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
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(TransferTransportEndpoint::TransferIdx)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TransferTransportEndpoint::TransportEndpointIdx)
                            .integer()
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
                            .name("fk-transfertransportendpoint-transfer")
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
                            .name("fk-transfertransportendpoint-transportendpoint")
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
                            .integer()
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
            .drop_table(Table::drop().table(Asset::Table).to_owned())
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

#[derive(DeriveIden)]
pub enum Txo {
    Table,
    Idx,
    Txid,
    Vout,
    BtcAmount,
    Colorable,
    Spent,
}

#[derive(DeriveIden)]
pub enum Asset {
    Table,
    Idx,
    AssetId,
}

#[derive(DeriveIden)]
pub enum BatchTransfer {
    Table,
    Idx,
    Txid,
    Status,
    CreatedAt,
    UpdatedAt,
    Expiration,
}

#[derive(DeriveIden)]
pub enum AssetTransfer {
    Table,
    Idx,
    UserDriven,
    BatchTransferIdx,
    AssetId,
}

#[derive(DeriveIden)]
pub enum Coloring {
    Table,
    Idx,
    TxoIdx,
    AssetTransferIdx,
    ColoringType,
    Amount,
}

#[derive(DeriveIden)]
pub enum Transfer {
    Table,
    Idx,
    AssetTransferIdx,
    Amount,
    BlindedUtxo,
    BlindingSecret,
    Ack,
}

#[derive(DeriveIden)]
pub enum TransportEndpoint {
    Table,
    Idx,
    TransportType,
    Endpoint,
}

#[derive(DeriveIden)]
pub enum TransferTransportEndpoint {
    Table,
    Idx,
    TransferIdx,
    TransportEndpointIdx,
    Used,
}

#[derive(DeriveIden)]
enum WalletTransaction {
    Table,
    Idx,
    Txid,
    WalletTransactionType,
}
