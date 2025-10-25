//! Reproduction case for PRAGMA foreign_keys bug in turso 0.2.2
//!
//! This example demonstrates that PRAGMA foreign_keys doesn't work properly in turso 0.2.2,
//! which means CASCADE DELETE and other foreign key constraints are not enforced.
//!
//! ## Expected behavior:
//! - PRAGMA foreign_keys = ON should enable foreign key constraints
//! - Deleting a parent record should cascade delete child records
//!
//! ## Actual behavior with turso 0.2.2:
//! - PRAGMA foreign_keys commands may succeed but don't actually enable constraints
//! - Child records remain after parent deletion (CASCADE DELETE doesn't work)
//!
//! ## Related issues:
//! - https://github.com/libsql/sqld/issues/764
//! - https://github.com/tursodatabase/libsql/issues/1119
//!
//! ## Run this example:
//! ```bash
//! cargo run --example pragma_foreign_keys
//! ```

use turso::{Builder, params};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== PRAGMA foreign_keys Bug Reproduction ===\n");

    // Create a temporary database
    let temp_dir = tempfile::TempDir::new()?;
    let db_path = temp_dir.path().join("test.db");
    let db_path_str = db_path.to_str().unwrap();

    println!("Creating database at: {}", db_path_str);

    // Create database
    let db = Builder::new_local(db_path_str).build().await?;
    let conn = db.connect()?;

    // Step 1: Create tables with foreign key constraints
    println!("\n1. Creating tables with FOREIGN KEY ... ON DELETE CASCADE");
    conn.execute(
        r#"
        CREATE TABLE parent (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL
        )
        "#,
        (),
    )
    .await?;

    conn.execute(
        r#"
        CREATE TABLE child (
            id INTEGER PRIMARY KEY,
            parent_id INTEGER NOT NULL,
            data TEXT,
            FOREIGN KEY (parent_id) REFERENCES parent(id) ON DELETE CASCADE
        )
        "#,
        (),
    )
    .await?;
    println!("   ✓ Tables created");

    // Step 2: Try to enable foreign keys with different syntaxes
    println!("\n2. Attempting to enable foreign keys with PRAGMA");

    let pragma_variants = vec![
        "PRAGMA foreign_keys = ON",
        "PRAGMA foreign_keys=ON",
        "PRAGMA foreign_keys = 1",
        "PRAGMA foreign_keys=1",
        "PRAGMA foreign_keys = true",
        "PRAGMA foreign_keys=true",
        "PRAGMA foreign_keys = ON;",
        "PRAGMA foreign_keys=ON;",
        "PRAGMA foreign_keys = 1;",
        "PRAGMA foreign_keys=1;",
        "PRAGMA foreign_keys = true;",
        "PRAGMA foreign_keys=true;",
    ];

    let mut pragma_succeeded = false;
    for pragma in &pragma_variants {
        match conn.execute(pragma, ()).await {
            Ok(_) => {
                println!("   ✓ '{}' executed successfully", pragma);
                pragma_succeeded = true;
                break;
            }
            Err(e) => {
                println!("   ✗ '{}' failed: {}", pragma, e);
            }
        }
    }

    if !pragma_succeeded {
        println!("   ⚠ All PRAGMA attempts failed!");
    }

    // Step 3: Check if foreign keys are actually enabled
    println!("\n3. Checking if foreign keys are enabled");
    match conn.query("PRAGMA foreign_keys", ()).await {
        Ok(mut rows) => {
            if let Some(row) = rows.next().await? {
                let enabled: i64 = row.get(0)?;
                if enabled == 1 {
                    println!("   ✓ PRAGMA foreign_keys = {} (enabled)", enabled);
                } else {
                    println!("   ✗ PRAGMA foreign_keys = {} (disabled)", enabled);
                }
            } else {
                println!("   ⚠ No result from PRAGMA foreign_keys query");
            }
        }
        Err(e) => {
            println!("   ✗ Failed to query PRAGMA foreign_keys: {}", e);
        }
    }

    // Step 4: Test CASCADE DELETE behavior
    println!("\n4. Testing CASCADE DELETE behavior");

    // Insert parent
    conn.execute(
        "INSERT INTO parent (id, name) VALUES (?, ?)",
        params![1, "test_parent"],
    )
    .await?;
    println!("   ✓ Inserted parent record (id=1)");

    // Insert child
    conn.execute(
        "INSERT INTO child (id, parent_id, data) VALUES (?, ?, ?)",
        params![1, 1, "test_child"],
    )
    .await?;
    println!("   ✓ Inserted child record (id=1, parent_id=1)");

    // Verify child exists
    let mut stmt = conn
        .prepare("SELECT COUNT(*) FROM child WHERE parent_id = ?")
        .await?;
    let mut rows = stmt.query(params![1]).await?;
    let row = rows.next().await?.unwrap();
    let count_before: i64 = row.get(0)?;
    println!("   ✓ Child records before deletion: {}", count_before);

    // Delete parent
    conn.execute("DELETE FROM parent WHERE id = ?", params![1])
        .await?;
    println!("   ✓ Deleted parent record (id=1)");

    // Check if child was cascade deleted
    let mut stmt = conn
        .prepare("SELECT COUNT(*) FROM child WHERE parent_id = ?")
        .await?;
    let mut rows = stmt.query(params![1]).await?;
    let row = rows.next().await?.unwrap();
    let count_after: i64 = row.get(0)?;

    println!("\n5. Result:");
    println!("   Child records after parent deletion: {}", count_after);

    if count_after == 0 {
        println!("   ✓ CASCADE DELETE worked correctly!");
        println!("\n=== PASS: Foreign keys are working ===");
    } else {
        println!("   ✗ CASCADE DELETE did NOT work!");
        println!("   Expected: 0 child records");
        println!("   Actual: {} child record(s)", count_after);
        println!("\n=== FAIL: Foreign keys are NOT working ===");
        println!("\nThis is the bug in turso 0.2.2:");
        println!("- PRAGMA foreign_keys commands may execute without error");
        println!("- But foreign key constraints are NOT actually enforced");
        println!("- CASCADE DELETE does not work as expected");
        std::process::exit(1);
    }

    Ok(())
}
