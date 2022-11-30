use super::m20220810_132253_create_coloring::Coloring;
use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::{ConnectionTrait, DbBackend, Statement};

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20221130_152708_delete_zero_allocations"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let query = Query::delete()
            .from_table(Coloring::Table)
            .cond_where(all![
                Expr::col(Coloring::ColoringType).eq(4),
                Expr::col(Coloring::Amount).eq("0"),
            ])
            .to_owned();
        let database_backend = manager.get_database_backend();
        let stmt_string = match database_backend {
            DbBackend::MySql => query.to_string(MysqlQueryBuilder),
            DbBackend::Postgres => query.to_string(PostgresQueryBuilder),
            DbBackend::Sqlite => query.to_string(SqliteQueryBuilder),
        };
        let stmt = Statement::from_string(database_backend, stmt_string);
        manager.get_connection().execute(stmt).await.map(|_| ())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        Ok(())
    }
}
