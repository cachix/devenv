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
    pub cmd_hash: String,
    pub output: String,
}
pub async fn get_command_by_hash<'a, A>(
    conn: A,
    cmd_hash: &str,
) -> Result<Option<CommandRow>, sqlx::Error>
where
    A: Acquire<'a, Database = Sqlite>,
{
    let mut conn = conn.acquire().await?;

    let record = sqlx::query_as!(
        CommandRow,
        r#"
            SELECT *
            FROM cached_cmd
            WHERE cmd_hash = ?
        "#,
        cmd_hash
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

    delete_command(&mut tx, cmd_hash).await?;
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
        INSERT INTO cached_cmd (raw, cmd_hash, output)
        VALUES (?, ?, ?)
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
async fn delete_command<'a, A>(conn: A, cmd_hash: &str) -> Result<(), sqlx::Error>
where
    A: Acquire<'a, Database = Sqlite>,
{
    let mut conn = conn.acquire().await?;

    sqlx::query!(
        r#"
        DELETE FROM cached_cmd
        WHERE cmd_hash = ?
        "#,
        cmd_hash
    )
    .execute(&mut *conn)
    .await?;

    Ok(())
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

    let file_path_query = r#"
        INSERT INTO file_path (path, content_hash)
        VALUES (?, ?)
        ON CONFLICT (path) DO UPDATE SET content_hash = excluded.content_hash
        RETURNING id
    "#;

    let mut file_ids = Vec::with_capacity(paths.len());
    for (path, hash) in paths {
        let id: i64 = sqlx::query(file_path_query)
            .bind(path.to_path_buf().into_os_string().as_bytes())
            .bind(hash)
            .fetch_one(&mut *conn)
            .await?
            .get(0);
        file_ids.push(id);
    }

    let cmd_input_path_query = r#"
        INSERT INTO cmd_input_path (cached_cmd_id, file_path_id)
        VALUES (?, ?)
        ON CONFLICT (cached_cmd_id, file_path_id) DO NOTHING
    "#;

    for &file_id in &file_ids {
        sqlx::query(cmd_input_path_query)
            .bind(command_id)
            .bind(file_id)
            .execute(&mut *conn)
            .await?;
    }
    Ok(file_ids)
}

#[derive(Debug)]
pub struct FilePathRow {
    pub path: PathBuf,
    pub content_hash: String,
    pub updated_at: SystemTime,
}

impl sqlx::FromRow<'_, SqliteRow> for FilePathRow {
    fn from_row(row: &SqliteRow) -> Result<Self, sqlx::Error> {
        let path: &[u8] = row.get("path");
        let content_hash: String = row.get("content_hash");
        let updated_at: u64 = row.get("updated_at");
        Ok(Self {
            path: PathBuf::from(OsStr::from_bytes(path)),
            content_hash,
            updated_at: UNIX_EPOCH + std::time::Duration::from_secs(updated_at),
        })
    }
}
pub async fn get_files_by_command_id(
    pool: &SqlitePool,
    command_id: i64,
) -> Result<Vec<FilePathRow>, sqlx::Error> {
    let files = sqlx::query_as(
        r#"
            SELECT fp.path, fp.content_hash, fp.updated_at
            FROM file_path fp
            JOIN cmd_input_path cip ON fp.id = cip.file_path_id
            WHERE cip.cached_cmd_id = ?
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
) -> Result<Vec<FilePathRow>, sqlx::Error> {
    let files = sqlx::query_as(
        r#"
            SELECT fp.path, fp.content_hash, fp.updated_at
            FROM file_path fp
            JOIN cmd_input_path cip ON fp.id = cip.file_path_id
            JOIN cached_cmd cc ON cip.cached_cmd_id = cc.id
            WHERE cc.cmd_hash = ?
        "#,
    )
    .bind(command_hash)
    .fetch_all(pool)
    .await?;

    Ok(files)
}
pub async fn delete_unreferenced_files(pool: &SqlitePool) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        r#"
        DELETE FROM file_path
        WHERE NOT EXISTS (
            SELECT 1
            FROM cmd_input_path
            WHERE cmd_input_path.file_path_id = file_path.id
        )
        "#,
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::SqlitePool;

    #[sqlx::test]
    async fn test_insert_and_retrieve_command(pool: SqlitePool) {
        let raw_cmd = "nix-build -A hello";
        let cmd_hash = "hash123";
        let output = b"Hello, world!";
        let paths = vec![
            (
                Cow::Borrowed(Path::new("/path/to/file1")),
                "hash1".to_string(),
            ),
            (
                Cow::Borrowed(Path::new("/path/to/file2")),
                "hash2".to_string(),
            ),
        ];

        let (command_id, file_ids) =
            insert_command_with_files(&pool, raw_cmd, cmd_hash, output, &paths)
                .await
                .unwrap();

        assert_eq!(file_ids.len(), 2);

        let retrieved_command = get_command_by_hash(&pool, cmd_hash).await.unwrap().unwrap();
        assert_eq!(retrieved_command.raw, raw_cmd);
        assert_eq!(retrieved_command.cmd_hash, cmd_hash);
        // TODO: fix conversion
        assert_eq!(retrieved_command.output, String::from_utf8_lossy(output));

        let files = get_files_by_command_id(&pool, command_id).await.unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, Path::new("/path/to/file1"));
        assert_eq!(files[0].content_hash, "hash1");
        assert_eq!(files[1].path, Path::new("/path/to/file2"));
        assert_eq!(files[1].content_hash, "hash2");
    }

    #[sqlx::test]
    async fn test_insert_multiple_commands(pool: SqlitePool) {
        // First command
        let raw_cmd1 = "nix-build -A hello";
        let cmd_hash1 = "hash123";
        let output1 = b"Hello, world!";
        let paths1 = vec![
            (
                Cow::Borrowed(Path::new("/path/to/file1")),
                "hash1".to_string(),
            ),
            (
                Cow::Borrowed(Path::new("/path/to/file2")),
                "hash2".to_string(),
            ),
        ];

        let (command_id1, file_ids1) =
            insert_command_with_files(&pool, raw_cmd1, cmd_hash1, output1, &paths1)
                .await
                .unwrap();

        // Second command
        let raw_cmd2 = "nix-build -A goodbye";
        let cmd_hash2 = "hash456";
        let output2 = b"Goodbye, world!";
        let paths2 = vec![
            (
                Cow::Borrowed(Path::new("/path/to/file2")),
                "hash2".to_string(),
            ),
            (
                Cow::Borrowed(Path::new("/path/to/file3")),
                "hash3".to_string(),
            ),
        ];

        let (command_id2, file_ids2) =
            insert_command_with_files(&pool, raw_cmd2, cmd_hash2, output2, &paths2)
                .await
                .unwrap();

        // Verify first command
        let retrieved_command1 = get_command_by_hash(&pool, cmd_hash1)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(retrieved_command1.raw, raw_cmd1);
        let files1 = get_files_by_command_id(&pool, command_id1).await.unwrap();
        assert_eq!(files1.len(), 2);

        // Verify second command
        let retrieved_command2 = get_command_by_hash(&pool, cmd_hash2)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(retrieved_command2.raw, raw_cmd2);
        let files2 = get_files_by_command_id(&pool, command_id2).await.unwrap();
        assert_eq!(files2.len(), 2);

        // Verify cmd_input_path rows
        let all_files = sqlx::query("SELECT * FROM cmd_input_path")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(all_files.len(), 4); // 2 files for each command

        // Verify file reuse
        assert_eq!(file_ids1.len(), 2);
        assert_eq!(file_ids2.len(), 2);
        assert!(file_ids1.contains(&file_ids2[0])); // file2 is shared between commands
    }

    #[sqlx::test]
    async fn test_insert_command_with_modified_files(pool: SqlitePool) {
        // First command
        let raw_cmd = "nix-build -A hello";
        let cmd_hash = "hash123";
        let output = b"Hello, world!";
        let paths1 = vec![
            (
                Cow::Borrowed(Path::new("/path/to/file1")),
                "hash1".to_string(),
            ),
            (
                Cow::Borrowed(Path::new("/path/to/file2")),
                "hash2".to_string(),
            ),
        ];

        let (_command_id1, file_ids1) =
            insert_command_with_files(&pool, raw_cmd, cmd_hash, output, &paths1)
                .await
                .unwrap();

        // Second command
        let paths2 = vec![
            (
                Cow::Borrowed(Path::new("/path/to/file2")),
                "hash2".to_string(),
            ),
            (
                Cow::Borrowed(Path::new("/path/to/file3")),
                "hash3".to_string(),
            ),
        ];

        let (command_id2, file_ids2) =
            insert_command_with_files(&pool, raw_cmd, cmd_hash, output, &paths2)
                .await
                .unwrap();

        // Investigate the files associated with the new command
        let files = get_files_by_command_id(&pool, command_id2).await.unwrap();
        println!(
            "Number of files associated with the command: {}",
            files.len()
        );
        for file in &files {
            println!("File path: {:?}, hash: {}", file.path, file.content_hash);
        }

        // Check if files are being accumulated instead of replaced
        assert_eq!(files.len(), 2, "Expected 2 files, but found {}. Files might be accumulating instead of being replaced.", files.len());

        // Verify the correct files are associated
        let file_paths: Vec<_> = files.iter().map(|f| f.path.to_str().unwrap()).collect();
        assert!(
            file_paths.contains(&"/path/to/file2"),
            "Expected /path/to/file2 to be present"
        );
        assert!(
            file_paths.contains(&"/path/to/file3"),
            "Expected /path/to/file3 to be present"
        );
        assert!(
            !file_paths.contains(&"/path/to/file1"),
            "Expected /path/to/file1 to be absent"
        );

        // Verify that file2 is reused and file3 is new
        assert_eq!(file_ids2.len(), 2, "Expected 2 file IDs");
        assert!(
            file_ids1.contains(&file_ids2[0]),
            "Expected file2 to be reused"
        );
        assert!(
            !file_ids1.contains(&file_ids2[1]),
            "Expected file3 to be new"
        );

        // Verify that the new command has the correct files
        let files = get_files_by_command_id(&pool, command_id2).await.unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, Path::new("/path/to/file2"));
        assert_eq!(files[0].content_hash, "hash2");
        assert_eq!(files[1].path, Path::new("/path/to/file3"));
        assert_eq!(files[1].content_hash, "hash3");

        // Verify that file2 is reused and file3 is new
        assert_eq!(file_ids2.len(), 2);
        assert!(file_ids1.contains(&file_ids2[0])); // file2 is reused
        assert!(!file_ids1.contains(&file_ids2[1])); // file3 is new
    }
}
