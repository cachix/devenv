use super::command::{EnvInputDesc, FileInputDesc, Input};
use devenv_cache_core::{error::CacheError, error::CacheResult, file::TrackedFile, time};
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use turso::{Connection, Row as TursoRow, params};

// Get the migrations directory path
pub fn migrations_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations")
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

impl CommandRow {
    fn from_turso_row(row: &TursoRow) -> CacheResult<Self> {
        let id: i64 = row.get(0)?;
        let raw: String = row.get(1)?;
        let cmd_hash: String = row.get(2)?;
        let input_hash: String = row.get(3)?;
        let output: Vec<u8> = row.get(4)?;
        let updated_at: i64 = row.get(5)?;

        Ok(Self {
            id,
            raw,
            cmd_hash,
            input_hash,
            output,
            updated_at: time::system_time_from_unix_seconds(updated_at),
        })
    }
}

pub async fn get_command_by_hash(
    conn: &Connection,
    cmd_hash: &str,
) -> CacheResult<Option<CommandRow>> {
    let mut stmt = conn
        .prepare("SELECT id, raw, cmd_hash, input_hash, output, updated_at FROM cached_cmd WHERE cmd_hash = ?")
        .await?;

    let mut rows = stmt.query(params![cmd_hash]).await?;

    if let Some(row) = rows.next().await? {
        Ok(Some(CommandRow::from_turso_row(&row)?))
    } else {
        Ok(None)
    }
}

pub async fn insert_command_with_inputs(
    conn: &Connection,
    raw_cmd: &str,
    cmd_hash: &str,
    input_hash: &str,
    output: &[u8],
    inputs: &[Input],
) -> CacheResult<(i64, Vec<i64>, Vec<i64>)> {
    // Begin transaction to ensure atomicity
    conn.execute("BEGIN TRANSACTION", params![]).await?;

    // Use a closure to handle the transaction result
    let result = async {
        delete_command(conn, cmd_hash).await?;
        let command_id = insert_command(conn, raw_cmd, cmd_hash, input_hash, output).await?;

        // Partition and extract file and env inputs
        let (file_inputs, env_inputs) = Input::partition_refs(inputs);

        let file_ids = insert_file_inputs(conn, &file_inputs, command_id).await?;
        let env_ids = insert_env_inputs(conn, &env_inputs, command_id).await?;

        Ok((command_id, file_ids, env_ids))
    }
    .await;

    // Commit or rollback based on result
    match result {
        Ok(data) => {
            conn.execute("COMMIT", params![]).await?;
            Ok(data)
        }
        Err(e) => {
            conn.execute("ROLLBACK", params![]).await?;
            Err(e)
        }
    }
}

async fn insert_command(
    conn: &Connection,
    raw_cmd: &str,
    cmd_hash: &str,
    input_hash: &str,
    output: &[u8],
) -> CacheResult<i64> {
    let mut stmt = conn
        .prepare("INSERT INTO cached_cmd (raw, cmd_hash, input_hash, output) VALUES (?, ?, ?, ?) RETURNING id")
        .await?;

    let mut rows = stmt
        .query(params![raw_cmd, cmd_hash, input_hash, output])
        .await?;

    let row = rows
        .next()
        .await?
        .ok_or_else(|| CacheError::Io(std::io::Error::other("No id returned from insert")))?;

    let id: i64 = row.get(0)?;
    Ok(id)
}

async fn delete_command(conn: &Connection, cmd_hash: &str) -> CacheResult<()> {
    conn.execute(
        "DELETE FROM cached_cmd WHERE cmd_hash = ?",
        params![cmd_hash],
    )
    .await?;
    Ok(())
}

pub async fn update_command_updated_at(conn: &Connection, id: i64) -> CacheResult<()> {
    let now = time::system_time_to_unix_seconds(SystemTime::now());
    conn.execute(
        "UPDATE cached_cmd SET updated_at = ? WHERE id = ?",
        params![now, id],
    )
    .await?;
    Ok(())
}

