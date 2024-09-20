use sqlx::sqlite::{Sqlite, SqliteConnectOptions, SqliteJournalMode, SqliteRow, SqliteSynchronous};
use sqlx::{Acquire, Row, SqlitePool};
use std::borrow::Cow;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

pub async fn setup_db<P: AsRef<str>>(database_url: P) -> Result<SqlitePool, sqlx::Error> {
    let conn_options = SqliteConnectOptions::from_str(database_url.as_ref())?
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .pragma("mmap_size", "134217728")
        .pragma("journal_size_limit", "27103364")
        .pragma("cache_size", "2000")
        .create_if_missing(true);

    let pool = SqlitePool::connect_with(conn_options).await?;

    sqlx::migrate!("./migrations").run(&pool).await?;

    Ok(pool)
}

#[derive(Debug, sqlx::FromRow)]
pub struct CommandRow {
    pub id: i64,
    pub raw: String,
    pub command_hash: String,
    pub output: String,
}

pub async fn get_command_by_hash<'a, A>(
    conn: A,
    command_hash: &str,
) -> Result<Option<CommandRow>, sqlx::Error>
where
    A: Acquire<'a, Database = Sqlite>,
{
    let mut conn = conn.acquire().await?;

    let record = sqlx::query_as!(
        CommandRow,
        r#"
            SELECT *
            FROM nix_command
            WHERE command_hash = ?
        "#,
        command_hash
    )
    .fetch_optional(&mut *conn)
    .await?;

    Ok(record)
}

pub async fn insert_command_with_files<'a, A>(
    conn: A,
    raw_cmd: &str,
    cmd_hash: &str,
    output: &[u8],
    paths: &[(Cow<'_, Path>, String)],
) -> Result<(i64, Vec<i64>), sqlx::Error>
where
    A: Acquire<'a, Database = Sqlite>,
{
    let mut conn = conn.acquire().await?;
    let mut tx = conn.begin().await?;

    let command_id = insert_command(&mut tx, raw_cmd, cmd_hash, output).await?;
    let file_ids = insert_files(&mut tx, paths, command_id).await?;

    tx.commit().await?;

    Ok((command_id, file_ids))
}

async fn insert_command<'a, A>(
    conn: A,
    raw_cmd: &str,
    cmd_hash: &str,
    output: &[u8],
) -> Result<i64, sqlx::Error>
where
    A: Acquire<'a, Database = Sqlite>,
{
    let mut conn = conn.acquire().await?;

    let record = sqlx::query!(
        r#"
        INSERT INTO nix_command (raw, command_hash, output)
        VALUES (?, ?, ?)
        ON CONFLICT (command_hash) DO UPDATE
          SET id = id
        RETURNING id
      "#,
        raw_cmd,
        cmd_hash,
        output
    )
    .fetch_one(&mut *conn)
    .await?;

    Ok(record.id)
}

async fn insert_files<'a, A>(
    conn: A,
    paths: &[(Cow<'_, Path>, String)],
    command_id: i64,
) -> Result<Vec<i64>, sqlx::Error>
where
    A: Acquire<'a, Database = Sqlite>,
{
    let mut conn = conn.acquire().await?;

    let file_query = r#"
        INSERT INTO file (path, content_hash)
        VALUES (?, ?)
        ON CONFLICT (path) DO UPDATE SET content_hash = excluded.content_hash
        RETURNING id
    "#;

    let mut file_ids = Vec::with_capacity(paths.len());
    for (path, hash) in paths {
        let id: i64 = sqlx::query(file_query)
            .bind(path.to_path_buf().into_os_string().as_bytes())
            .bind(hash)
            .fetch_one(&mut *conn)
            .await?
            .get(0);
        file_ids.push(id);
    }

    let input_file_query = r#"
        INSERT INTO input_file (nix_command_id, file_id)
        VALUES (?, ?)
        ON CONFLICT (nix_command_id, file_id) DO NOTHING
    "#;

    for &file_id in &file_ids {
        sqlx::query(input_file_query)
            .bind(command_id)
            .bind(file_id)
            .execute(&mut *conn)
            .await?;
    }
    Ok(file_ids)
}

#[derive(Debug)]
pub struct FileRow {
    pub path: PathBuf,
    pub content_hash: String,
    pub updated_at: SystemTime,
}

impl sqlx::FromRow<'_, SqliteRow> for FileRow {
    fn from_row(row: &SqliteRow) -> Result<Self, sqlx::Error> {
        let path: &[u8] = row.get("path");
        let content_hash: String = row.get("content_hash");
        let updated_at: u64 = row.get("updated_at");
        Ok(Self {
            path: PathBuf::from(OsStr::from_bytes(path)),
            content_hash: content_hash,
            updated_at: UNIX_EPOCH + std::time::Duration::from_secs(updated_at),
        })
    }
}

pub async fn get_files_by_command_id(
    pool: &SqlitePool,
    command_id: i64,
) -> Result<Vec<FileRow>, sqlx::Error> {
    let files = sqlx::query_as(
        r#"
            SELECT f.path, f.content_hash, f.updated_at
            FROM file f
            JOIN input_file if ON f.id = if.file_id
            WHERE if.nix_command_id = ?
        "#,
    )
    .bind(command_id)
    .fetch_all(pool)
    .await?;

    Ok(files)
}

pub async fn get_files_by_command_hash(
    pool: &SqlitePool,
    command_hash: &str,
) -> Result<Vec<FileRow>, sqlx::Error> {
    let files = sqlx::query_as(
        r#"
            SELECT f.path, f.content_hash, f.updated_at
            FROM file f
            JOIN input_file if ON f.id = if.file_id
            JOIN nix_command nc ON if.nix_command_id = nc.id
            WHERE nc.command_hash = ?
        "#,
    )
    .bind(command_hash)
    .fetch_all(pool)
    .await?;

    Ok(files)
}
