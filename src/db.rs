use anyhow::{Context, Result};
use directories::ProjectDirs;
use sqlx::{SqlitePool, sqlite::SqliteConnectOptions};
use std::{fs, str::FromStr};

pub struct Db {
    pub pool: SqlitePool,
}

impl Db {
    pub async fn open() -> Result<Self> {
        let dirs = ProjectDirs::from("", "", "chronograph")
            .context("cannot determine app data directory")?;
        let data_dir = dirs.data_local_dir();
        fs::create_dir_all(data_dir)
            .with_context(|| format!("cannot create data dir: {}", data_dir.display()))?;

        let db_path = data_dir.join("chronograph.db");
        let db_url = format!("sqlite://{}?mode=rwc", db_path.display());

        let opts = SqliteConnectOptions::from_str(&db_url)?
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .pragma("synchronous", "NORMAL")
            .pragma("foreign_keys", "ON");

        let pool = SqlitePool::connect_with(opts).await
            .with_context(|| format!("cannot open SQLite: {}", db_path.display()))?;

        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .context("migration failed")?;

        Ok(Self { pool })
    }
}
