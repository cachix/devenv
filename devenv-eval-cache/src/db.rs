use super::command::{EnvInputDesc, FileInputDesc, Input};
use sqlx::sqlite::{Sqlite, SqliteConnectOptions, SqliteJournalMode, SqliteRow, SqliteSynchronous};
use sqlx::{migrate::MigrateDatabase, Acquire, Row, SqlitePool};
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::error;

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

    if let Err(err) = sqlx::migrate!().run(&pool).await {
        error!(error = %err, "Failed to migrate the Nix evaluation cache database. Attempting to recreate the database.");

        // Delete the database and rerun the migrations
        Sqlite::drop_database(database_url.as_ref()).await?;
        Sqlite::create_database(database_url.as_ref()).await?;
        sqlx::migrate!().run(&pool).await?;
    }

    Ok(pool)
}

/// The row type for the `cached_cmd` table.
#[derive(Clone, Debug)]
pub struct CommandRow {
    /// The primary key
    pub id: i64,
    /// The raw command string (for debugging)
    pub raw: String,
    /// A hash of the command string
    pub cmd_hash: String,
    /// A hash of the content hashes of the input files
    pub input_hash: String,
    /// The raw output of the command
    pub output: Vec<u8>,
    /// The time the cached command was checked or created
    pub updated_at: SystemTime,
}

impl sqlx::FromRow<'_, SqliteRow> for CommandRow {
    fn from_row(row: &SqliteRow) -> Result<Self, sqlx::Error> {
        let id: i64 = row.get("id");
        let raw: String = row.get("raw");
        let cmd_hash: String = row.get("cmd_hash");
        let input_hash: String = row.get("input_hash");
        let output: Vec<u8> = row.get("output");
        let updated_at: u64 = row.get("updated_at");
        Ok(Self {
            id,
            raw,
            cmd_hash,
            input_hash,
            output,
            updated_at: UNIX_EPOCH + std::time::Duration::from_secs(updated_at),
        })
    }
}

pub async fn get_command_by_hash<'a, A>(
    conn: A,
    cmd_hash: &str,
) -> Result<Option<CommandRow>, sqlx::Error>
where
    A: Acquire<'a, Database = Sqlite>,
{
    let mut conn = conn.acquire().await?;

    let record = sqlx::query_as(
        r#"
            SELECT *
            FROM cached_cmd
            WHERE cmd_hash = ?
        "#,
    )
    .bind(cmd_hash)
    .fetch_optional(&mut *conn)
    .await?;

    Ok(record)
}

pub async fn insert_command_with_inputs<'a, A>(
    conn: A,
    raw_cmd: &str,
    cmd_hash: &str,
    input_hash: &str,
    output: &[u8],
    inputs: &[Input],
) -> Result<(i64, Vec<i64>, Vec<i64>), sqlx::Error>
where
    A: Acquire<'a, Database = Sqlite>,
{
    let mut conn = conn.acquire().await?;
    let mut tx = conn.begin().await?;

    delete_command(&mut tx, cmd_hash).await?;
    let command_id = insert_command(&mut tx, raw_cmd, cmd_hash, input_hash, output).await?;

    // Partition and extract file and env inputs
    let (file_inputs, env_inputs) = Input::partition_refs(inputs);

    let file_ids = insert_file_inputs(&mut tx, &file_inputs, command_id).await?;
    let env_ids = insert_env_inputs(&mut tx, &env_inputs, command_id).await?;

    tx.commit().await?;

    Ok((command_id, file_ids, env_ids))
}

