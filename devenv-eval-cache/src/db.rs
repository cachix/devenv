use crate::eval_inputs::{EnvInputDesc, FileInputDesc, Input};
use devenv_cache_core::db::Dir;
use devenv_cache_core::error::CacheError;
use devenv_cache_core::{file::TrackedFile, time};
use include_dir::include_dir;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use turso::{Connection, Row};

/// Embedded migrations directory for the eval cache database.
/// These are included at compile time so they work in Nix-built binaries.
pub static MIGRATIONS_DIR: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/migrations");

/// Path to the migrations folder (only used during development/testing)
#[cfg(test)]
pub const MIGRATIONS_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/migrations");

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
    fn from_row(row: &Row) -> Result<Self, turso::Error> {
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

impl From<FileInputRow> for Input {
    fn from(row: FileInputRow) -> Self {
        Self::File(row.into())
    }
}

impl From<FileInputRow> for FileInputDesc {
    fn from(row: FileInputRow) -> Self {
        Self {
            path: row.path,
            is_directory: row.is_directory,
            content_hash: if row.content_hash.is_empty() {
                None
            } else {
                Some(row.content_hash)
            },
            modified_at: row.modified_at,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EnvInputRow {
    pub name: String,
    pub content_hash: String,
}

impl EnvInputRow {
    fn from_row(row: &Row) -> Result<Self, turso::Error> {
        let name: String = row.get(0)?;
        let content_hash: String = row.get(1)?;
        Ok(Self { name, content_hash })
    }
}

impl From<EnvInputRow> for Input {
    fn from(row: EnvInputRow) -> Self {
        Self::Env(row.into())
    }
}

impl From<EnvInputRow> for EnvInputDesc {
    fn from(row: EnvInputRow) -> Self {
        Self {
            name: row.name,
            content_hash: if row.content_hash.is_empty() {
                None
            } else {
                Some(row.content_hash)
            },
        }
    }
}

pub async fn update_file_modified_at<P: AsRef<Path>>(
    conn: &Connection,
    path: P,
    modified_at: SystemTime,
) -> Result<(), CacheError> {
    let modified_at = time::system_time_to_unix_seconds(modified_at);
    let now = time::system_time_to_unix_seconds(SystemTime::now());
    let path_bytes = path.as_ref().to_path_buf().into_os_string();

    conn.execute(
        r#"
        UPDATE file_input
        SET modified_at = ?, updated_at = ?
        WHERE path = ?
        "#,
        turso::params![modified_at, now, path_bytes.as_bytes()],
    )
    .await?;

    Ok(())
}

// =============================================================================
// Eval Cache Database Functions
// =============================================================================

/// The row type for the `cached_eval` table.
#[derive(Clone, Debug)]
pub struct EvalRow {
    /// The primary key
    pub id: i64,
    /// A hash of the cache key (NixArgs + attr_name)
    pub key_hash: String,
    /// The attribute name being evaluated
    pub attr_name: String,
    /// A hash of the content hashes of the input files/envs
    pub input_hash: String,
    /// The JSON output of the evaluation
    pub json_output: String,
    /// The time the cached eval was checked or created
    pub updated_at: SystemTime,
}

impl EvalRow {
    fn from_row(row: &Row) -> Result<Self, turso::Error> {
        let id: i64 = row.get(0)?;
        let key_hash: String = row.get(1)?;
        let attr_name: String = row.get(2)?;
        let input_hash: String = row.get(3)?;
        let json_output: String = row.get(4)?;
        let updated_at: i64 = row.get(5)?;
        Ok(Self {
            id,
            key_hash,
            attr_name,
            input_hash,
            json_output,
            updated_at: time::system_time_from_unix_seconds(updated_at),
        })
    }
}

/// Get a cached eval by its key hash.
pub async fn get_eval_by_key_hash(
    conn: &Connection,
    key_hash: &str,
) -> Result<Option<EvalRow>, CacheError> {
    let mut stmt = conn
        .prepare(
            r#"
            SELECT id, key_hash, attr_name, input_hash, json_output, updated_at
            FROM cached_eval
            WHERE key_hash = ?
        "#,
        )
        .await?;

    let mut rows = stmt.query(turso::params![key_hash]).await?;

    match rows.next().await? {
        Some(row) => Ok(Some(EvalRow::from_row(&row)?)),
        None => Ok(None),
    }
}

/// Insert a cached eval with its inputs (files and env vars).
pub async fn insert_eval_with_inputs(
    conn: &Connection,
    key_hash: &str,
    attr_name: &str,
    input_hash: &str,
    json_output: &str,
    inputs: &[Input],
) -> Result<(i64, Vec<i64>, Vec<i64>), CacheError> {
    // Start transaction
    conn.execute("BEGIN TRANSACTION", ()).await?;

    let result = async {
        delete_eval(conn, key_hash).await?;
        let eval_id = insert_eval(conn, key_hash, attr_name, input_hash, json_output).await?;

        // Partition and extract file and env inputs
        let (file_inputs, env_inputs) = Input::partition_refs(inputs);

        let file_ids = insert_eval_file_inputs(conn, &file_inputs, eval_id).await?;
        let env_ids = insert_eval_env_inputs(conn, &env_inputs, eval_id).await?;

        Ok::<_, CacheError>((eval_id, file_ids, env_ids))
    }
    .await;

    match result {
        Ok(data) => {
            conn.execute("COMMIT", ()).await?;
            Ok(data)
        }
        Err(e) => {
            let _ = conn.execute("ROLLBACK", ()).await;
            Err(e)
        }
    }
}

async fn insert_eval(
    conn: &Connection,
    key_hash: &str,
    attr_name: &str,
    input_hash: &str,
    json_output: &str,
) -> Result<i64, CacheError> {
    let mut stmt = conn
        .prepare(
            r#"
        INSERT INTO cached_eval (key_hash, attr_name, input_hash, json_output)
        VALUES (?, ?, ?, ?)
        RETURNING id
        "#,
        )
        .await?;

    let mut rows = stmt
        .query(turso::params![key_hash, attr_name, input_hash, json_output])
        .await?;

    let row = rows
        .next()
        .await?
        .ok_or_else(|| CacheError::initialization("Insert failed - no row returned"))?;
    let id: i64 = row.get(0)?;
    Ok(id)
}

async fn delete_eval(conn: &Connection, key_hash: &str) -> Result<(), CacheError> {
    conn.execute(
        r#"
        DELETE FROM cached_eval
        WHERE key_hash = ?
        "#,
        turso::params![key_hash],
    )
    .await?;

    Ok(())
}

/// Update the updated_at timestamp for a cached eval.
pub async fn update_eval_updated_at(conn: &Connection, id: i64) -> Result<(), CacheError> {
    let now = time::system_time_to_unix_seconds(SystemTime::now());

    conn.execute(
        r#"
        UPDATE cached_eval
        SET updated_at = ?
        WHERE id = ?
        "#,
        turso::params![now, id],
    )
    .await?;

    Ok(())
}

async fn insert_eval_file_inputs(
    conn: &Connection,
    file_inputs: &[&FileInputDesc],
    eval_id: i64,
) -> Result<Vec<i64>, CacheError> {
    let insert_file_input = r#"
        INSERT INTO file_input (path, is_directory, content_hash, modified_at)
        VALUES (?, ?, ?, ?)
        ON CONFLICT (path) DO UPDATE
        SET content_hash = excluded.content_hash,
            is_directory = excluded.is_directory,
            modified_at = excluded.modified_at,
            updated_at = ?
        RETURNING id
    "#;

    let now = time::system_time_to_unix_seconds(SystemTime::now());
    let mut file_ids = Vec::with_capacity(file_inputs.len());

    for FileInputDesc {
        path,
        is_directory,
        content_hash,
        modified_at,
    } in file_inputs
    {
        let modified_at = time::system_time_to_unix_seconds(*modified_at);
        let path_bytes = path.to_path_buf().into_os_string();
        let content_hash_str = content_hash.as_ref().map(|s| s.as_str()).unwrap_or("");

        let mut stmt = conn.prepare(insert_file_input).await?;
        let mut rows = stmt
            .query(turso::params![
                path_bytes.as_bytes(),
                is_directory,
                content_hash_str,
                modified_at,
                now
            ])
            .await?;

        let row = rows
            .next()
            .await?
            .ok_or_else(|| CacheError::initialization("Insert file failed - no row returned"))?;
        let id: i64 = row.get(0)?;
        file_ids.push(id);
    }

    // Link to eval via eval_input_path table
    let eval_input_path_query = r#"
        INSERT INTO eval_input_path (cached_eval_id, file_input_id)
        VALUES (?, ?)
        ON CONFLICT (cached_eval_id, file_input_id) DO NOTHING
    "#;

    for &file_id in &file_ids {
        conn.execute(eval_input_path_query, turso::params![eval_id, file_id])
            .await?;
    }

    Ok(file_ids)
}

async fn insert_eval_env_inputs(
    conn: &Connection,
    env_inputs: &[&EnvInputDesc],
    eval_id: i64,
) -> Result<Vec<i64>, CacheError> {
    let insert_env_input = r#"
        INSERT INTO eval_env_input (cached_eval_id, name, content_hash)
        VALUES (?, ?, ?)
        ON CONFLICT (cached_eval_id, name) DO UPDATE
        SET content_hash = excluded.content_hash,
            updated_at = ?
        RETURNING id
    "#;

    let now = time::system_time_to_unix_seconds(SystemTime::now());
    let mut env_input_ids = Vec::with_capacity(env_inputs.len());

    for EnvInputDesc { name, content_hash } in env_inputs {
        let content_hash_str = content_hash.as_ref().map(|s| s.as_str()).unwrap_or("");

        let mut stmt = conn.prepare(insert_env_input).await?;
        let mut rows = stmt
            .query(turso::params![eval_id, name.clone(), content_hash_str, now])
            .await?;

        let row = rows
            .next()
            .await?
            .ok_or_else(|| CacheError::initialization("Insert env failed - no row returned"))?;
        let id: i64 = row.get(0)?;
        env_input_ids.push(id);
    }

    Ok(env_input_ids)
}

/// Get file inputs for a cached eval by eval ID.
pub async fn get_files_by_eval_id(
    conn: &Connection,
    eval_id: i64,
) -> Result<Vec<FileInputRow>, CacheError> {
    let mut stmt = conn
        .prepare(
            r#"
            SELECT f.path, f.is_directory, f.content_hash, f.modified_at, f.updated_at
            FROM file_input f
            JOIN eval_input_path eip ON f.id = eip.file_input_id
            WHERE eip.cached_eval_id = ?
        "#,
        )
        .await?;

    let mut rows = stmt.query(turso::params![eval_id]).await?;
    let mut files = Vec::new();

    while let Some(row) = rows.next().await? {
        files.push(FileInputRow::from_row(&row)?);
    }

    Ok(files)
}

/// Get env inputs for a cached eval by eval ID.
pub async fn get_envs_by_eval_id(
    conn: &Connection,
    eval_id: i64,
) -> Result<Vec<EnvInputRow>, CacheError> {
    let mut stmt = conn
        .prepare(
            r#"
            SELECT e.name, e.content_hash
            FROM eval_env_input e
            WHERE e.cached_eval_id = ?
        "#,
        )
        .await?;

    let mut rows = stmt.query(turso::params![eval_id]).await?;
    let mut envs = Vec::new();

    while let Some(row) = rows.next().await? {
        envs.push(EnvInputRow::from_row(&row)?);
    }

    Ok(envs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use devenv_cache_core::compute_string_hash;
    use devenv_cache_core::db::Database;
    use std::path::Path;
    use tempfile::TempDir;

    async fn setup_test_db() -> (Database, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let migrations_path = Path::new(MIGRATIONS_PATH);
        let db = Database::new(db_path, migrations_path).await.unwrap();
        (db, temp_dir)
    }

    #[tokio::test]
    async fn test_insert_and_retrieve_eval() {
        let (db, _temp_dir) = setup_test_db().await;
        let conn = db.connect().await.unwrap();

        let key_hash = compute_string_hash("(import /foo/bar {}):config.shell");
        let attr_name = "config.shell";
        let json_output = r#"{"type":"derivation","path":"/nix/store/abc-shell"}"#;
        let modified_at = SystemTime::now();
        let inputs = vec![
            Input::File(FileInputDesc {
                path: "/path/to/devenv.nix".into(),
                is_directory: false,
                content_hash: Some("hash1".to_string()),
                modified_at,
            }),
            Input::Env(EnvInputDesc {
                name: "HOME".to_string(),
                content_hash: Some("hash2".to_string()),
            }),
        ];
        let input_hash = Input::compute_input_hash(&inputs);

        let (eval_id, file_ids, env_ids) = insert_eval_with_inputs(
            &conn,
            &key_hash,
            attr_name,
            &input_hash,
            json_output,
            &inputs,
        )
        .await
        .unwrap();

        assert_eq!(file_ids.len(), 1);
        assert_eq!(env_ids.len(), 1);

        // Retrieve the eval
        let retrieved_eval = get_eval_by_key_hash(&conn, &key_hash)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(retrieved_eval.key_hash, key_hash);
        assert_eq!(retrieved_eval.attr_name, attr_name);
        assert_eq!(retrieved_eval.json_output, json_output);
        assert_eq!(retrieved_eval.input_hash, input_hash);

        // Retrieve file inputs
        let files = get_files_by_eval_id(&conn, eval_id).await.unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, PathBuf::from("/path/to/devenv.nix"));
        assert_eq!(files[0].content_hash, "hash1");

        // Retrieve env inputs
        let envs = get_envs_by_eval_id(&conn, eval_id).await.unwrap();
        assert_eq!(envs.len(), 1);
        assert_eq!(envs[0].name, "HOME");
        assert_eq!(envs[0].content_hash, "hash2");
    }

    #[tokio::test]
    async fn test_eval_update_replaces_old() {
        let (db, _temp_dir) = setup_test_db().await;
        let conn = db.connect().await.unwrap();

        let key_hash = compute_string_hash("(import /foo/bar {}):config.packages");
        let attr_name = "config.packages";
        let modified_at = SystemTime::now();

        // First eval
        let inputs1 = vec![Input::File(FileInputDesc {
            path: "/path/to/file1.nix".into(),
            is_directory: false,
            content_hash: Some("old_hash".to_string()),
            modified_at,
        })];
        let input_hash1 = Input::compute_input_hash(&inputs1);
        let json_output1 = r#"["pkg1"]"#;

        insert_eval_with_inputs(
            &conn,
            &key_hash,
            attr_name,
            &input_hash1,
            json_output1,
            &inputs1,
        )
        .await
        .unwrap();

        // Second eval with same key but different inputs/output
        let inputs2 = vec![Input::File(FileInputDesc {
            path: "/path/to/file2.nix".into(),
            is_directory: false,
            content_hash: Some("new_hash".to_string()),
            modified_at,
        })];
        let input_hash2 = Input::compute_input_hash(&inputs2);
        let json_output2 = r#"["pkg1","pkg2"]"#;

        let (eval_id, _, _) = insert_eval_with_inputs(
            &conn,
            &key_hash,
            attr_name,
            &input_hash2,
            json_output2,
            &inputs2,
        )
        .await
        .unwrap();

        // Should have replaced the old eval
        let retrieved = get_eval_by_key_hash(&conn, &key_hash)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(retrieved.json_output, json_output2);
        assert_eq!(retrieved.input_hash, input_hash2);

        // Should have only the new file input
        let files = get_files_by_eval_id(&conn, eval_id).await.unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, PathBuf::from("/path/to/file2.nix"));
    }

    #[tokio::test]
    async fn test_eval_file_reuse_across_evals() {
        let (db, _temp_dir) = setup_test_db().await;
        let conn = db.connect().await.unwrap();

        let modified_at = SystemTime::now();

        // Shared file input
        let shared_file = Input::File(FileInputDesc {
            path: "/path/to/shared.nix".into(),
            is_directory: false,
            content_hash: Some("shared_hash".to_string()),
            modified_at,
        });

        // First eval
        let key1 = compute_string_hash("expr1:attr1");
        let inputs1 = vec![shared_file.clone()];
        let input_hash1 = Input::compute_input_hash(&inputs1);

        let (_, file_ids1, _) =
            insert_eval_with_inputs(&conn, &key1, "attr1", &input_hash1, "{}", &inputs1)
                .await
                .unwrap();

        // Second eval with same shared file
        let key2 = compute_string_hash("expr2:attr2");
        let inputs2 = vec![shared_file];
        let input_hash2 = Input::compute_input_hash(&inputs2);

        let (_, file_ids2, _) =
            insert_eval_with_inputs(&conn, &key2, "attr2", &input_hash2, "{}", &inputs2)
                .await
                .unwrap();

        // File should be reused (same ID)
        assert_eq!(file_ids1[0], file_ids2[0]);
    }
}
