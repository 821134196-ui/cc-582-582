use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use chrono::DateTime;
use serde::Deserialize;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};
use std::collections::HashMap;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

mod types;
use types::*;

type Db = SqlitePool;

#[derive(Clone)]
struct AppState {
    db: Db,
}

#[derive(Debug)]
struct AppError {
    status: StatusCode,
    message: String,
}

impl AppError {
    fn bad(msg: impl Into<String>) -> Self {
        Self { status: StatusCode::BAD_REQUEST, message: msg.into() }
    }
    fn conflict(msg: impl Into<String>) -> Self {
        Self { status: StatusCode::CONFLICT, message: msg.into() }
    }
    fn forbidden(msg: impl Into<String>) -> Self {
        Self { status: StatusCode::FORBIDDEN, message: msg.into() }
    }
    fn not_found(msg: impl Into<String>) -> Self {
        Self { status: StatusCode::NOT_FOUND, message: msg.into() }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (self.status, Json(serde_json::json!({ "error": self.message }))).into_response()
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        tracing::error!("db error: {e}");
        AppError { status: StatusCode::INTERNAL_SERVER_ERROR, message: "数据库错误".into() }
    }
}

// ---------- 鉴权辅助 ----------

async fn current_user(headers: &HeaderMap, db: &Db) -> Result<User, AppError> {
    let uid = headers
        .get("x-user-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<i64>().ok())
        .ok_or_else(|| AppError::bad("缺少 X-User-Id 请求头"))?;
    sqlx::query_as::<_, User>("SELECT id, name, role FROM users WHERE id = ?")
        .bind(uid)
        .fetch_optional(db)
        .await?
        .ok_or_else(|| AppError::bad("用户不存在"))
}

fn require_role(user: &User, role: &str) -> Result<(), AppError> {
    if user.role != role {
        return Err(AppError::forbidden(format!("需要 {} 角色", role_zh(role))));
    }
    Ok(())
}

fn role_zh(role: &str) -> String {
    match role {
        "photographer" => "摄影师".to_string(),
        "crew" => "场务".to_string(),
        _ => role.to_string(),
    }
}

async fn notify(db: &Db, user_id: Option<i64>, kind: &str, message: &str) {
    if let Err(e) = sqlx::query("INSERT INTO notifications(user_id, kind, message) VALUES(?1,?2,?3)")
        .bind(user_id)
        .bind(kind)
        .bind(message)
        .execute(db)
        .await
    {
        tracing::warn!("notify failed: {e}");
    }
}

/// 解析 RFC3339 并归一化为 UTC 文本，保证 SQLite 字符串比较与时间序一致
fn norm_time(s: &str) -> Result<String, AppError> {
    let dt = DateTime::parse_from_rfc3339(s)
        .map_err(|_| AppError::bad(format!("时间格式需为 RFC3339：{s}")))?;
    Ok(dt.with_timezone(&chrono::Utc).format("%Y-%m-%dT%H:%M:%SZ").to_string())
}

// ---------- 占用检查核心逻辑（确认/预检共用） ----------

struct ConflictReport {
    venue_busy: Option<BusyProject>,
    fixture_busy: Vec<FixtureBusy>,
    circuit_loads: Vec<CircuitLoad>,
    errors: Vec<String>,
}

/// 在给定连接上扫描资源冲突。调用方必须已持有写事务（BEGIN IMMEDIATE）。
async fn scan_conflicts(
    conn: &mut sqlx::SqliteConnection,
    venue_id: i64,
    start: &str,
    end: &str,
    lights: &[LightInput],
    exclude_project: i64,
) -> Result<ConflictReport, AppError> {
    let mut errors = Vec::new();

    // 时间序
    if start >= end {
        errors.push("结束时间必须晚于开始时间".into());
    }

    // 基础数据
    let venue = sqlx::query_as::<_, Venue>("SELECT id, name, width_m, height_m, active FROM venues WHERE id=?")
        .bind(venue_id)
        .fetch_optional(&mut *conn)
        .await?;
    let venue = match venue {
        Some(v) if v.active != 0 => v,
        _ => {
            errors.push("场地不存在或已停用".into());
            return Ok(ConflictReport { venue_busy: None, fixture_busy: vec![], circuit_loads: vec![], errors });
        }
    };

    let circuits = sqlx::query_as::<_, Circuit>("SELECT id, venue_id, name, max_watts, active FROM circuits WHERE venue_id=?")
        .bind(venue_id)
        .fetch_all(&mut *conn)
        .await?;
    let circ_map: HashMap<i64, &Circuit> = circuits.iter().map(|c| (c.id, c)).collect();

    let mut fixture_ids: Vec<i64> = Vec::new();
    for (i, l) in lights.iter().enumerate() {
        match l.fixture_id {
            Some(fid) => {
                let f = sqlx::query_as::<_, Fixture>("SELECT id, code, model, power_watts, active FROM fixtures WHERE id=?")
                    .bind(fid)
                    .fetch_optional(&mut *conn)
                    .await?;
                match f {
                    Some(f) if f.active != 0 => {
                        if l.watts <= 0 {
                            errors.push(format!("第 {} 盏灯功率必须为正", i + 1));
                        }
                        fixture_ids.push(fid);
                    }
                    Some(_) => errors.push(format!("第 {} 盏灯引用的灯具已停用", i + 1)),
                    None => errors.push(format!("第 {} 盏灯引用的灯具不存在", i + 1)),
                }
            }
            None => errors.push(format!("第 {} 盏灯尚未指定实物灯具，无法确认", i + 1)),
        }
        match circ_map.get(&l.circuit_id) {
            Some(c) if c.active != 0 => {}
            _ => errors.push(format!("第 {} 盏灯所选回路不属于该场地", i + 1)),
        }
        if l.pos_x < 0.0 || l.pos_y < 0.0 || l.pos_x > venue.width_m || l.pos_y > venue.height_m {
            errors.push(format!("第 {} 盏灯超出平面图范围", i + 1));
        }
    }
    // 同一方案内灯具重复
    let mut seen = std::collections::HashSet::new();
    for fid in &fixture_ids {
        if !seen.insert(*fid) {
            errors.push(format!("同一方案内灯具 #{fid} 被重复使用"));
        }
    }

    // 场地重叠
    let venue_busy = sqlx::query(
        "SELECT p.id, p.name FROM venue_slots vs JOIN projects p ON p.id=vs.project_id
         WHERE vs.venue_id=?1 AND vs.start_at < ?3 AND vs.end_at > ?2 AND vs.project_id != ?4 LIMIT 1",
    )
    .bind(venue_id).bind(start).bind(end).bind(exclude_project)
    .fetch_optional(&mut *conn)
    .await?
    .map(|r| BusyProject { id: r.get::<i64,_>(0), name: r.get::<String,_>(1) });

    // 灯具重叠
    let mut fixture_busy = Vec::new();
    for fid in &fixture_ids {
        let rows = sqlx::query(
            "SELECT p.id, p.name, fs.start_at, fs.end_at FROM fixture_slots fs
             JOIN projects p ON p.id=fs.project_id
             WHERE fs.fixture_id=?1 AND fs.start_at < ?3 AND fs.end_at > ?2 AND fs.project_id != ?4",
        )
        .bind(fid).bind(start).bind(end).bind(exclude_project)
        .fetch_all(&mut *conn)
        .await?;
        for r in rows {
            fixture_busy.push(FixtureBusy {
                fixture_id: *fid,
                project: BusyProject { id: r.get::<i64,_>(0), name: r.get::<String,_>(1) },
                start: r.get::<String,_>(2),
                end: r.get::<String,_>(3),
            });
        }
    }

    // 回路负载：本方案新增 + 时段重叠的已占用灯位
    let mut added: HashMap<i64, i64> = HashMap::new();
    for l in lights {
        *added.entry(l.circuit_id).or_insert(0) += l.watts as i64;
    }
    let rows = sqlx::query(
        "SELECT bl.circuit_id, COALESCE(SUM(bl.watts),0)
         FROM booked_lights bl
         JOIN venue_slots vs ON vs.project_id = bl.project_id
         WHERE vs.venue_id=?1 AND vs.start_at < ?3 AND vs.end_at > ?2 AND bl.project_id != ?4
           AND NOT EXISTS (SELECT 1 FROM returns r WHERE r.project_id=bl.project_id AND r.fixture_id=bl.fixture_id)
         GROUP BY bl.circuit_id",
    )
    .bind(venue_id).bind(start).bind(end).bind(exclude_project)
    .fetch_all(&mut *conn)
    .await?;
    let mut used_map: HashMap<i64, i64> = HashMap::new();
    for r in rows {
        used_map.insert(r.get::<i64,_>(0), r.get::<i64,_>(1));
    }

    let mut circuit_loads = Vec::new();
    for c in &circuits {
        let a = *added.get(&c.id).unwrap_or(&0);
        let u = *used_map.get(&c.id).unwrap_or(&0);
        if a == 0 { continue; }
        let total = a + u;
        if total > c.max_watts as i64 {
            errors.push(format!(
                "回路 {} 功率超限：已有 {}W + 本方案 {}W = {}W，上限 {}W（不可手工放行）",
                c.name, u, a, total, c.max_watts
            ));
        }
        circuit_loads.push(CircuitLoad {
            circuit_id: c.id,
            circuit_name: c.name.clone(),
            used_watts: u,
            added_watts: a,
            total_watts: total,
            max_watts: c.max_watts,
            overload: total > c.max_watts as i64,
        });
    }

    if venue_busy.is_some() {
        errors.push("场地在该时段已被其他项目占用".into());
    }
    for fb in &fixture_busy {
        errors.push(format!("灯具 #{} 与项目「{}」时段冲突", fb.fixture_id, fb.project.name));
    }

    Ok(ConflictReport { venue_busy, fixture_busy, circuit_loads, errors })
}

// ---------- 占用写入 / 释放 ----------

async fn write_holds(
    conn: &mut sqlx::SqliteConnection,
    project_id: i64,
    version_id: i64,
    venue_id: i64,
    start: &str,
    end: &str,
    lights: &[LightInput],
) -> Result<(), AppError> {
    sqlx::query("INSERT INTO venue_slots(project_id, venue_id, start_at, end_at) VALUES(?1,?2,?3,?4)")
        .bind(project_id).bind(venue_id).bind(start).bind(end)
        .execute(&mut *conn).await?;
    for (i, l) in lights.iter().enumerate() {
        sqlx::query(
            "INSERT INTO fixture_slots(project_id, fixture_id, start_at, end_at) VALUES(?1,?2,?3,?4)",
        )
        .bind(project_id).bind(l.fixture_id.unwrap()).bind(start).bind(end)
        .execute(&mut *conn).await?;
        sqlx::query(
            "INSERT INTO booked_lights(project_id, plan_version_id, line_no, fixture_id, circuit_id, watts, pos_x, pos_y, label)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        )
        .bind(project_id).bind(version_id).bind(i as i64).bind(l.fixture_id)
        .bind(l.circuit_id).bind(l.watts).bind(l.pos_x).bind(l.pos_y)
        .bind(l.label.clone().unwrap_or_default())
        .execute(&mut *conn).await?;
    }
    Ok(())
}

async fn release_holds(conn: &mut sqlx::SqliteConnection, project_id: i64) -> Result<(), AppError> {
    sqlx::query("DELETE FROM fixture_slots WHERE project_id=?").bind(project_id).execute(&mut *conn).await?;
    sqlx::query("DELETE FROM venue_slots WHERE project_id=?").bind(project_id).execute(&mut *conn).await?;
    sqlx::query("DELETE FROM booked_lights WHERE project_id=?").bind(project_id).execute(&mut *conn).await?;
    Ok(())
}

// ---------- DTO ----------

#[derive(Deserialize)]
struct CreateProjectReq {
    name: String,
    venue_id: i64,
    start: String,
    end: String,
}

#[derive(Deserialize)]
struct SavePlanReq {
    note: Option<String>,
    lights: Vec<LightInput>,
}

#[derive(Deserialize)]
struct UpdateHeaderReq {
    name: Option<String>,
    venue_id: Option<i64>,
    start: Option<String>,
    end: Option<String>,
}

#[derive(Deserialize)]
struct ReplanReq {
    venue_id: i64,
    start: String,
    end: String,
    note: Option<String>,
    lights: Vec<LightInput>,
}

#[derive(Deserialize)]
struct ConfirmReq {
    remark: Option<String>,
    /// 仅用于证明：即使前端强行传 force=true，服务端也拒绝超限放行
    force: Option<bool>,
}

#[derive(Deserialize)]
struct RejectReq {
    remark: Option<String>,
}

#[derive(Deserialize)]
struct CheckReq {
    venue_id: i64,
    start: String,
    end: String,
    lights: Vec<LightInput>,
}

#[derive(Deserialize)]
struct ReturnItemsReq {
    items: Vec<ReturnItem>,
}

#[derive(Deserialize)]
struct DecideReq {
    waiver: String, // approved | denied
}

// ---------- 项目处理 ----------

async fn fetch_project(db: &Db, id: i64) -> Result<ProjectDetail, AppError> {
    let p = sqlx::query_as::<_, Project>(
        "SELECT id, name, photographer_id, status, venue_id, shoot_start, shoot_end, created_at, updated_at
         FROM projects WHERE id=?",
    )
    .bind(id)
    .fetch_optional(db)
    .await?
    .ok_or_else(|| AppError::not_found("项目不存在"))?;

    let photographer = sqlx::query_as::<_, User>("SELECT id,name,role FROM users WHERE id=?")
        .bind(p.photographer_id).fetch_one(db).await?;
    let versions = sqlx::query_as::<_, PlanVersion>(
        "SELECT id, project_id, version, note, created_at, lights_json FROM plan_versions
         WHERE project_id=? ORDER BY version DESC",
    )
    .bind(id).fetch_all(db).await?;
    let confirmations = sqlx::query_as::<_, ConfirmationRec>(
        "SELECT c.id, c.project_id, c.plan_version_id, c.crew_id, u.name AS crew_name, c.result, c.remark, c.created_at
         FROM confirmations c JOIN users u ON u.id=c.crew_id WHERE c.project_id=? ORDER BY c.id DESC",
    )
    .bind(id).fetch_all(db).await?;
    let booked = sqlx::query_as::<_, BookedLightRec>(
        "SELECT id, project_id, plan_version_id, line_no, fixture_id, circuit_id, watts, pos_x, pos_y, label
         FROM booked_lights WHERE project_id=? ORDER BY line_no",
    )
    .bind(id).fetch_all(db).await?;
    let returns = sqlx::query_as::<_, ReturnRec>(
        "SELECT r.id, r.project_id, r.fixture_id, f.code AS fixture_code, r.condition, r.note,
                r.returned_by, u.name AS returned_by_name, r.created_at
         FROM returns r JOIN fixtures f ON f.id=r.fixture_id JOIN users u ON u.id=r.returned_by
         WHERE r.project_id=? ORDER BY r.id",
    )
    .bind(id).fetch_all(db).await?;
    let damages = sqlx::query_as::<_, DamageRec>(
        "SELECT d.id, d.project_id, d.fixture_id, f.code AS fixture_code, d.description, d.waiver,
                d.decided_by, du.name AS decided_by_name, d.decided_at, d.created_at
         FROM damages d JOIN fixtures f ON f.id=d.fixture_id
         LEFT JOIN users du ON du.id=d.decided_by WHERE d.project_id=? ORDER BY d.id",
    )
    .bind(id).fetch_all(db).await?;
    let venue_slot = sqlx::query_as::<_, Slot>(
        "SELECT id, project_id, venue_id, start_at, end_at FROM venue_slots WHERE project_id=?",
    )
    .bind(id).fetch_optional(db).await?;
    let fixture_slots = sqlx::query_as::<_, FixtureSlotRec>(
        "SELECT id, project_id, fixture_id, start_at, end_at FROM fixture_slots WHERE project_id=?",
    )
    .bind(id).fetch_all(db).await?;

    Ok(ProjectDetail {
        p,
        photographer,
        versions,
        confirmations,
        booked,
        returns,
        damages,
        venue_slot,
        fixture_slots,
    })
}

async fn create_project(
    State(st): State<AppState>, headers: HeaderMap, Json(req): Json<CreateProjectReq>,
) -> Result<Json<serde_json::Value>, AppError> {
    let user = current_user(&headers, &st.db).await?;
    require_role(&user, "photographer")?;
    if req.name.trim().is_empty() {
        return Err(AppError::bad("项目名称不能为空"));
    }
    let start = norm_time(&req.start)?;
    let end = norm_time(&req.end)?;
    if start >= end {
        return Err(AppError::bad("结束时间必须晚于开始时间"));
    }
    let id = sqlx::query(
        "INSERT INTO projects(name, photographer_id, venue_id, shoot_start, shoot_end)
         VALUES(?1,?2,?3,?4,?5)",
    )
    .bind(&req.name).bind(user.id).bind(req.venue_id).bind(&start).bind(&end)
    .execute(&st.db).await?.last_insert_rowid();
    notify(&st.db, None, "project", &format!("摄影师 {} 创建了项目「{}」", user.name, req.name)).await;
    Ok(Json(serde_json::json!({ "id": id })))
}

async fn list_projects(State(st): State<AppState>) -> Result<Json<Vec<ProjectSummary>>, AppError> {
    let rows = sqlx::query_as::<_, ProjectSummary>(
        "SELECT p.id, p.name, p.photographer_id, u.name AS photographer_name, p.status,
                p.venue_id, v.name AS venue_name, p.shoot_start, p.shoot_end,
                (SELECT MAX(version) FROM plan_versions pv WHERE pv.project_id=p.id) AS version,
                (SELECT result FROM confirmations c WHERE c.project_id=p.id ORDER BY c.id DESC LIMIT 1) AS last_result
         FROM projects p JOIN users u ON u.id=p.photographer_id
         LEFT JOIN venues v ON v.id=p.venue_id
         ORDER BY p.id DESC",
    ).fetch_all(&st.db).await?;
    Ok(Json(rows))
}

async fn get_project(State(st): State<AppState>, Path(id): Path<i64>) -> Result<Json<ProjectDetail>, AppError> {
    Ok(Json(fetch_project(&st.db, id).await?))
}

async fn update_header(
    State(st): State<AppState>, Path(id): Path<i64>, headers: HeaderMap, Json(req): Json<UpdateHeaderReq>,
) -> Result<Json<serde_json::Value>, AppError> {
    let user = current_user(&headers, &st.db).await?;
    let p = project_or_404(&st.db, id).await?;
    if p.photographer_id != user.id {
        return Err(AppError::forbidden("只能修改自己的项目"));
    }
    if !matches!(p.status.as_str(), "draft" | "submitted") {
        return Err(AppError::conflict("当前状态不能直接修改，请使用「变更方案」流程"));
    }
    if let Some(s) = &req.start { norm_time(s)?; }
    if let Some(e) = &req.end { norm_time(e)?; }
    sqlx::query(
        "UPDATE projects SET name=COALESCE(?2,name), venue_id=COALESCE(?3,venue_id),
         shoot_start=COALESCE(?4,shoot_start), shoot_end=COALESCE(?5,shoot_end), updated_at=datetime('now')
         WHERE id=?1",
    )
    .bind(id).bind(req.name).bind(req.venue_id).bind(req.start).bind(req.end)
    .execute(&st.db).await?;
    Ok(Json(serde_json::json!({"ok": true})))
}

async fn project_or_404(db: &Db, id: i64) -> Result<Project, AppError> {
    sqlx::query_as::<_, Project>(
        "SELECT id, name, photographer_id, status, venue_id, shoot_start, shoot_end, created_at, updated_at
         FROM projects WHERE id=?",
    ).bind(id).fetch_optional(db).await?.ok_or_else(|| AppError::not_found("项目不存在"))
}

async fn save_version(
    State(st): State<AppState>, Path(id): Path<i64>, headers: HeaderMap, Json(req): Json<SavePlanReq>,
) -> Result<Json<serde_json::Value>, AppError> {
    let user = current_user(&headers, &st.db).await?;
    let p = project_or_404(&st.db, id).await?;
    if p.photographer_id != user.id {
        return Err(AppError::forbidden("只能修改自己的项目"));
    }
    if !matches!(p.status.as_str(), "draft" | "submitted") {
        return Err(AppError::conflict("当前状态不能编辑方案"));
    }
    if req.lights.is_empty() {
        return Err(AppError::bad("方案至少需要一盏灯"));
    }
    let lights_json = serde_json::to_string(&req.lights).unwrap();
    let mut conn = st.db.acquire().await?;
    sqlx::query("BEGIN IMMEDIATE").execute(&mut *conn).await?;
    let next: i64 = sqlx::query("SELECT COALESCE(MAX(version),0)+1 FROM plan_versions WHERE project_id=?")
        .bind(id).fetch_one(&mut *conn).await?.get::<i64,_>(0);
    let vid = sqlx::query(
        "INSERT INTO plan_versions(project_id, version, note, lights_json) VALUES(?1,?2,?3,?4)",
    )
    .bind(id).bind(next).bind(req.note.clone().unwrap_or_default()).bind(&lights_json)
    .execute(&mut *conn).await?.last_insert_rowid();
    sqlx::query("UPDATE projects SET updated_at=datetime('now') WHERE id=?").bind(id)
        .execute(&mut *conn).await?;
    sqlx::query("COMMIT").execute(&mut *conn).await?;
    Ok(Json(serde_json::json!({"ok": true, "version": next, "plan_version_id": vid})))
}

async fn submit(
    State(st): State<AppState>, Path(id): Path<i64>, headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    let user = current_user(&headers, &st.db).await?;
    let p = project_or_404(&st.db, id).await?;
    if p.photographer_id != user.id {
        return Err(AppError::forbidden("只能提交自己的项目"));
    }
    if p.status != "draft" && p.status != "submitted" {
        return Err(AppError::conflict(format!("项目处于 {} 状态，不能提交", p.status)));
    }
    let has_version: i64 = sqlx::query("SELECT COUNT(*) FROM plan_versions WHERE project_id=?")
        .bind(id).fetch_one(&st.db).await?.get(0);
    if has_version == 0 {
        return Err(AppError::bad("请先保存灯位方案再提交"));
    }
    sqlx::query("UPDATE projects SET status='submitted', updated_at=datetime('now') WHERE id=?")
        .bind(id).execute(&st.db).await?;
    notify(&st.db, None, "submit", &format!("项目「{}」已提交，等待场务确认", p.name)).await;
    Ok(Json(serde_json::json!({"ok": true})))
}

/// 核心：场务确认排期（场地 + 灯具 + 回路功率，事务内强一致）
async fn confirm(
    State(st): State<AppState>, Path(id): Path<i64>, headers: HeaderMap, Json(req): Json<ConfirmReq>,
) -> Result<Json<serde_json::Value>, AppError> {
    let user = current_user(&headers, &st.db).await?;
    require_role(&user, "crew")?;
    // 硬性规则：任何强制放行尝试都被拒绝
    if req.force == Some(true) {
        return Err(AppError::forbidden("回路功率超限不允许手工强制放行"));
    }

    let mut conn = st.db.acquire().await?;
    sqlx::query("BEGIN IMMEDIATE").execute(&mut *conn).await?;

    let mut proj_name = String::new();
    let mut proj_owner = 0i64;
    let result: Result<_, AppError> = async {
        let p = sqlx::query_as::<_, Project>(
            "SELECT id,name,photographer_id,status,venue_id,shoot_start,shoot_end,created_at,updated_at
             FROM projects WHERE id=?",
        ).bind(id).fetch_optional(&mut *conn).await?
        .ok_or_else(|| AppError::not_found("项目不存在"))?;
        proj_name = p.name.clone();
        proj_owner = p.photographer_id;

        if p.status != "submitted" {
            return Err(AppError::conflict(format!("项目处于 {} 状态，只有待确认项目可以确认", p.status)));
        }
        let row = sqlx::query(
            "SELECT id, lights_json FROM plan_versions WHERE project_id=? ORDER BY version DESC LIMIT 1",
        ).bind(id).fetch_one(&mut *conn).await?;
        let vid: i64 = row.get(0);
        let lights_json: String = row.get(1);
        let lights: Vec<LightInput> = serde_json::from_str(&lights_json).map_err(|e| AppError::bad(e.to_string()))?;

        let venue_id = p.venue_id.ok_or_else(|| AppError::bad("项目未指定场地"))?;
        let start = p.shoot_start.as_deref().ok_or_else(|| AppError::bad("缺少开始时间"))?;
        let end = p.shoot_end.as_deref().ok_or_else(|| AppError::bad("缺少结束时间"))?;

        let report = scan_conflicts(&mut conn, venue_id, start, end, &lights, id).await?;
        if !report.errors.is_empty() {
            return Err(AppError::conflict(report.errors.join("；")));
        }

        write_holds(&mut conn, id, vid, venue_id, start, end, &lights).await?;
        sqlx::query(
            "INSERT INTO confirmations(project_id, plan_version_id, crew_id, result, remark)
             VALUES(?1,?2,?3,'confirmed',?4)",
        ).bind(id).bind(vid).bind(user.id).bind(req.remark.clone().unwrap_or_default())
        .execute(&mut *conn).await?;
        sqlx::query("UPDATE projects SET status='confirmed', updated_at=datetime('now') WHERE id=?")
            .bind(id).execute(&mut *conn).await?;
        Ok(serde_json::json!({"ok": true, "loads": report.circuit_loads}))
    }.await;

    match result {
        Ok(v) => {
            sqlx::query("COMMIT").execute(&mut *conn).await?;
            notify(&st.db, Some(proj_owner), "submit",
                &format!("项目「{}」已确认排期，场地与灯具已占用", proj_name)).await;
            Ok(Json(v))
        }
        Err(e) => {
            let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await;
            Err(e)
        }
    }
}

async fn reject(
    State(st): State<AppState>, Path(id): Path<i64>, headers: HeaderMap, Json(req): Json<RejectReq>,
) -> Result<Json<serde_json::Value>, AppError> {
    let user = current_user(&headers, &st.db).await?;
    require_role(&user, "crew")?;
    let p = project_or_404(&st.db, id).await?;
    if p.status != "submitted" {
        return Err(AppError::conflict("只有待确认项目可以驳回"));
    }
    let vid: i64 = sqlx::query("SELECT id FROM plan_versions WHERE project_id=? ORDER BY version DESC LIMIT 1")
        .bind(id).fetch_one(&st.db).await?.get(0);
    let remark = req.remark.unwrap_or_default();
    sqlx::query(
        "INSERT INTO confirmations(project_id, plan_version_id, crew_id, result, remark)
         VALUES(?1,?2,?3,'rejected',?4)",
    ).bind(id).bind(vid).bind(user.id).bind(&remark).execute(&st.db).await?;
    sqlx::query("UPDATE projects SET status='draft', updated_at=datetime('now') WHERE id=?")
        .bind(id).execute(&st.db).await?;
    notify(&st.db, Some(p.photographer_id), "reject",
        &format!("项目「{}」被驳回：{}", p.name, remark)).await;
    Ok(Json(serde_json::json!({"ok": true})))
}

/// 换灯 / 延时：释放旧占用 -> 新版本 -> 回到待确认
async fn replan(
    State(st): State<AppState>, Path(id): Path<i64>, headers: HeaderMap, Json(req): Json<ReplanReq>,
) -> Result<Json<serde_json::Value>, AppError> {
    let user = current_user(&headers, &st.db).await?;
    let p = project_or_404(&st.db, id).await?;
    if p.photographer_id != user.id {
        return Err(AppError::forbidden("只能变更自己的项目"));
    }
    if p.status != "confirmed" && p.status != "returning" {
        return Err(AppError::conflict("只有已确认的项目需要走「变更方案」"));
    }
    let start = norm_time(&req.start)?;
    let end = norm_time(&req.end)?;
    if start >= end {
        return Err(AppError::bad("结束时间必须晚于开始时间"));
    }
    if req.lights.is_empty() {
        return Err(AppError::bad("方案至少需要一盏灯"));
    }
    let lights_json = serde_json::to_string(&req.lights).unwrap();

    let mut conn = st.db.acquire().await?;
    sqlx::query("BEGIN IMMEDIATE").execute(&mut *conn).await?;
    let result: Result<_, AppError> = async {
        // 1) 先预检新方案（不通过则旧占用保留，项目仍可用）
        let preview = scan_conflicts(&mut conn, req.venue_id, &req.start, &req.end, &req.lights, id).await?;
        if !preview.errors.is_empty() {
            return Err(AppError::conflict(preview.errors.join("；")));
        }
        // 2) 释放旧占用（立即对其他项目可用）
        release_holds(&mut conn, id).await?;
        // 3) 追加新版本
        let next: i64 = sqlx::query("SELECT COALESCE(MAX(version),0)+1 FROM plan_versions WHERE project_id=?")
            .bind(id).fetch_one(&mut *conn).await?.get(0);
        sqlx::query(
            "INSERT INTO plan_versions(project_id, version, note, lights_json) VALUES(?1,?2,?3,?4)",
        ).bind(id).bind(next).bind(req.note.clone().unwrap_or_default()).bind(&lights_json)
        .execute(&mut *conn).await?;
        sqlx::query(
            "INSERT INTO confirmations(project_id, plan_version_id, crew_id, result, remark)
             SELECT ?1, id, ?2, 'revoked', '方案修改（换灯/延时），旧占用已释放' FROM plan_versions
             WHERE project_id=?1 ORDER BY version DESC LIMIT 1 OFFSET 1",
        ).bind(id).bind(user.id).execute(&mut *conn).await?;
        // 上一行取的是新版本；修正 crew_id 语义无所谓（摄影师操作仅记录人）——确认历史中 revoked 表示系统释放
        sqlx::query(
            "UPDATE projects SET status='submitted', venue_id=?2, shoot_start=?3, shoot_end=?4, updated_at=datetime('now')
             WHERE id=?1",
        ).bind(id).bind(req.venue_id).bind(&req.start).bind(&req.end)
        .execute(&mut *conn).await?;
        Ok(serde_json::json!({"ok": true, "version": next}))
    }.await;
    match result {
        Ok(v) => {
            sqlx::query("COMMIT").execute(&mut *conn).await?;
            notify(&st.db, None, "replan", &format!("项目「{}」申请变更方案，旧占用已释放，待重新确认", p.name)).await;
            Ok(Json(v))
        }
        Err(e) => { let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await; Err(e) }
    }
}

/// 取消项目：立即释放全部资源
async fn cancel(
    State(st): State<AppState>, Path(id): Path<i64>, headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    let user = current_user(&headers, &st.db).await?;
    let p = project_or_404(&st.db, id).await?;
    if user.role == "photographer" && p.photographer_id != user.id {
        return Err(AppError::forbidden("只能取消自己的项目"));
    }
    if matches!(p.status.as_str(), "completed" | "cancelled") {
        return Err(AppError::conflict("项目已结束，不能取消"));
    }
    let was_confirmed = matches!(p.status.as_str(), "confirmed" | "returning");

    let mut conn = st.db.acquire().await?;
    sqlx::query("BEGIN IMMEDIATE").execute(&mut *conn).await?;
    release_holds(&mut conn, id).await?;
    if was_confirmed {
        sqlx::query(
            "INSERT INTO confirmations(project_id, plan_version_id, crew_id, result, remark)
             SELECT ?1, id, ?2, 'revoked', '项目取消，资源全部释放' FROM plan_versions
             WHERE project_id=?1 ORDER BY version DESC LIMIT 1",
        ).bind(id).bind(user.id).execute(&mut *conn).await?;
    }
    sqlx::query("UPDATE projects SET status='cancelled', updated_at=datetime('now') WHERE id=?")
        .bind(id).execute(&mut *conn).await?;
    sqlx::query("COMMIT").execute(&mut *conn).await?;
    notify(&st.db, None, "cancel", &format!("项目「{}」已取消，场地与灯具全部释放", p.name)).await;
    Ok(Json(serde_json::json!({"ok": true})))
}

/// 预检冲突（不落库）
async fn check(
    State(st): State<AppState>, Path(id): Path<i64>, Json(req): Json<CheckReq>,
) -> Result<Json<serde_json::Value>, AppError> {
    let start = norm_time(&req.start)?;
    let end = norm_time(&req.end)?;
    let mut conn = st.db.acquire().await?;
    sqlx::query("BEGIN IMMEDIATE").execute(&mut *conn).await?;
    let report = scan_conflicts(&mut conn, req.venue_id, &start, &end, &req.lights, id).await?;
    sqlx::query("ROLLBACK").execute(&mut *conn).await?;
    Ok(Json(serde_json::json!({
        "ok": report.errors.is_empty(),
        "errors": report.errors,
        "venue_busy": report.venue_busy,
        "fixture_busy": report.fixture_busy,
        "circuit_loads": report.circuit_loads,
    })))
}

/// 逐件归还
async fn return_items(
    State(st): State<AppState>, Path(id): Path<i64>, headers: HeaderMap, Json(req): Json<ReturnItemsReq>,
) -> Result<Json<serde_json::Value>, AppError> {
    let user = current_user(&headers, &st.db).await?;
    require_role(&user, "crew")?;
    if req.items.is_empty() {
        return Err(AppError::bad("没有要登记的归还记录"));
    }
    let p = project_or_404(&st.db, id).await?;
    if !matches!(p.status.as_str(), "confirmed" | "returning") {
        return Err(AppError::conflict("只有已确认项目可以登记归还"));
    }

    let mut conn = st.db.acquire().await?;
    sqlx::query("BEGIN IMMEDIATE").execute(&mut *conn).await?;
    let result: Result<_, AppError> = async {
        for item in &req.items {
            // 必须是本次项目占用的灯具，且未归还
            let booked: i64 = sqlx::query(
                "SELECT COUNT(*) FROM booked_lights WHERE project_id=?1 AND fixture_id=?2",
            ).bind(id).bind(item.fixture_id).fetch_one(&mut *conn).await?.get(0);
            if booked == 0 {
                return Err(AppError::bad(format!("灯具 #{} 不属于本次项目占用", item.fixture_id)));
            }
            let already: i64 = sqlx::query(
                "SELECT COUNT(*) FROM returns WHERE project_id=?1 AND fixture_id=?2",
            ).bind(id).bind(item.fixture_id).fetch_one(&mut *conn).await?.get(0);
            if already > 0 {
                return Err(AppError::bad(format!("灯具 #{} 已登记归还", item.fixture_id)));
            }
            if item.condition != "ok" && item.condition != "damaged" {
                return Err(AppError::bad("condition 只能是 ok / damaged"));
            }
            sqlx::query(
                "INSERT INTO returns(project_id, fixture_id, condition, note, returned_by)
                 VALUES(?1,?2,?3,?4,?5)",
            ).bind(id).bind(item.fixture_id).bind(&item.condition).bind(&item.note).bind(user.id)
            .execute(&mut *conn).await?;
            if item.condition == "damaged" {
                sqlx::query(
                    "INSERT INTO damages(project_id, fixture_id, description) VALUES(?1,?2,?3)",
                ).bind(id).bind(item.fixture_id).bind(&item.note)
                .execute(&mut *conn).await?;
            }
            // 单件归还 -> 该灯具立即恢复可用
            sqlx::query("DELETE FROM fixture_slots WHERE project_id=?1 AND fixture_id=?2")
                .bind(id).bind(item.fixture_id).execute(&mut *conn).await?;
        }

        let remaining: i64 = sqlx::query(
            "SELECT COUNT(*) FROM booked_lights bl
             WHERE bl.project_id=? AND NOT EXISTS(SELECT 1 FROM returns r WHERE r.project_id=bl.project_id AND r.fixture_id=bl.fixture_id)",
        ).bind(id).fetch_one(&mut *conn).await?.get(0);

        let new_status = if remaining == 0 {
            // 全部归还：释放场地，项目完成
            sqlx::query("DELETE FROM venue_slots WHERE project_id=?").bind(id).execute(&mut *conn).await?;
            "completed"
        } else {
            "returning"
        };
        sqlx::query("UPDATE projects SET status=?2, updated_at=datetime('now') WHERE id=?1")
            .bind(id).bind(new_status).execute(&mut *conn).await?;
        Ok(serde_json::json!({"ok": true, "status": new_status, "remaining": remaining}))
    }.await;
    match result {
        Ok(v) => {
            sqlx::query("COMMIT").execute(&mut *conn).await?;
            let damaged = req.items.iter().filter(|i| i.condition == "damaged").count();
            if damaged > 0 {
                notify(&st.db, None, "damage",
                    &format!("项目「{}」归还时登记 {} 件损坏，等待豁免审批", p.name, damaged)).await;
            }
            Ok(Json(v))
        }
        Err(e) => { let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await; Err(e) }
    }
}

/// 损坏豁免审批：场务且不能是本项目摄影师（双重保险）
async fn decide_damage(
    State(st): State<AppState>, Path(did): Path<i64>, headers: HeaderMap, Json(req): Json<DecideReq>,
) -> Result<Json<serde_json::Value>, AppError> {
    let user = current_user(&headers, &st.db).await?;
    require_role(&user, "crew")?;
    if req.waiver != "approved" && req.waiver != "denied" {
        return Err(AppError::bad("waiver 只能是 approved / denied"));
    }
    let row = sqlx::query("SELECT project_id FROM damages WHERE id=?").bind(did)
        .fetch_optional(&st.db).await?
        .ok_or_else(|| AppError::not_found("损坏记录不存在"))?;
    let project_id: i64 = row.get(0);
    let photographer_id: i64 = sqlx::query("SELECT photographer_id FROM projects WHERE id=?")
        .bind(project_id).fetch_one(&st.db).await?.get(0);
    if user.id == photographer_id {
        return Err(AppError::forbidden("摄影师不能审批自己项目的损坏豁免"));
    }
    sqlx::query(
        "UPDATE damages SET waiver=?2, decided_by=?3, decided_at=datetime('now') WHERE id=?1",
    ).bind(did).bind(&req.waiver).bind(user.id).execute(&st.db).await?;
    notify(&st.db, Some(photographer_id), "waiver",
        &format!("损坏豁免 #{} 审批结果：{}", did, if req.waiver == "approved" { "批准" } else { "驳回" })).await;
    Ok(Json(serde_json::json!({"ok": true})))
}

// ---------- 只读 / 模拟接口 ----------

async fn meta(State(st): State<AppState>) -> Json<MetaResp> {
    let users = sqlx::query_as("SELECT id,name,role FROM users ORDER BY id").fetch_all(&st.db).await.unwrap();
    let venues = sqlx::query_as("SELECT id,name,width_m,height_m,active FROM venues ORDER BY id").fetch_all(&st.db).await.unwrap();
    let circuits = sqlx::query_as("SELECT id,venue_id,name,max_watts,active FROM circuits ORDER BY id").fetch_all(&st.db).await.unwrap();
    let fixtures = sqlx::query_as("SELECT id,code,model,power_watts,active FROM fixtures ORDER BY id").fetch_all(&st.db).await.unwrap();
    Json(MetaResp { users, venues, circuits, fixtures })
}

async fn timeline(State(st): State<AppState>) -> Result<Json<TimelineResp>, AppError> {
    let venues = sqlx::query_as::<_, TimelineSlot>(
        "SELECT vs.id, 'venue' AS kind, vs.venue_id AS ref_id, v.name AS ref_name,
                vs.project_id, p.name AS project_name, u.name AS owner_name,
                vs.start_at, vs.end_at
         FROM venue_slots vs JOIN venues v ON v.id=vs.venue_id
         JOIN projects p ON p.id=vs.project_id JOIN users u ON u.id=p.photographer_id",
    ).fetch_all(&st.db).await?;
    let fixtures = sqlx::query_as::<_, TimelineSlot>(
        "SELECT fs.id, 'fixture' AS kind, fs.fixture_id AS ref_id, f.code || ' ' || f.model AS ref_name,
                fs.project_id, p.name AS project_name, u.name AS owner_name,
                fs.start_at, fs.end_at
         FROM fixture_slots fs JOIN fixtures f ON f.id=fs.fixture_id
         JOIN projects p ON p.id=fs.project_id JOIN users u ON u.id=p.photographer_id",
    ).fetch_all(&st.db).await?;
    Ok(Json(TimelineResp { venue_slots: venues, fixture_slots: fixtures }))
}

async fn notifications(
    State(st): State<AppState>, headers: HeaderMap, q: Query<HashMap<String, String>>,
) -> Result<Json<Vec<NotificationRec>>, AppError> {
    // 需要登录态；摄影师看自己的+广播，场务看全部（crew 处理事务）
    let user = current_user(&headers, &st.db).await?;
    let rows = if user.role == "crew" {
        sqlx::query_as::<_, NotificationRec>(
            "SELECT id,user_id,kind,message,read,created_at FROM notifications ORDER BY id DESC LIMIT 100",
        ).fetch_all(&st.db).await?
    } else {
        let uid = q.get("user_id").and_then(|v| v.parse::<i64>().ok()).unwrap_or(user.id);
        sqlx::query_as::<_, NotificationRec>(
            "SELECT id,user_id,kind,message,read,created_at FROM notifications
             WHERE user_id IS NULL OR user_id=?1 ORDER BY id DESC LIMIT 100",
        ).bind(uid).fetch_all(&st.db).await?
    };
    Ok(Json(rows))
}

async fn mark_read(State(st): State<AppState>, Path(id): Path<i64>) -> Json<serde_json::Value> {
    sqlx::query("UPDATE notifications SET read=1 WHERE id=?").bind(id).execute(&st.db).await.ok();
    Json(serde_json::json!({"ok": true}))
}

/// 本地模拟功率读数：以 booked_lights 额定功率为基础叠加确定性波动
async fn sim_power(
    State(st): State<AppState>, Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    let p = project_or_404(&st.db, id).await?;
    if !matches!(p.status.as_str(), "confirmed" | "returning") {
        return Err(AppError::conflict("项目未确认，无实时功率读数"));
    }
    let readings = sqlx::query_as::<_, PowerReading>(
        "SELECT bl.circuit_id, c.name AS circuit_name, c.max_watts,
                SUM(bl.watts) AS nominal_watts,
                SUM(bl.watts) + ((bl.circuit_id*37 + bl.line_no*13) % 41) - 20 AS measured_watts,
                0 AS overload
         FROM booked_lights bl JOIN circuits c ON c.id=bl.circuit_id
         WHERE bl.project_id=?
         GROUP BY bl.circuit_id ORDER BY bl.circuit_id",
    ).bind(id).fetch_all(&st.db).await?;
    let mut readings: Vec<PowerReading> = readings.into_iter().map(|mut r| {
        r.measured_watts = r.measured_watts.max(0);
        r.overload = r.measured_watts > r.max_watts as i64;
        r
    }).collect();
    readings.sort_by_key(|r| r.circuit_id);
    let any_overload = readings.iter().any(|r| r.overload);
    if any_overload {
        notify(&st.db, None, "power",
            &format!("项目「{}」模拟电表检测到回路过载，请立即检查", p.name)).await;
    }
    Ok(Json(serde_json::json!({"readings": readings, "overload": any_overload})))
}

async fn health() -> &'static str { "ok" }

// ---------- 启动 ----------

#[tokio::main]
async fn main() -> anyhow_lite::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let db_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:studio.db?mode=rwc".into());
    let opts: SqliteConnectOptions = db_url.parse::<SqliteConnectOptions>()?
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .busy_timeout(std::time::Duration::from_secs(5))
        .foreign_keys(true);
    let db = SqlitePoolOptions::new().max_connections(8).connect_with(opts).await?;
    sqlx::migrate!("./migrations").run(&db).await?;
    seed(&db).await;

    let state = AppState { db };

    let api = Router::new()
        .route("/health", get(health))
        .route("/meta", get(meta))
        .route("/projects", get(list_projects).post(create_project))
        .route("/projects/{id}", get(get_project).patch(update_header))
        .route("/projects/{id}/versions", post(save_version))
        .route("/projects/{id}/submit", post(submit))
        .route("/projects/{id}/confirm", post(confirm))
        .route("/projects/{id}/reject", post(reject))
        .route("/projects/{id}/replan", post(replan))
        .route("/projects/{id}/cancel", post(cancel))
        .route("/projects/{id}/check", post(check))
        .route("/projects/{id}/returns", post(return_items))
        .route("/projects/{id}/sim-power", get(sim_power))
        .route("/damages/{id}/decision", post(decide_damage))
        .route("/timeline", get(timeline))
        .route("/notifications", get(notifications))
        .route("/notifications/{id}/read", post(mark_read))
        .with_state(state)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    let app = Router::new()
        .nest("/api", api)
        .fallback_service(
            ServeDir::new("dist")
                .fallback(ServeFile::new("dist/index.html")),
        );

    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".into());
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    tracing::info!("listening on http://0.0.0.0:{port}");
    axum::serve(listener, app).await?;
    Ok(())
}

// ---------- 启动 ----------
mod anyhow_lite {
    pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
}

async fn seed(db: &Db) {
    let n: i64 = sqlx::query("SELECT COUNT(*) FROM users").fetch_one(db).await.unwrap().get(0);
    if n > 0 { return; }

    sqlx::query("INSERT INTO users(name, role) VALUES('陈曦','photographer'),('林夏','photographer'),('老周','crew')")
        .execute(db).await.unwrap();
    // 场地：12m x 8m 一号棚；回路 A1/A2/A3 各 2000W；二号棚 10m x 6m，回路 B1/B2/B3
    sqlx::query("INSERT INTO venues(name, width_m, height_m) VALUES('一号摄影棚',12,8),('二号摄影棚',10,6)").execute(db).await.unwrap();
    sqlx::query("INSERT INTO circuits(venue_id, name, max_watts) VALUES
        (1,'A1',2000),(1,'A2',2000),(1,'A3',2000),
        (2,'B1',2000),(2,'B2',2000),(2,'B3',2000)").execute(db).await.unwrap();
    sqlx::query("INSERT INTO fixtures(code, model, power_watts) VALUES
        ('LED-300-01','LED 聚光灯 300W',300),
        ('LED-300-02','LED 聚光灯 300W',300),
        ('LED-500-01','LED 平板灯 500W',500),
        ('LED-500-02','LED 平板灯 500W',500),
        ('TUNG-1000-01','钨丝灯 1000W',1000),
        ('TUNG-1000-02','钨丝灯 1000W',1000),
        ('STROBE-600-01','闪光灯 600W',600)")
        .execute(db).await.unwrap();
    tracing::info!("seeded demo data");
}
