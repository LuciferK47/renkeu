//! Migrations are reversible (CONTEXT.md §3) and the pre-M8 `.sql` files,
//! renamed to `.up.sql`, kept byte-identical so databases that already
//! applied them still validate (sqlx compares checksums on every start).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use sqlx::migrate::Migrator;

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

/// sha384 of each original migration, taken from the files at the M7 commit.
const ORIGINAL_CHECKSUMS: &[(i64, &str)] = &[
    (20_260_704_000_000, "ee05aa6832e10787b2297a02081248f9ddcfa5511f9d41faac4ac43164428368107bd2d6984e18cf3f77d2e3c1b51303"),
    (20_260_704_000_001, "61f707d5770420750998eacfc3bb2113ec1e813ee6927ec0a3ce02ec5ca5c3bde0559c5cd94c147232ad15bf9358c9b1"),
    (20_260_704_000_002, "5a2a628fddf28d37b7e11c10b71dfcc87a5e3c10e398ee8cef370760077b474451ce324db3588ec2532578258d9d3096"),
    (20_260_802_000_000, "1c6b8f1fdeb1e4a58102ee5944984e8e7823bdc7285d6d91c0b18a06a4fd6e0bcac73a77b8c9a62095c30c58e3262f6d"),
];

async fn pool() -> sqlx::SqlitePool {
    sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap()
}

async fn tables(pool: &sqlx::SqlitePool) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type='table' \
         AND name NOT LIKE 'sqlite_%' AND name != '_sqlx_migrations' ORDER BY name",
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

#[test]
fn original_migrations_keep_their_checksums() {
    for (version, expected) in ORIGINAL_CHECKSUMS {
        let m = MIGRATOR
            .iter()
            .find(|m| m.version == *version && m.migration_type.is_up_migration())
            .unwrap_or_else(|| panic!("migration {version} missing"));
        assert_eq!(hex(&m.checksum), *expected, "migration {version} was edited");
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

#[tokio::test]
async fn every_migration_has_a_down() {
    let ups = MIGRATOR.iter().filter(|m| m.migration_type.is_up_migration()).count();
    let downs = MIGRATOR.iter().filter(|m| m.migration_type.is_down_migration()).count();
    assert_eq!(ups, ORIGINAL_CHECKSUMS.len());
    assert_eq!(ups, downs, "each .up.sql needs a matching .down.sql");
}

#[tokio::test]
async fn up_down_up_round_trips() {
    let pool = pool().await;

    MIGRATOR.run(&pool).await.unwrap();
    let full = tables(&pool).await;
    assert_eq!(full, ["compile_errors", "compile_queue", "history"]);

    MIGRATOR.undo(&pool, 0).await.unwrap();
    assert!(tables(&pool).await.is_empty(), "down must remove everything");

    MIGRATOR.run(&pool).await.unwrap();
    assert_eq!(tables(&pool).await, full);
}

#[tokio::test]
async fn undoing_only_the_latest_restores_the_previous_shape() {
    let pool = pool().await;
    MIGRATOR.run(&pool).await.unwrap();

    MIGRATOR.undo(&pool, 20_260_704_000_002).await.unwrap();

    // Pre-realign compile_queue had `session_id`; the realigned one has `job_id`.
    let cols: Vec<String> = sqlx::query_scalar("SELECT name FROM pragma_table_info('compile_queue')")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert!(cols.contains(&"session_id".to_owned()), "{cols:?}");
    assert!(!cols.contains(&"job_id".to_owned()), "{cols:?}");

    MIGRATOR.run(&pool).await.unwrap();
}
