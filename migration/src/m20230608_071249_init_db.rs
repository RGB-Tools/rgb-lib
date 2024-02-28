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
                    .col(ColumnDef::new(Txo::Vout).big_unsigned().not_null())
                    .col(ColumnDef::new(Txo::BtcAmount).string().not_null())
                    .col(ColumnDef::new(Txo::Spent).boolean().not_null())
                    .col(ColumnDef::new(Txo::Exists).boolean().not_null())
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
                    .table(Media::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Media::Idx)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(Media::Digest)
                            .string()
                            .not_null()
                            .unique_key(),
                    )
                    .col(ColumnDef::new(Media::Mime).string().not_null())
                    .to_owned(),
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
                    .col(ColumnDef::new(Asset::MediaIdx).integer())
                    .col(ColumnDef::new(Asset::Id).string().not_null().unique_key())
                    .col(ColumnDef::new(Asset::Schema).tiny_unsigned().not_null())
                    .col(ColumnDef::new(Asset::AddedAt).big_unsigned().not_null())
                    .col(ColumnDef::new(Asset::Details).string())
                    .col(ColumnDef::new(Asset::IssuedSupply).string().not_null())
                    .col(ColumnDef::new(Asset::Name).string().not_null())
                    .col(ColumnDef::new(Asset::Precision).tiny_unsigned().not_null())
                    .col(ColumnDef::new(Asset::Ticker).string())
                    .col(ColumnDef::new(Asset::Timestamp).big_unsigned().not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-asset-media")
                            .from(Asset::Table, Asset::MediaIdx)
                            .to(Media::Table, Media::Idx)
                            .on_delete(ForeignKeyAction::Restrict)
                            .on_update(ForeignKeyAction::Cascade),
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
                            .tiny_unsigned()
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
                    .col(
                        ColumnDef::new(BatchTransfer::MinConfirmations)
                            .tiny_unsigned()
                            .not_null(),
                    )
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
                            .to(Asset::Table, Asset::Id)
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
                    .col(ColumnDef::new(Coloring::Type).tiny_unsigned().not_null())
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
                    .col(ColumnDef::new(Transfer::Incoming).boolean().not_null())
                    .col(ColumnDef::new(Transfer::RecipientType).tiny_unsigned())
                    .col(ColumnDef::new(Transfer::RecipientID).string())
                    .col(ColumnDef::new(Transfer::Ack).boolean())
                    .col(ColumnDef::new(Transfer::Vout).big_unsigned())
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
                    .name("idx-transportendpoint-transporttype-endpoint")
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
                    .name("idx-tte-transferidx-transportendpointidx")
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
                    .table(Token::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Token::Idx)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Token::AssetIdx).integer().not_null())
                    .col(ColumnDef::new(Token::Index).big_unsigned().not_null())
                    .col(ColumnDef::new(Token::Ticker).string())
                    .col(ColumnDef::new(Token::Name).string())
                    .col(ColumnDef::new(Token::Details).string())
                    .col(ColumnDef::new(Token::EmbeddedMedia).boolean().not_null())
                    .col(ColumnDef::new(Token::Reserves).boolean().not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-token-asset")
                            .from(Token::Table, Token::AssetIdx)
                            .to(Asset::Table, Asset::Idx)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(TokenMedia::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(TokenMedia::Idx)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(TokenMedia::TokenIdx).integer().not_null())
                    .col(ColumnDef::new(TokenMedia::MediaIdx).integer().not_null())
                    .col(ColumnDef::new(TokenMedia::AttachmentId).tiny_unsigned())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-tokenmedia-token")
                            .from(TokenMedia::Table, TokenMedia::TokenIdx)
                            .to(Token::Table, Token::Idx)
                            .on_delete(ForeignKeyAction::Cascade)
                            .on_update(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-tokenmedia-media")
                            .from(TokenMedia::Table, TokenMedia::MediaIdx)
                            .to(Media::Table, Media::Idx)
                            .on_delete(ForeignKeyAction::Restrict)
                            .on_update(ForeignKeyAction::Restrict),
                    )
                    .to_owned(),
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
                        ColumnDef::new(WalletTransaction::Type)
                            .tiny_unsigned()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(PendingWitnessScript::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(PendingWitnessScript::Idx)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(PendingWitnessScript::Script)
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
                    .table(PendingWitnessOutpoint::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(PendingWitnessOutpoint::Idx)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(PendingWitnessOutpoint::Txid)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(PendingWitnessOutpoint::Vout)
                            .big_unsigned()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                sea_query::Index::create()
                    .name("idx-pendingwitnessoutpoint-txid-vout")
                    .table(PendingWitnessOutpoint::Table)
                    .col(PendingWitnessOutpoint::Txid)
                    .col(PendingWitnessOutpoint::Vout)
                    .unique()
                    .clone(),
            )
            .await?;

        manager
            .create_index(
                sea_query::Index::create()
                    .name("idx-coloring-assettransferidx-txoidx")
                    .table(Coloring::Table)
                    .col(Coloring::AssetTransferIdx)
                    .col(Coloring::TxoIdx)
                    .unique()
                    .clone(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(BackupInfo::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(BackupInfo::Idx)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(BackupInfo::LastBackupTimestamp)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(BackupInfo::LastOperationTimestamp)
                            .string()
                            .not_null(),
                    )
                    .to_owned(),
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
            .drop_table(Table::drop().table(Token::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(Media::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(TokenMedia::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(WalletTransaction::Table).to_owned())
            .await?;

        manager
            .drop_table(
                Table::drop()
                    .table(PendingWitnessOutpoint::Table)
                    .to_owned(),
            )
            .await?;

        manager
            .drop_table(Table::drop().table(PendingWitnessScript::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(BackupInfo::Table).to_owned())
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
    Spent,
    Exists,
}

#[derive(DeriveIden)]
pub enum Asset {
    Table,
    Idx,
    MediaIdx,
    Id,
    Schema,
    AddedAt,
    Details,
    IssuedSupply,
    Name,
    Precision,
    Ticker,
    Timestamp,
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
    MinConfirmations,
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
    Type,
    Amount,
}

#[derive(DeriveIden)]
pub enum Transfer {
    Table,
    Idx,
    AssetTransferIdx,
    Amount,
    Incoming,
    RecipientType,
    RecipientID,
    Ack,
    Vout,
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
enum Token {
    Table,
    Idx,
    AssetIdx,
    Index,
    Ticker,
    Name,
    Details,
    EmbeddedMedia,
    Reserves,
}

#[derive(DeriveIden)]
enum Media {
    Table,
    Idx,
    Digest,
    Mime,
}

#[derive(DeriveIden)]
enum TokenMedia {
    Table,
    Idx,
    TokenIdx,
    MediaIdx,
    AttachmentId,
}

#[derive(DeriveIden)]
enum WalletTransaction {
    Table,
    Idx,
    Txid,
    Type,
}

#[derive(DeriveIden)]
enum PendingWitnessScript {
    Table,
    Idx,
    Script,
}

#[derive(DeriveIden)]
enum PendingWitnessOutpoint {
    Table,
    Idx,
    Txid,
    Vout,
}

#[derive(DeriveIden)]
enum BackupInfo {
    Table,
    Idx,
    LastBackupTimestamp,
    LastOperationTimestamp,
}