async fn insert_file_inputs(
    conn: &Connection,
    file_inputs: &[&FileInputDesc],
    command_id: i64,
) -> CacheResult<Vec<i64>> {
    let insert_file_input = "INSERT INTO file_input (path, is_directory, content_hash, modified_at) \
                             VALUES (?, ?, ?, ?) \
                             ON CONFLICT (path) DO UPDATE \
                             SET content_hash = excluded.content_hash, \
                                 is_directory = excluded.is_directory, \
                                 modified_at = excluded.modified_at, \
                                 updated_at = ? \
                             RETURNING id";

    let now = time::system_time_to_unix_seconds(SystemTime::now());
    let mut file_ids = Vec::with_capacity(file_inputs.len());

    for FileInputDesc {
        path,
        is_directory,
        content_hash,
        modified_at,
    } in file_inputs
    {
        let modified_at_unix = time::system_time_to_unix_seconds(*modified_at);

        // Note: turso requires preparing statement for each query execution
        let mut stmt = conn.prepare(insert_file_input).await?;

        let mut rows = stmt
            .query(params![
                path.to_path_buf().into_os_string().as_bytes(),
                is_directory,
                content_hash.as_deref().unwrap_or(""),
                modified_at_unix,
                now
            ])
            .await?;

        let row = rows.next().await?.ok_or_else(|| {
            CacheError::Io(std::io::Error::other(
                "No id returned from file input insert",
            ))
        })?;

        let id: i64 = row.get(0)?;
        file_ids.push(id);
    }

    let cmd_input_path_query = "INSERT INTO cmd_input_path (cached_cmd_id, file_input_id) \
                                VALUES (?, ?) \
                                ON CONFLICT (cached_cmd_id, file_input_id) DO NOTHING";

    for &file_id in &file_ids {
        conn.execute(cmd_input_path_query, params![command_id, file_id])
            .await?;
    }

    Ok(file_ids)
}

