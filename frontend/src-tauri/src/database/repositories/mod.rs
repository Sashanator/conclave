pub mod meeting;
pub mod meeting_notes;
pub mod setting;
pub mod speakers;
pub mod summary;
pub mod transcript;
pub mod transcript_chunk;

/// One connection, so every query in a test sees the same in-memory database.
#[cfg(test)]
pub(crate) async fn test_pool() -> sqlx::SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    pool
}
