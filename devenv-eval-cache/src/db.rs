use crate::eval_inputs::{EnvInputDesc, FileInputDesc, Input};
use devenv_cache_core::{file::TrackedFile, time};
use sqlx::sqlite::{Sqlite, SqliteRow};
use sqlx::{Acquire, Row, SqlitePool};
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

// Create a constant for embedded migrations
pub const MIGRATIONS: sqlx::migrate::Migrator = sqlx::migrate!();

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
        let modified_at: i64 = row.get("modified_at");
        let updated_at: i64 = row.get("updated_at");
        Ok(Self {
            path: PathBuf::from(OsStr::from_bytes(path)),
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

impl sqlx::FromRow<'_, SqliteRow> for EnvInputRow {
    fn from_row(row: &SqliteRow) -> Result<Self, sqlx::Error> {
        let name: String = row.get("name");
        let content_hash: String = row.get("content_hash");
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
    pool: &SqlitePool,
    path: P,
    modified_at: SystemTime,
) -> Result<(), sqlx::Error> {
    let modified_at = time::system_time_to_unix_seconds(modified_at);
    let now = time::system_time_to_unix_seconds(SystemTime::now());

    sqlx::query(
        r#"
        UPDATE file_input
        SET modified_at = ?, updated_at = ?
        WHERE path = ?
        "#,
    )
    .bind(modified_at)
    .bind(now)
    .bind(path.as_ref().to_path_buf().into_os_string().as_bytes())
    .execute(pool)
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

impl sqlx::FromRow<'_, SqliteRow> for EvalRow {
    fn from_row(row: &SqliteRow) -> Result<Self, sqlx::Error> {
        let id: i64 = row.get("id");
        let key_hash: String = row.get("key_hash");
        let attr_name: String = row.get("attr_name");
        let input_hash: String = row.get("input_hash");
        let json_output: String = row.get("json_output");
        let updated_at: i64 = row.get("updated_at");
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
pub async fn get_eval_by_key_hash<'a, A>(
    conn: A,
    key_hash: &str,
) -> Result<Option<EvalRow>, sqlx::Error>
where
    A: Acquire<'a, Database = Sqlite>,
{
    let mut conn = conn.acquire().await?;

    let record = sqlx::query_as(
        r#"
            SELECT *
            FROM cached_eval
            WHERE key_hash = ?
        "#,
    )
    .bind(key_hash)
    .fetch_optional(&mut *conn)
    .await?;

    Ok(record)
}

/// Insert a cached eval with its inputs (files and env vars).
pub async fn insert_eval_with_inputs<'a, A>(
    conn: A,
    key_hash: &str,
    attr_name: &str,
    input_hash: &str,
    json_output: &str,
    inputs: &[Input],
) -> Result<(i64, Vec<i64>, Vec<i64>), sqlx::Error>
where
    A: Acquire<'a, Database = Sqlite>,
{
    let mut conn = conn.acquire().await?;
    let mut tx = conn.begin().await?;

    delete_eval(&mut tx, key_hash).await?;
    let eval_id = insert_eval(&mut tx, key_hash, attr_name, input_hash, json_output).await?;

    // Partition and extract file and env inputs
    let (file_inputs, env_inputs) = Input::partition_refs(inputs);

    let file_ids = insert_eval_file_inputs(&mut tx, &file_inputs, eval_id).await?;
    let env_ids = insert_eval_env_inputs(&mut tx, &env_inputs, eval_id).await?;

    tx.commit().await?;

    Ok((eval_id, file_ids, env_ids))
}

async fn insert_eval<'a, A>(
    conn: A,
    key_hash: &str,
    attr_name: &str,
    input_hash: &str,
    json_output: &str,
) -> Result<i64, sqlx::Error>
where
    A: Acquire<'a, Database = Sqlite>,
{
    let mut conn = conn.acquire().await?;

    let record = sqlx::query(
        r#"
        INSERT INTO cached_eval (key_hash, attr_name, input_hash, json_output)
        VALUES (?, ?, ?, ?)
        RETURNING id
        "#,
    )
    .bind(key_hash)
    .bind(attr_name)
    .bind(input_hash)
    .bind(json_output)
    .fetch_one(&mut *conn)
    .await?;

    let id: i64 = record.get(0);
    Ok(id)
}

async fn delete_eval<'a, A>(conn: A, key_hash: &str) -> Result<(), sqlx::Error>
where
    A: Acquire<'a, Database = Sqlite>,
{
    let mut conn = conn.acquire().await?;

    sqlx::query(
        r#"
        DELETE FROM cached_eval
        WHERE key_hash = ?
        "#,
    )
    .bind(key_hash)
    .execute(&mut *conn)
    .await?;

    Ok(())
}

/// Update the updated_at timestamp for a cached eval.
pub async fn update_eval_updated_at<'a, A>(conn: A, id: i64) -> Result<(), sqlx::Error>
where
    A: Acquire<'a, Database = Sqlite>,
{
    let mut conn = conn.acquire().await?;
    let now = time::system_time_to_unix_seconds(SystemTime::now());

    sqlx::query(
        r#"
        UPDATE cached_eval
        SET updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(now)
    .bind(id)
    .execute(&mut *conn)
    .await?;

    Ok(())
}

async fn insert_eval_file_inputs<'a, A>(
    conn: A,
    file_inputs: &[&FileInputDesc],
    eval_id: i64,
) -> Result<Vec<i64>, sqlx::Error>
where
    A: Acquire<'a, Database = Sqlite>,
{
    let mut conn = conn.acquire().await?;

    // Reuse the same file_input table as command caching
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
        let id: i64 = sqlx::query(insert_file_input)
            .bind(path.to_path_buf().into_os_string().as_bytes())
            .bind(is_directory)
            .bind(content_hash.as_ref().unwrap_or(&"".to_string()))
            .bind(modified_at)
            .bind(now)
            .fetch_one(&mut *conn)
            .await?
            .get(0);
        file_ids.push(id);
    }

    // Link to eval via eval_input_path table
    let eval_input_path_query = r#"
        INSERT INTO eval_input_path (cached_eval_id, file_input_id)
        VALUES (?, ?)
        ON CONFLICT (cached_eval_id, file_input_id) DO NOTHING
    "#;

    for &file_id in &file_ids {
        sqlx::query(eval_input_path_query)
            .bind(eval_id)
            .bind(file_id)
            .execute(&mut *conn)
            .await?;
    }
    Ok(file_ids)
}