async fn insert_env_inputs(
    conn: &Connection,
    env_inputs: &[&EnvInputDesc],
    command_id: i64,
) -> CacheResult<Vec<i64>> {
    let insert_env_input = "INSERT INTO env_input (cached_cmd_id, name, content_hash) \
                            VALUES (?, ?, ?) \
                            ON CONFLICT (cached_cmd_id, name) DO UPDATE \
                            SET content_hash = excluded.content_hash, \
                                updated_at = ? \
                            RETURNING id";

    let now = time::system_time_to_unix_seconds(SystemTime::now());
    let mut env_input_ids = Vec::with_capacity(env_inputs.len());

    for EnvInputDesc { name, content_hash } in env_inputs {
        // Note: turso requires preparing statement for each query execution
        let mut stmt = conn.prepare(insert_env_input).await?;

        let mut rows = stmt
            .query(params![
                command_id,
                name.as_str(),
                content_hash.as_deref().unwrap_or(""),
                now
            ])
            .await?;

        let row = rows.next().await?.ok_or_else(|| {
            CacheError::Io(std::io::Error::other(
                "No id returned from env input insert",
            ))
        })?;

        let id: i64 = row.get(0)?;
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

impl FileInputRow {
    fn from_turso_row(row: &TursoRow) -> CacheResult<Self> {
        let path: Vec<u8> = row.get(0)?;
        let is_directory: bool = row.get(1)?;
        let content_hash: String = row.get(2)?;
        let modified_at: i64 = row.get(3)?;
        let updated_at: i64 = row.get(4)?;

        Ok(Self {
            path: PathBuf::from(OsStr::from_bytes(&path)),
            is_directory,
            content_hash,
            modified_at: time::system_time_from_unix_seconds(modified_at),
            updated_at: time::system_time_from_unix_seconds(updated_at),
        })
    }
}

// Helper method to convert a FileInputRow to a TrackedFile
impl FileInputRow {
    pub fn to_tracked_file(&self) -> TrackedFile {
        TrackedFile {
            path: self.path.clone(),
            is_directory: self.is_directory,
            content_hash: Some(self.content_hash.clone()),
            modified_at: self.modified_at,
            checked_at: self.updated_at,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EnvInputRow {
    pub name: String,
    pub content_hash: String,
}

impl EnvInputRow {
    fn from_turso_row(row: &TursoRow) -> CacheResult<Self> {
        let name: String = row.get(0)?;
        let content_hash: String = row.get(1)?;
        Ok(Self { name, content_hash })
    }
}

pub async fn get_files_by_command_id(
    conn: &Connection,
    command_id: i64,
) -> CacheResult<Vec<FileInputRow>> {
    let mut stmt = conn
        .prepare(
            "SELECT f.path, f.is_directory, f.content_hash, f.modified_at, f.updated_at \
         FROM file_input f \
         JOIN cmd_input_path cip ON f.id = cip.file_input_id \
         WHERE cip.cached_cmd_id = ?",
        )
        .await?;

    let mut rows = stmt.query(params![command_id]).await?;

    let mut files = Vec::new();
    while let Some(row) = rows.next().await? {
        files.push(FileInputRow::from_turso_row(&row)?);
    }

    Ok(files)
}

pub async fn get_files_by_command_hash(
    conn: &Connection,
    command_hash: &str,
) -> CacheResult<Vec<FileInputRow>> {
    let mut stmt = conn
        .prepare(
            "SELECT f.path, f.is_directory, f.content_hash, f.modified_at, f.updated_at \
         FROM file_input f \
         JOIN cmd_input_path cip ON f.id = cip.file_input_id \
         JOIN cached_cmd cc ON cip.cached_cmd_id = cc.id \
         WHERE cc.cmd_hash = ?",
        )
        .await?;

    let mut rows = stmt.query(params![command_hash]).await?;

    let mut files = Vec::new();
    while let Some(row) = rows.next().await? {
        files.push(FileInputRow::from_turso_row(&row)?);
    }

    Ok(files)
}

pub async fn get_envs_by_command_id(
    conn: &Connection,
    command_id: i64,
) -> CacheResult<Vec<EnvInputRow>> {
    let mut stmt = conn
        .prepare(
            "SELECT e.name, e.content_hash \
         FROM env_input e \
         WHERE e.cached_cmd_id = ?",
        )
        .await?;

    let mut rows = stmt.query(params![command_id]).await?;

    let mut envs = Vec::new();
    while let Some(row) = rows.next().await? {
        envs.push(EnvInputRow::from_turso_row(&row)?);
    }

    Ok(envs)
}

pub async fn get_envs_by_command_hash(
    conn: &Connection,
    command_hash: &str,
) -> CacheResult<Vec<EnvInputRow>> {
    let mut stmt = conn
        .prepare(
            "SELECT e.name, e.content_hash \
         FROM env_input e \
         JOIN cached_cmd cc ON e.cached_cmd_id = cc.id \
         WHERE cc.cmd_hash = ?",
        )
        .await?;

    let mut rows = stmt.query(params![command_hash]).await?;

    let mut envs = Vec::new();
    while let Some(row) = rows.next().await? {
        envs.push(EnvInputRow::from_turso_row(&row)?);
    }

    Ok(envs)
}

pub async fn update_file_modified_at<P: AsRef<Path>>(
    conn: &Connection,
    path: P,
    modified_at: SystemTime,
) -> CacheResult<()> {
    let modified_at_unix = time::system_time_to_unix_seconds(modified_at);
    let now = time::system_time_to_unix_seconds(SystemTime::now());

    conn.execute(
        "UPDATE file_input SET modified_at = ?, updated_at = ? WHERE path = ?",
        params![
            modified_at_unix,
            now,
            path.as_ref().to_path_buf().into_os_string().as_bytes()
        ],
    )
    .await?;

    Ok(())
}

pub async fn delete_unreferenced_files(conn: &Connection) -> CacheResult<u64> {
    let result = conn
        .execute(
            "DELETE FROM file_input \
         WHERE NOT EXISTS ( \
             SELECT 1 \
             FROM cmd_input_path \
             WHERE cmd_input_path.file_input_id = file_input.id \
         )",
            params![],
        )
        .await?;

    Ok(result as u64)
}

#[cfg(test)]
mod tests {
    use devenv_cache_core::{compute_string_hash, db::Database};
    use tempfile::TempDir;

    use super::*;

    async fn setup_test_db() -> (TempDir, Database, Connection) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let migrations_dir = migrations_dir();

        let db = Database::new(db_path, &migrations_dir).await.unwrap();
        let conn = db.connect().await.unwrap();

        (temp_dir, db, conn)
    }

    #[tokio::test]
    async fn test_insert_and_retrieve_command() {
        let (_temp_dir, _db, conn) = setup_test_db().await;

        let raw_cmd = "nix-build -A hello";
        let cmd_hash = compute_string_hash(raw_cmd);
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
        let input_hash = compute_string_hash(
            &inputs
                .iter()
                .filter_map(Input::content_hash)
                .collect::<String>(),
        );

        let (command_id, file_ids, _) =
            insert_command_with_inputs(&conn, raw_cmd, &cmd_hash, &input_hash, output, &inputs)
                .await
                .unwrap();

        assert_eq!(file_ids.len(), 2);

        let retrieved_command = get_command_by_hash(&conn, &cmd_hash)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(retrieved_command.raw, raw_cmd);
        assert_eq!(retrieved_command.cmd_hash, cmd_hash);
        assert_eq!(retrieved_command.output, output);

        let files = get_files_by_command_id(&conn, command_id).await.unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, PathBuf::from("/path/to/file1"));
        assert_eq!(files[0].content_hash, "hash1");
        assert_eq!(files[1].path, PathBuf::from("/path/to/file2"));
        assert_eq!(files[1].content_hash, "hash2");
    }

    #[tokio::test]
    async fn test_insert_multiple_commands() {
        let (_temp_dir, _db, conn) = setup_test_db().await;

        // First command
        let raw_cmd1 = "nix-build -A hello";
        let cmd_hash1 = compute_string_hash(raw_cmd1);
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
        let input_hash1 = compute_string_hash(
            &inputs1
                .iter()
                .filter_map(Input::content_hash)
                .collect::<String>(),
        );

        let (command_id1, file_ids1, _) = insert_command_with_inputs(
            &conn,
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
        let cmd_hash2 = compute_string_hash(raw_cmd2);
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
        let input_hash2 = compute_string_hash(
            &inputs2
                .iter()
                .filter_map(Input::content_hash)
                .collect::<String>(),
        );

        let (command_id2, file_ids2, _) = insert_command_with_inputs(
            &conn,
            raw_cmd2,
            &cmd_hash2,
            &input_hash2,
            output2,
            &inputs2,
        )
        .await
        .unwrap();

        // Verify first command
        let retrieved_command1 = get_command_by_hash(&conn, &cmd_hash1)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(retrieved_command1.raw, raw_cmd1);
        let files1 = get_files_by_command_id(&conn, command_id1).await.unwrap();
        assert_eq!(files1.len(), 2);

        // Verify second command
        let retrieved_command2 = get_command_by_hash(&conn, &cmd_hash2)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(retrieved_command2.raw, raw_cmd2);
        let files2 = get_files_by_command_id(&conn, command_id2).await.unwrap();
        assert_eq!(files2.len(), 2);

        // Verify cmd_input_path rows - count them manually
        let mut stmt = conn
            .prepare("SELECT COUNT(*) FROM cmd_input_path")
            .await
            .unwrap();
        let mut rows = stmt.query(params![]).await.unwrap();
        let row = rows.next().await.unwrap().unwrap();
        let count: i64 = row.get(0).unwrap();
        assert_eq!(count, 4); // 2 files for each command

        // Verify file reuse
        assert_eq!(file_ids1.len(), 2);
        assert_eq!(file_ids2.len(), 2);
        assert!(file_ids1.contains(&file_ids2[0])); // file2 is shared between commands
    }

    #[tokio::test]
    async fn test_insert_command_with_modified_files() {
        let (_temp_dir, _db, conn) = setup_test_db().await;

        // First command
        let raw_cmd = "nix-build -A hello";
        let cmd_hash = compute_string_hash(raw_cmd);
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
        let input_hash = compute_string_hash(
            &inputs1
                .iter()
                .filter_map(Input::content_hash)
                .collect::<String>(),
        );

        let (_command_id1, file_ids1, _) =
            insert_command_with_inputs(&conn, raw_cmd, &cmd_hash, &input_hash, output, &inputs1)
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
        let input_hash2 = compute_string_hash(
            &inputs2
                .iter()
                .filter_map(Input::content_hash)
                .collect::<String>(),
        );

        let (command_id2, file_ids2, _) =
            insert_command_with_inputs(&conn, raw_cmd, &cmd_hash, &input_hash2, output, &inputs2)
                .await
                .unwrap();

        // Investigate the files associated with the new command
        let files = get_files_by_command_id(&conn, command_id2).await.unwrap();
        println!(
            "Number of files associated with the command: {}",
            files.len()
        );
        for file in &files {
            println!("File path: {:?}, hash: {}", file.path, file.content_hash);
        }

        // Check if files are being accumulated instead of replaced
        assert_eq!(
            files.len(),
            2,
            "Expected 2 files, but found {}. Files might be accumulating instead of being replaced.",
            files.len()
        );

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
        let files = get_files_by_command_id(&conn, command_id2).await.unwrap();
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
