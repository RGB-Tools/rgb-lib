//! Replace the `reject_list_url` column of the `asset` table, which could only hold the URL of a
//! reject list, with a `reject_list` JSON column holding the whole location the genesis of the
//! asset commits to (a URL or an EVM contract, see `RgbRejectListInfo`).
//!
//! Existing URLs are converted to the JSON representation of the URL variant.

use sea_orm_migration::{
    prelude::*,
    sea_orm::{ConnectionTrait, Statement},
};

#[derive(DeriveMigrationName)]
pub struct Migration;

/// The JSON representation of a reject list served at `url`, as `RgbRejectListInfo::Url`.
fn url_to_json(url: &str) -> String {
    // a URL has no control characters, so escaping the JSON string delimiters is enough
    let escaped = url.replace('\\', "\\\\").replace('"', "\\\"");
    format!(r#"{{"Url":"{escaped}"}}"#)
}

/// The URL of a reject list stored as JSON, `None` if the location isn't a URL.
fn json_to_url(json: &str) -> Option<String> {
    let escaped = json
        .trim()
        .strip_prefix(r#"{"Url":""#)?
        .strip_suffix(r#""}"#)?;
    Some(escaped.replace("\\\"", "\"").replace("\\\\", "\\"))
}

/// The non-null values of the `column` column of the `asset` table, by row index.
async fn asset_values(
    manager: &SchemaManager<'_>,
    column: &str,
) -> Result<Vec<(i32, String)>, DbErr> {
    let rows = manager
        .get_connection()
        .query_all_raw(Statement::from_string(
            manager.get_database_backend(),
            format!("SELECT idx, {column} FROM asset WHERE {column} IS NOT NULL"),
        ))
        .await?;
    rows.iter()
        .map(|row| Ok((row.try_get("", "idx")?, row.try_get("", column)?)))
        .collect()
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Asset::Table)
                    .add_column(ColumnDef::new(Asset::RejectList).json().null())
                    .to_owned(),
            )
            .await?;

        for (idx, url) in asset_values(manager, "reject_list_url").await? {
            manager
                .exec_stmt(
                    Query::update()
                        .table(Asset::Table)
                        .value(Asset::RejectList, url_to_json(&url))
                        .and_where(Expr::col(Asset::Idx).eq(idx))
                        .to_owned(),
                )
                .await?;
        }

        manager
            .alter_table(
                Table::alter()
                    .table(Asset::Table)
                    .drop_column(Asset::RejectListUrl)
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
                    .add_column(ColumnDef::new(Asset::RejectListUrl).string().null())
                    .to_owned(),
            )
            .await?;

        // reject lists kept by an EVM contract cannot be represented, so they are dropped
        for (idx, json) in asset_values(manager, "reject_list").await? {
            if let Some(url) = json_to_url(&json) {
                manager
                    .exec_stmt(
                        Query::update()
                            .table(Asset::Table)
                            .value(Asset::RejectListUrl, url)
                            .and_where(Expr::col(Asset::Idx).eq(idx))
                            .to_owned(),
                    )
                    .await?;
            }
        }

        manager
            .alter_table(
                Table::alter()
                    .table(Asset::Table)
                    .drop_column(Asset::RejectList)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
pub enum Asset {
    Table,
    Idx,
    RejectList,
    RejectListUrl,
}