async fn insert_command<'a, A>(
    conn: A,
    raw_cmd: &str,
    cmd_hash: &str,
    input_hash: &str,
    output: &[u8],
) -> Result<i64, sqlx::Error>
where
    A: Acquire<'a, Database = Sqlite>,
{
    let mut conn = conn.acquire().await?;

    let record = sqlx::query!(
        r#"
        INSERT INTO cached_cmd (raw, cmd_hash, input_hash, output)
        VALUES (?, ?, ?, ?)
        RETURNING id
        "#,
        raw_cmd,
        cmd_hash,
        input_hash,
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

pub async fn update_command_updated_at<'a, A>(conn: A, id: i64) -> Result<(), sqlx::Error>
where
    A: Acquire<'a, Database = Sqlite>,
{
    let mut conn = conn.acquire().await?;

    sqlx::query!(
        r#"
        UPDATE cached_cmd
        SET updated_at = strftime('%s', 'now')
        WHERE id = ?
        "#,
        id
    )
    .execute(&mut *conn)
    .await?;

    Ok(())
}

async fn insert_file_inputs<'a, A>(
    conn: A,
    file_inputs: &[&FileInputDesc],
    command_id: i64,
) -> Result<Vec<i64>, sqlx::Error>
where
    A: Acquire<'a, Database = Sqlite>,
{
    let mut conn = conn.acquire().await?;

    let insert_file_input = r#"
        INSERT INTO file_input (path, is_directory, content_hash, modified_at)
        VALUES (?, ?, ?, ?)
        ON CONFLICT (path) DO UPDATE
        SET content_hash = excluded.content_hash,
            is_directory = excluded.is_directory,
            modified_at = excluded.modified_at,
            updated_at = strftime('%s', 'now')
        RETURNING id
    "#;

    let mut file_ids = Vec::with_capacity(file_inputs.len());
    for FileInputDesc {
        path,
        is_directory,
        content_hash,
        modified_at,
    } in file_inputs
    {
        let modified_at = modified_at.duration_since(UNIX_EPOCH).unwrap().as_secs() as i64;
        let id: i64 = sqlx::query(insert_file_input)
            .bind(path.to_path_buf().into_os_string().as_bytes())
            .bind(is_directory)
            .bind(content_hash.as_ref().unwrap_or(&"".to_string()))
            .bind(modified_at)
            .fetch_one(&mut *conn)
            .await?
            .get(0);
        file_ids.push(id);
    }

    let cmd_input_path_query = r#"
        INSERT INTO cmd_input_path (cached_cmd_id, file_input_id)
        VALUES (?, ?)
        ON CONFLICT (cached_cmd_id, file_input_id) DO NOTHING
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

async fn insert_env_inputs<'a, A>(
    conn: A,
    env_inputs: &[&EnvInputDesc],
    command_id: i64,
) -> Result<Vec<i64>, sqlx::Error>
where
    A: Acquire<'a, Database = Sqlite>,
{
    let mut conn = conn.acquire().await?;

    let insert_env_input = r#"
        INSERT INTO env_input (cached_cmd_id, name, content_hash)
        VALUES (?, ?, ?)
        ON CONFLICT (cached_cmd_id, name) DO UPDATE
        SET content_hash = excluded.content_hash,
            updated_at = strftime('%s', 'now')
        RETURNING id
    "#;

    let mut env_input_ids = Vec::with_capacity(env_inputs.len());
    for EnvInputDesc { name, content_hash } in env_inputs {
        let id: i64 = sqlx::query(insert_env_input)
            .bind(command_id)
            .bind(name)
            .bind(content_hash.as_ref().unwrap_or(&"".to_string()))
            .fetch_one(&mut *conn)
            .await?
            .get(0);
        env_input_ids.push(id);
    }

    Ok(env_input_ids)
}

/// The row type for the `file_input` table.
#[derive(Clone, Debug, PartialEq)]
pub struct FileInputRow {
    /// A path
    pub path: PathBuf,
    /// Whether the path is a directory
    pub is_directory: bool,
    /// The hash of the file's content
    pub content_hash: String,
    /// The last modified time of the file
    pub modified_at: SystemTime,
    /// The last time the row was updated
    pub updated_at: SystemTime,
}

impl sqlx::FromRow<'_, SqliteRow> for FileInputRow {
    fn from_row(row: &SqliteRow) -> Result<Self, sqlx::Error> {
        let path: &[u8] = row.get("path");
        let is_directory: bool = row.get("is_directory");
        let content_hash: String = row.get("content_hash");
        let modified_at: u64 = row.get("modified_at");
        let updated_at: u64 = row.get("updated_at");
        Ok(Self {
            path: PathBuf::from(OsStr::from_bytes(path)),
            is_directory,
            content_hash,
            modified_at: UNIX_EPOCH + std::time::Duration::from_secs(modified_at),
            updated_at: UNIX_EPOCH + std::time::Duration::from_secs(updated_at),
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EnvInputRow {
    pub name: String,
    pub content_hash: String,
}

impl sqlx::FromRow<'_, SqliteRow> for EnvInputRow {
    fn from_row(row: &SqliteRow) -> Result<Self, sqlx::Error> {
        let name: String = row.get("name");
        let content_hash: String = row.get("content_hash");
        Ok(Self { name, content_hash })
    }
}

pub async fn get_files_by_command_id(
    pool: &SqlitePool,
    command_id: i64,
) -> Result<Vec<FileInputRow>, sqlx::Error> {
    let files = sqlx::query_as(
        r#"
            SELECT f.path, f.is_directory, f.content_hash, f.modified_at, f.updated_at
            FROM file_input f
            JOIN cmd_input_path cip ON f.id = cip.file_input_id
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
) -> Result<Vec<FileInputRow>, sqlx::Error> {
    let files = sqlx::query_as(
        r#"
            SELECT f.path, f.is_directory, f.content_hash, f.modified_at, f.updated_at
            FROM file_input f
            JOIN cmd_input_path cip ON f.id = cip.file_input_id
            JOIN cached_cmd cc ON cip.cached_cmd_id = cc.id
            WHERE cc.cmd_hash = ?
        "#,
    )
    .bind(command_hash)
    .fetch_all(pool)
    .await?;

    Ok(files)
}

pub async fn get_envs_by_command_id(
    pool: &SqlitePool,
    command_id: i64,
) -> Result<Vec<EnvInputRow>, sqlx::Error> {
    let files = sqlx::query_as(
        r#"
            SELECT e.name, e.content_hash, e.updated_at
            FROM env_input e
            WHERE e.cached_cmd_id = ?
        "#,
    )
    .bind(command_id)
    .fetch_all(pool)
    .await?;

    Ok(files)
}

pub async fn get_envs_by_command_hash(
    pool: &SqlitePool,
    command_hash: &str,
) -> Result<Vec<EnvInputRow>, sqlx::Error> {
    let files = sqlx::query_as(
        r#"
            SELECT e.name, e.content_hash, e.updated_at
            FROM env_input e
            JOIN cached_cmd cc ON e.cached_cmd_id = cc.id
            WHERE cc.cmd_hash = ?
        "#,
    )
    .bind(command_hash)
    .fetch_all(pool)
    .await?;

    Ok(files)
}

pub async fn update_file_modified_at<P: AsRef<Path>>(
    pool: &SqlitePool,
    path: P,
    modified_at: SystemTime,
) -> Result<(), sqlx::Error> {
    let modified_at = modified_at.duration_since(UNIX_EPOCH).unwrap().as_secs() as i64;

    sqlx::query(
        r#"
        UPDATE file_input
        SET modified_at = ?, updated_at = strftime('%s', 'now')
        WHERE path = ?
        "#,
    )
    .bind(modified_at)
    .bind(path.as_ref().to_path_buf().into_os_string().as_bytes())
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn delete_unreferenced_files(pool: &SqlitePool) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        r#"
        DELETE FROM file_input
        WHERE NOT EXISTS (
            SELECT 1
            FROM cmd_input_path
            WHERE cmd_input_path.file_input_id = file_input.id
        )
        "#,
    )
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

#[cfg(test)]
mod tests {
    use crate::hash;

    use super::*;
    use sqlx::SqlitePool;

    #[sqlx::test]
    async fn test_insert_and_retrieve_command(pool: SqlitePool) {
        let raw_cmd = "nix-build -A hello";
        let cmd_hash = hash::digest(raw_cmd);
        let output = b"Hello, world!";
        let modified_at = SystemTime::now();
        let inputs = vec![
            Input::File(FileInputDesc {
                path: "/path/to/file1".into(),
                is_directory: false,
                content_hash: Some("hash1".to_string()),
                modified_at,
            }),
            Input::File(FileInputDesc {
                path: "/path/to/file2".into(),
                is_directory: false,
                content_hash: Some("hash2".to_string()),
                modified_at,
            }),
        ];
        let input_hash = hash::digest(
            &inputs
                .iter()
                .filter_map(Input::content_hash)
                .collect::<String>(),
        );

        let (command_id, file_ids, _) =
            insert_command_with_inputs(&pool, raw_cmd, &cmd_hash, &input_hash, output, &inputs)
                .await
                .unwrap();

        assert_eq!(file_ids.len(), 2);

        let retrieved_command = get_command_by_hash(&pool, &cmd_hash)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(retrieved_command.raw, raw_cmd);
        assert_eq!(retrieved_command.cmd_hash, cmd_hash);
        assert_eq!(retrieved_command.output, output);

        let files = get_files_by_command_id(&pool, command_id).await.unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, PathBuf::from("/path/to/file1"));
        assert_eq!(files[0].content_hash, "hash1");
        assert_eq!(files[1].path, PathBuf::from("/path/to/file2"));
        assert_eq!(files[1].content_hash, "hash2");
    }

    #[sqlx::test]
    async fn test_insert_multiple_commands(pool: SqlitePool) {
        // First command
        let raw_cmd1 = "nix-build -A hello";
        let cmd_hash1 = hash::digest(raw_cmd1);
        let output1 = b"Hello, world!";
        let modified_at = SystemTime::now();
        let inputs1 = vec![
            Input::File(FileInputDesc {
                path: "/path/to/file1".into(),
                is_directory: false,
                content_hash: Some("hash1".to_string()),
                modified_at,
            }),
            Input::File(FileInputDesc {
                path: "/path/to/file2".into(),
                is_directory: false,
                content_hash: Some("hash2".to_string()),
                modified_at,
            }),
        ];
        let input_hash1 = hash::digest(
            &inputs1
                .iter()
                .filter_map(Input::content_hash)
                .collect::<String>(),
        );

        let (command_id1, file_ids1, _) = insert_command_with_inputs(
            &pool,
            raw_cmd1,
            &cmd_hash1,
            &input_hash1,
            output1,
            &inputs1,
        )
        .await
        .unwrap();

        // Second command
        let raw_cmd2 = "nix-build -A goodbye";
        let cmd_hash2 = hash::digest(raw_cmd2);
        let output2 = b"Goodbye, world!";
        let modified_at = SystemTime::now();
        let inputs2 = vec![
            Input::File(FileInputDesc {
                path: "/path/to/file2".into(),
                is_directory: false,
                content_hash: Some("hash2".to_string()),
                modified_at,
            }),
            Input::File(FileInputDesc {
                path: "/path/to/file3".into(),
                is_directory: false,
                content_hash: Some("hash3".to_string()),
                modified_at,
            }),
        ];
        let input_hash2 = hash::digest(
            &inputs2
                .iter()
                .filter_map(Input::content_hash)
                .collect::<String>(),
        );

        let (command_id2, file_ids2, _) = insert_command_with_inputs(
            &pool,
            raw_cmd2,
            &cmd_hash2,
            &input_hash2,
            output2,
            &inputs2,
        )
        .await
        .unwrap();

        // Verify first command
        let retrieved_command1 = get_command_by_hash(&pool, &cmd_hash1)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(retrieved_command1.raw, raw_cmd1);
        let files1 = get_files_by_command_id(&pool, command_id1).await.unwrap();
        assert_eq!(files1.len(), 2);

        // Verify second command
        let retrieved_command2 = get_command_by_hash(&pool, &cmd_hash2)
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
        let cmd_hash = hash::digest(raw_cmd);
        let output = b"Hello, world!";
        let modified_at = SystemTime::now();
        let inputs1 = vec![
            Input::File(FileInputDesc {
                path: "/path/to/file1".into(),
                is_directory: false,
                content_hash: Some("hash1".to_string()),
                modified_at,
            }),
            Input::File(FileInputDesc {
                path: "/path/to/file2".into(),
                is_directory: false,
                content_hash: Some("hash2".to_string()),
                modified_at,
            }),
        ];
        let input_hash = hash::digest(
            &inputs1
                .iter()
                .filter_map(Input::content_hash)
                .collect::<String>(),
        );

        let (_command_id1, file_ids1, _) =
            insert_command_with_inputs(&pool, raw_cmd, &cmd_hash, &input_hash, output, &inputs1)
                .await
                .unwrap();

        // Second command
        let inputs2 = vec![
            Input::File(FileInputDesc {
                path: "/path/to/file2".into(),
                is_directory: false,
                content_hash: Some("hash2".to_string()),
                modified_at,
            }),
            Input::File(FileInputDesc {
                path: "/path/to/file3".into(),
                is_directory: false,
                content_hash: Some("hash3".to_string()),
                modified_at,
            }),
        ];
        let input_hash2 = hash::digest(
            &inputs2
                .iter()
                .filter_map(Input::content_hash)
                .collect::<String>(),
        );

        let (command_id2, file_ids2, _) =
            insert_command_with_inputs(&pool, raw_cmd, &cmd_hash, &input_hash2, output, &inputs2)
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
        assert_eq!(files[0].path, PathBuf::from("/path/to/file2"));
        assert_eq!(files[0].content_hash, "hash2");
        assert_eq!(files[1].path, PathBuf::from("/path/to/file3"));
        assert_eq!(files[1].content_hash, "hash3");

        // Verify that file2 is reused and file3 is new
        assert_eq!(file_ids2.len(), 2);
        assert!(file_ids1.contains(&file_ids2[0])); // file2 is reused
        assert!(!file_ids1.contains(&file_ids2[1])); // file3 is new
    }
}
