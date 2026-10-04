mod db;
mod engine;
mod error;
mod handlers;
mod models;

use axum::{http::StatusCode, routing::get, Router};
use sqlx::SqlitePool;
use std::{env, net::SocketAddr, path::PathBuf, sync::Arc};
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,sqlx=warn".into()),
        )
        .init();

    let db_path = env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:data/studio.db?mode=rwc".into());
    let pool = db::connect(&db_path).await?;
    db::migrate(&pool).await?;
    seed::run(&pool).await?;

    let state = AppState { pool };

    let dist_dir = env::var("FRONTEND_DIST")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("frontend/dist"));
    tracing::info!("前端静态目录: {}", dist_dir.display());
    let index = dist_dir.join("index.html");

    let api = handlers::router();

    let app = Router::new()
        .nest("/api", api)
        .route("/healthz", get(|| async { (StatusCode::OK, "ok") }))
        .fallback_service(
            // 静态资源；找不到文件时回退 index.html（Svelte SPA 路由）
            ServeDir::new(&dist_dir).fallback(ServeFile::new(index)),
        )
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let port: u16 = env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8080);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("摄影棚灯光排期系统启动: http://localhost:{port}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

// 让 Arc 在某些编译配置下不报未使用
#[allow(dead_code)]
fn _arc_marker(_: Arc<()>) {}

mod seed {
    use sqlx::SqlitePool;

    pub async fn run(pool: &SqlitePool) -> Result<(), sqlx::Error> {
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users").fetch_one(pool).await?;
        if count.0 > 0 {
            return Ok(());
        }
        tracing::info!("数据库为空，写入演示种子数据…");

        // 用户：摄影师 x2、场务、管理员
        sqlx::query("INSERT INTO users (id,name,role) VALUES
            (1,'张伟（摄影师）','photographer'),
            (2,'李娜（摄影师）','photographer'),
            (3,'王磊（场务）','crew'),
            (4,'系统管理员','admin')")
            .execute(pool).await?;

        // 场地 A：12m x 9m
        sqlx::query("INSERT INTO studios (id,name,width_m,height_m) VALUES
            (1,'A 号摄影棚',12,9),(2,'B 号摄影棚',10,7)")
            .execute(pool).await?;

        // A 棚回路（功率上限 W）
        sqlx::query("INSERT INTO circuits (id,studio_id,name,max_power_w) VALUES
            (1,1,'A-回路1（墙插区）',2400),
            (2,1,'A-回路2（天幕区）',3000),
            (3,1,'A-回路3（重型专线）',6000),
            (4,2,'B-回路1',2000),
            (5,2,'B-回路2',3500)")
            .execute(pool).await?;

        // A 棚灯具
        sqlx::query("INSERT INTO fixtures (id,studio_id,name,model,power_w,default_circuit_id,status) VALUES
            (1,1,'镝灯 HMI-1200 #1','ARRI M18',1800,3,'ok'),
            (2,1,'镝灯 HMI-1200 #2','ARRI M18',1800,3,'ok'),
            (3,1,'LED 平板 #1','ARRI SkyPanel S60',450,1,'ok'),
            (4,1,'LED 平板 #2','ARRI SkyPanel S60',450,1,'ok'),
            (5,1,'LED 聚光灯 #1','Aputure 600d',720,2,'ok'),
            (6,1,'LED 聚光灯 #2','Aputure 600d',720,2,'ok'),
            (7,1,'钨丝灯 2K','Mole 2000W',2000,3,'repair'),
            (8,1,'闪光灯组','Profoto Pro-11',2400,2,'ok'),
            (9,2,'B 棚 LED 平板','LiteGear 300',300,4,'ok'),
            (10,2,'B 棚镝灯','HMI 800',800,5,'ok'),
            (11,2,'B 棚钨丝灯','1K Tungsten',1000,5,'ok')")
            .execute(pool).await?;

        tracing::info!("种子数据完成：用户4 / 场地2 / 回路5 / 灯具11（含1盏维修中）");
        Ok(())
    }
}
