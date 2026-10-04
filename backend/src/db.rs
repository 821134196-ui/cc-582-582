use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::{str::FromStr, time::Duration};

pub type BoxError = Box<dyn std::error::Error + Send + Sync>;

pub async fn connect(db_url: &str) -> Result<SqlitePool, BoxError> {
    let opts = SqliteConnectOptions::from_str(db_url)?
        .create_if_missing(true)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(10));

    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .connect_with(opts)
        .await?;
    Ok(pool)
}

/// 启动时执行内置迁移。
/// 触发器体内含 ';'，因此按词扫描，仅在 BEGIN…END 块外按 ';' 切分语句。
pub async fn migrate(pool: &SqlitePool) -> Result<(), BoxError> {
    let sql = include_str!("../migrations/001_init.sql");
    let mut conn = pool.acquire().await?;

    let mut stmt = String::new();
    let mut depth: i32 = 0;
    let mut word = String::new();

    for ch in sql.chars() {
        if ch.is_alphanumeric() || ch == '_' {
            word.push(ch.to_ascii_uppercase());
            stmt.push(ch);
        } else {
            if !word.is_empty() {
                match word.as_str() {
                    // 本迁移中 BEGIN…END 与 CASE…END 配对出现，统一计深度，
                    // 使触发器体内 SELECT 语句的 ';' 不会切断 CREATE TRIGGER
                    "BEGIN" | "CASE" => depth += 1,
                    "END" => depth -= 1,
                    _ => {}
                }
                word.clear();
            }
            if ch == ';' && depth == 0 {
                let s = stmt.trim();
                if !s.is_empty() {
                    sqlx::query(s).execute(&mut *conn).await?;
                }
                stmt.clear();
            } else {
                stmt.push(ch);
            }
        }
    }
    let tail = stmt.trim();
    if !tail.is_empty() {
        sqlx::query(tail).execute(&mut *conn).await?;
    }
    Ok(())
}
