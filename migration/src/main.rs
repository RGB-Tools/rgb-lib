use sea_orm_migration::prelude::*;

#[tokio::main]
async fn main() {
    cli::run_cli(rgb_lib_migration::Migrator).await;
}

#[cfg(test)]
mod tests {
    use std::path::{MAIN_SEPARATOR_STR, PathBuf};

    use rgb_lib_migration::Migrator;
    use sea_orm_migration::sea_orm::{ConnectOptions, Database};

    use super::*;

    const TEST_DATA_DIR_PARTS: [&str; 3] = ["tests", "tmp", "test_db"];

    #[tokio::test]
    async fn test_migrations() {
        let db_path = PathBuf::from(TEST_DATA_DIR_PARTS.join(MAIN_SEPARATOR_STR));
        std::fs::create_dir_all(db_path.parent().unwrap()).unwrap();
        if db_path.exists() {
            std::fs::remove_file(&db_path).unwrap();
        }
        let connection_string = format!("sqlite://{}?mode=rwc", db_path.display());
        let connect_options = ConnectOptions::new(connection_string);
        let db = Database::connect(connect_options).await.unwrap();

        Migrator::up(&db, None).await.unwrap();
        Migrator::refresh(&db).await.unwrap();
    }
}