async fn insert_eval_env_inputs<'a, A>(
    conn: A,
    env_inputs: &[&EnvInputDesc],
    eval_id: i64,
) -> Result<Vec<i64>, sqlx::Error>
where
    A: Acquire<'a, Database = Sqlite>,
{
    let mut conn = conn.acquire().await?;

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
        let id: i64 = sqlx::query(insert_env_input)
            .bind(eval_id)
            .bind(name)
            .bind(content_hash.as_ref().unwrap_or(&"".to_string()))
            .bind(now)
            .fetch_one(&mut *conn)
            .await?
            .get(0);
        env_input_ids.push(id);
    }

    Ok(env_input_ids)
}

/// Get file inputs for a cached eval by eval ID.
pub async fn get_files_by_eval_id(
    pool: &SqlitePool,
    eval_id: i64,
) -> Result<Vec<FileInputRow>, sqlx::Error> {
    let files = sqlx::query_as(
        r#"
            SELECT f.path, f.is_directory, f.content_hash, f.modified_at, f.updated_at
            FROM file_input f
            JOIN eval_input_path eip ON f.id = eip.file_input_id
            WHERE eip.cached_eval_id = ?
        "#,
    )
    .bind(eval_id)
    .fetch_all(pool)
    .await?;

    Ok(files)
}

/// Get env inputs for a cached eval by eval ID.
pub async fn get_envs_by_eval_id(
    pool: &SqlitePool,
    eval_id: i64,
) -> Result<Vec<EnvInputRow>, sqlx::Error> {
    let envs = sqlx::query_as(
        r#"
            SELECT e.name, e.content_hash
            FROM eval_env_input e
            WHERE e.cached_eval_id = ?
        "#,
    )
    .bind(eval_id)
    .fetch_all(pool)
    .await?;

    Ok(envs)
}

#[cfg(test)]
mod tests {
    use devenv_cache_core::compute_string_hash;

    use super::*;
    use sqlx::SqlitePool;

    #[sqlx::test]
    async fn test_insert_and_retrieve_eval(pool: SqlitePool) {
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
            &pool,
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
        let retrieved_eval = get_eval_by_key_hash(&pool, &key_hash)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(retrieved_eval.key_hash, key_hash);
        assert_eq!(retrieved_eval.attr_name, attr_name);
        assert_eq!(retrieved_eval.json_output, json_output);
        assert_eq!(retrieved_eval.input_hash, input_hash);

        // Retrieve file inputs
        let files = get_files_by_eval_id(&pool, eval_id).await.unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, PathBuf::from("/path/to/devenv.nix"));
        assert_eq!(files[0].content_hash, "hash1");

        // Retrieve env inputs
        let envs = get_envs_by_eval_id(&pool, eval_id).await.unwrap();
        assert_eq!(envs.len(), 1);
        assert_eq!(envs[0].name, "HOME");
        assert_eq!(envs[0].content_hash, "hash2");
    }

    #[sqlx::test]
    async fn test_eval_update_replaces_old(pool: SqlitePool) {
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
            &pool,
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
            &pool,
            &key_hash,
            attr_name,
            &input_hash2,
            json_output2,
            &inputs2,
        )
        .await
        .unwrap();

        // Should have replaced the old eval
        let retrieved = get_eval_by_key_hash(&pool, &key_hash)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(retrieved.json_output, json_output2);
        assert_eq!(retrieved.input_hash, input_hash2);

        // Should have only the new file input
        let files = get_files_by_eval_id(&pool, eval_id).await.unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, PathBuf::from("/path/to/file2.nix"));
    }

    #[sqlx::test]
    async fn test_eval_file_reuse_across_evals(pool: SqlitePool) {
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
            insert_eval_with_inputs(&pool, &key1, "attr1", &input_hash1, "{}", &inputs1)
                .await
                .unwrap();

        // Second eval with same shared file
        let key2 = compute_string_hash("expr2:attr2");
        let inputs2 = vec![shared_file];
        let input_hash2 = Input::compute_input_hash(&inputs2);

        let (_, file_ids2, _) =
            insert_eval_with_inputs(&pool, &key2, "attr2", &input_hash2, "{}", &inputs2)
                .await
                .unwrap();

        // File should be reused (same ID)
        assert_eq!(file_ids1[0], file_ids2[0]);
    }
}
