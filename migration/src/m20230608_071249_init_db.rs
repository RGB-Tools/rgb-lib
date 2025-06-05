use sea_orm_migration::{prelude::*, schema::*};

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
                    .col(pk_auto(Txo::Idx))
                    .col(string(Txo::Txid))
                    .col(big_unsigned(Txo::Vout))
                    .col(string(Txo::BtcAmount))
                    .col(boolean(Txo::Spent))
                    .col(boolean(Txo::Exists))
                    .col(boolean(Txo::PendingWitness))
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
                    .col(pk_auto(Media::Idx))
                    .col(string_uniq(Media::Digest))
                    .col(string(Media::Mime))
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Asset::Table)
                    .if_not_exists()
                    .col(pk_auto(Asset::Idx))
                    .col(integer_null(Asset::MediaIdx))
                    .col(string_uniq(Asset::Id))
                    .col(tiny_unsigned(Asset::Schema))
                    .col(big_unsigned(Asset::AddedAt))
                    .col(string_null(Asset::Details))
                    .col(string(Asset::IssuedSupply))
                    .col(string(Asset::Name))
                    .col(tiny_unsigned(Asset::Precision))
                    .col(string_null(Asset::Ticker))
                    .col(big_unsigned(Asset::Timestamp))
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
                    .col(pk_auto(BatchTransfer::Idx))
                    .col(string_null(BatchTransfer::Txid))
                    .col(tiny_unsigned(BatchTransfer::Status))
                    .col(big_unsigned(BatchTransfer::CreatedAt))
                    .col(big_unsigned(BatchTransfer::UpdatedAt))
                    .col(big_unsigned_null(BatchTransfer::Expiration))
                    .col(tiny_unsigned(BatchTransfer::MinConfirmations))
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(AssetTransfer::Table)
                    .if_not_exists()
                    .col(pk_auto(AssetTransfer::Idx))
                    .col(boolean(AssetTransfer::UserDriven))
                    .col(integer(AssetTransfer::BatchTransferIdx))
                    .col(string_null(AssetTransfer::AssetId))
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
                    .col(pk_auto(Coloring::Idx))
                    .col(integer(Coloring::TxoIdx))
                    .col(integer(Coloring::AssetTransferIdx))
                    .col(tiny_unsigned(Coloring::Type))
                    .col(json(Coloring::Assignment))
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
                    .col(pk_auto(Transfer::Idx))
                    .col(integer(Transfer::AssetTransferIdx))
                    .col(json_null(Transfer::RequestedAssignment))
                    .col(boolean(Transfer::Incoming))
                    .col(json_null(Transfer::RecipientType))
                    .col(string_null(Transfer::RecipientID))
                    .col(boolean_null(Transfer::Ack))
                    .col(string_null(Transfer::InvoiceString))
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
                    .col(pk_auto(TransportEndpoint::Idx))
                    .col(tiny_unsigned(TransportEndpoint::TransportType))
                    .col(string(TransportEndpoint::Endpoint))
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
                    .col(pk_auto(TransferTransportEndpoint::Idx))
                    .col(integer(TransferTransportEndpoint::TransferIdx))
                    .col(integer(TransferTransportEndpoint::TransportEndpointIdx))
                    .col(boolean(TransferTransportEndpoint::Used).default(false))
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
                    .col(pk_auto(Token::Idx))
                    .col(integer(Token::AssetIdx))
                    .col(big_unsigned(Token::Index))
                    .col(string_null(Token::Ticker))
                    .col(string_null(Token::Name))
                    .col(string_null(Token::Details))
                    .col(boolean(Token::EmbeddedMedia))
                    .col(boolean(Token::Reserves))
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
                    .col(pk_auto(TokenMedia::Idx))
                    .col(integer(TokenMedia::TokenIdx))
                    .col(integer(TokenMedia::MediaIdx))
                    .col(tiny_unsigned_null(TokenMedia::AttachmentId))
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
                    .col(pk_auto(WalletTransaction::Idx))
                    .col(string(WalletTransaction::Txid))
                    .col(tiny_unsigned(WalletTransaction::Type))
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(PendingWitnessScript::Table)
                    .if_not_exists()
                    .col(pk_auto(PendingWitnessScript::Idx))
                    .col(string(PendingWitnessScript::Script).unique_key())
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                sea_query::Index::create()
                    .name("idx-coloring-assettransferidx-txoidx")
                    .table(Coloring::Table)
                    .col(Coloring::AssetTransferIdx)
                    .col(Coloring::TxoIdx)
                    .clone(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(BackupInfo::Table)
                    .if_not_exists()
                    .col(pk_auto(BackupInfo::Idx))
                    .col(string(BackupInfo::LastBackupTimestamp))
                    .col(string(BackupInfo::LastOperationTimestamp))
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Coloring::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(Txo::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(TokenMedia::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(Token::Table).to_owned())
            .await?;

        manager
            .drop_table(
                Table::drop()
                    .table(TransferTransportEndpoint::Table)
                    .to_owned(),
            )
            .await?;

        manager
            .drop_table(Table::drop().table(Transfer::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(AssetTransfer::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(BatchTransfer::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(Asset::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(Media::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(TransportEndpoint::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(WalletTransaction::Table).to_owned())
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
    PendingWitness,
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
    Assignment,
}

#[derive(DeriveIden)]
pub enum Transfer {
    Table,
    Idx,
    AssetTransferIdx,
    RequestedAssignment,
    Incoming,
    RecipientType,
    RecipientID,
    Ack,
    InvoiceString,
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
enum BackupInfo {
    Table,
    Idx,
    LastBackupTimestamp,
    LastOperationTimestamp,
}
