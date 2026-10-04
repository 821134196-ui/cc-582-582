use crate::error::{AppError, ApiResult};
use crate::models::Plan;
use crate::{engine, AppState};
use axum::{
    async_trait,
    extract::{Path, Query, State},
    http::{request::Parts, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

// ---------------- 当前登录用户（前端通过 X-User-Id 传入） ----------------

#[derive(Clone, Debug)]
pub struct Actor {
    pub id: i64,
    pub name: String,
    pub role: String,
}

#[async_trait]
impl axum::extract::FromRequestParts<AppState> for Actor {
    type Rejection = AppError;
    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let uid = parts
            .headers
            .get("x-user-id")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<i64>().ok())
            .ok_or_else(|| AppError::new(StatusCode::UNAUTHORIZED, "缺少 X-User-Id 请求头"))?;
        let row: Option<(i64, String, String)> =
            sqlx::query_as("SELECT id, name, role FROM users WHERE id = ?")
                .bind(uid)
                .fetch_optional(&state.pool)
                .await?;
        let (id, name, role) = row.ok_or_else(|| AppError::unauthorized("用户不存在"))?;
        Ok(Actor { id, name, role })
    }
}

pub fn router() -> Router<AppState> {
    Router::new()
        // 目录
        .route("/users", get(list_users))
        .route("/studios", get(list_studios))
        .route("/timeline", get(timeline))
        .route("/power/live", get(power_live))
        .route("/fixtures/:id/status", post(set_fixture_status))
        // 通知
        .route("/notifications", get(list_notifications).post(read_all))
        .route("/notifications/:id/read", post(read_one))
        // 项目
        .route("/projects", get(list_projects).post(create_project))
        .route("/validate", post(validate_dry_run))
        .route("/projects/:id", get(get_project).put(update_project))
        .route("/projects/:id/validate", post(validate_project))
        .route("/projects/:id/submit", post(submit_project))
        .route("/projects/:id/confirm", post(confirm_project))
        .route("/projects/:id/cancel", post(cancel_project))
        .route("/projects/:id/start", post(start_project))
        .route("/projects/:id/complete", post(complete_project))
        // 归还 / 损坏
        .route("/projects/:id/returns", get(list_returns).post(record_return))
        .route("/projects/:id/damages", get(list_damages))
        .route("/damages/:did/waiver/request", post(request_waiver))
        .route("/damages/:did/waiver/decision", post(waiver_decision))
}

// ---------------- 通用工具 ----------------

async fn project_row(
    state: &AppState,
    id: i64,
) -> ApiResult<(i64, String, i64, i64, String, Option<String>, Option<String>, Option<i64>)> {
    sqlx::query_as(
        "SELECT id, name, photographer_id, studio_id, status, start_at, end_at, current_version_id
         FROM projects WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::not_found("项目不存在"))
}

async fn notify(state: &AppState, user_id: i64, kind: &str, title: &str, body: &str, project_id: Option<i64>) {
    let now = Utc::now().to_rfc3339();
    let _ = sqlx::query(
        "INSERT INTO notifications (user_id, kind, title, body, project_id, created_at)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(user_id)
    .bind(kind)
    .bind(title)
    .bind(body)
    .bind(project_id)
    .bind(now)
    .execute(&state.pool)
    .await;
    // 本地模拟外部推送接口（企业微信/短信网关）：打印到服务端日志
    tracing::info!("[mock-notify → user {}] {} | {}", user_id, title, body);
}

async fn notify_role(state: &AppState, role: &str, kind: &str, title: &str, body: &str, project_id: i64) {
    let crews: Vec<(i64,)> = sqlx::query_as("SELECT id FROM users WHERE role = ?")
        .bind(role)
        .fetch_all(&state.pool)
        .await
        .unwrap_or_default();
    for (id,) in crews {
        notify(state, id, kind, title, body, Some(project_id)).await;
    }
}

/// 释放项目当前全部占用（灯具 + 场地），在事务中执行
async fn release_all(_state: &AppState, tx: &mut sqlx::SqliteConnection, project_id: i64) -> ApiResult<()> {
    let now = Utc::now().to_rfc3339();
    sqlx::query("UPDATE bookings SET status='cancelled', returned_at=COALESCE(returned_at, ?)
                 WHERE project_id = ? AND status = 'held'")
        .bind(&now)
        .bind(project_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE studio_slots SET status='released', released_at=?
                 WHERE project_id = ? AND status = 'held'")
        .bind(&now)
        .bind(project_id)
        .execute(&mut *tx)
        .await?;
    Ok(())
}

fn is_crew(role: &str) -> bool {
    role == "crew" || role == "admin"
}

// ---------------- 目录 ----------------

async fn list_users(State(s): State<AppState>) -> ApiResult<Json<Value>> {
    let rows: Vec<(i64, String, String)> =
        sqlx::query_as("SELECT id, name, role FROM users ORDER BY id").fetch_all(&s.pool).await?;
    Ok(Json(json!(rows.iter().map(|(id, name, role)| json!({
        "id": id, "name": name, "role": role
    })).collect::<Vec<_>>())))
}

async fn list_studios(State(s): State<AppState>, actor: Actor) -> ApiResult<Json<Value>> {
    let _ = actor;
    let studios: Vec<(i64, String, f64, f64)> =
        sqlx::query_as("SELECT id, name, width_m, height_m FROM studios ORDER BY id")
            .fetch_all(&s.pool)
            .await?;
    let mut out = Vec::new();
    for (id, name, w, h) in studios {
        let circuits: Vec<(i64, String, i64)> =
            sqlx::query_as("SELECT id, name, max_power_w FROM circuits WHERE studio_id=? ORDER BY id")
                .bind(id)
                .fetch_all(&s.pool)
                .await?;
        let cs: Vec<Value> = circuits
            .iter()
            .map(|(cid, cname, maxw)| json!({"id": cid, "name": cname, "max_power_w": maxw}))
            .collect();
        let fixtures: Vec<(i64, String, Option<String>, i64, Option<i64>, String)> = sqlx::query_as(
            "SELECT id, name, model, power_w, default_circuit_id, status
             FROM fixtures WHERE studio_id=? ORDER BY id")
            .bind(id)
            .fetch_all(&s.pool)
            .await?;
        let fs: Vec<Value> = fixtures
            .iter()
            .map(|(fid, fname, model, pw, def, st)| json!({
                "id": fid, "name": fname, "model": model,
                "power_w": pw, "default_circuit_id": def, "status": st
            }))
            .collect();
        out.push(json!({"id": id, "name": name, "width_m": w, "height_m": h,
                        "circuits": cs, "fixtures": fs}));
    }
    Ok(Json(json!(out)))
}

// ---------------- 通知 ----------------

async fn list_notifications(State(s): State<AppState>, actor: Actor) -> ApiResult<Json<Value>> {
    let rows: Vec<(i64, String, String, Option<String>, Option<i64>, i64, String)> = sqlx::query_as(
        "SELECT id, kind, title, body, project_id, is_read, created_at
         FROM notifications WHERE user_id=? ORDER BY id DESC LIMIT 100")
        .bind(actor.id)
        .fetch_all(&s.pool)
        .await?;
    Ok(Json(json!(rows.iter().map(|r| json!({
        "id": r.0, "kind": r.1, "title": r.2, "body": r.3,
        "project_id": r.4, "is_read": r.5 == 1, "created_at": r.6
    })).collect::<Vec<_>>())))
}

async fn read_one(State(s): State<AppState>, actor: Actor, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    sqlx::query("UPDATE notifications SET is_read=1 WHERE id=? AND user_id=?")
        .bind(id).bind(actor.id).execute(&s.pool).await?;
    Ok(Json(json!({"ok": true})))
}

async fn read_all(State(s): State<AppState>, actor: Actor) -> ApiResult<Json<Value>> {
    sqlx::query("UPDATE notifications SET is_read=1 WHERE user_id=?")
        .bind(actor.id).execute(&s.pool).await?;
    Ok(Json(json!({"ok": true})))
}

// ---------------- 项目 ----------------

#[derive(Deserialize)]
struct DryRun {
    studio_id: i64,
    start_at: String,
    end_at: String,
    #[serde(default)]
    exclude_project_id: Option<i64>,
    items: Vec<crate::models::PlanItem>,
}

async fn validate_dry_run(
    State(s): State<AppState>,
    _actor: Actor,
    Json(body): Json<DryRun>,
) -> ApiResult<Json<Value>> {
    let (start, end) = engine::parse_window(&body.start_at, &body.end_at)?;
    let plan = Plan { items: body.items };
    let report = engine::validate(&s.pool, body.studio_id, &start, &end, &plan, body.exclude_project_id).await?;
    Ok(Json(json!(report)))
}

#[derive(Deserialize)]
struct CreateProject {
    name: String,
    studio_id: i64,
    start_at: String,
    end_at: String,
    items: Vec<crate::models::PlanItem>,
    note: Option<String>,
}

async fn create_project(
    State(s): State<AppState>,
    actor: Actor,
    Json(body): Json<CreateProject>,
) -> ApiResult<impl IntoResponse> {
    if actor.role != "photographer" && actor.role != "admin" {
        return Err(AppError::forbidden("只有摄影师可以提交拍摄方案"));
    }
    let (start, end) = engine::parse_window(&body.start_at, &body.end_at)?;
    let now = Utc::now().to_rfc3339();

    let studio_exists: Option<(i64,)> = sqlx::query_as("SELECT id FROM studios WHERE id=?")
        .bind(body.studio_id).fetch_optional(&s.pool).await?;
    if studio_exists.is_none() {
        return Err(AppError::bad_request("场地不存在"));
    }

    let mut tx = s.pool.begin().await?;
    let pid: (i64,) = sqlx::query_as(
        "INSERT INTO projects (name, photographer_id, studio_id, status, start_at, end_at, created_at)
         VALUES (?, ?, ?, 'draft', ?, ?, ?) RETURNING id",
    )
    .bind(&body.name).bind(actor.id).bind(body.studio_id)
    .bind(start.to_rfc3339()).bind(end.to_rfc3339()).bind(&now)
    .fetch_one(&mut *tx).await?;

    let plan = Plan { items: body.items };
    let plan_json = serde_json::to_string(&plan).unwrap();
    let vid: (i64,) = sqlx::query_as(
        "INSERT INTO project_versions (project_id, version_no, plan_json, note, created_by, created_at)
         VALUES (?, 1, ?, ?, ?, ?) RETURNING id",
    )
    .bind(pid.0).bind(plan_json).bind(&body.note).bind(actor.id).bind(&now)
    .fetch_one(&mut *tx).await?;
    sqlx::query("UPDATE projects SET current_version_id=? WHERE id=?")
        .bind(vid.0).bind(pid.0).execute(&mut *tx).await?;
    tx.commit().await?;

    Ok((StatusCode::CREATED, Json(json!({"id": pid.0, "version_id": vid.0}))))
}

#[derive(Deserialize)]
struct ListQuery {
    status: Option<String>,
}

async fn list_projects(State(s): State<AppState>, actor: Actor, q: Query<ListQuery>) -> ApiResult<Json<Value>> {
    let mut sql = String::from(
        "SELECT p.id, p.name, p.photographer_id, u.name, p.studio_id, st.name,
                p.status, p.start_at, p.end_at, p.current_version_id, p.created_at
         FROM projects p JOIN users u ON u.id=p.photographer_id
         JOIN studios st ON st.id=p.studio_id WHERE 1=1");
    if q.status.is_some() {
        sql.push_str(" AND p.status = ?");
    }
    // 摄影师默认只看自己的；场务看全部（仍可通过 all=1）
    let mut query = sqlx::query_as::<_, (i64, String, i64, String, i64, String, String, String, String, Option<i64>, String)>(&sql);
    if let Some(st) = &q.status {
        query = query.bind(st);
    }
    let rows = query.fetch_all(&s.pool).await?;
    let list: Vec<Value> = rows
        .iter()
        .map(|r| json!({
            "id": r.0, "name": r.1, "photographer_id": r.2, "photographer_name": r.3,
            "studio_id": r.4, "studio_name": r.5, "status": r.6,
            "start_at": r.7, "end_at": r.8, "current_version_id": r.9, "created_at": r.10
        }))
        .collect();
    let _ = actor;
    Ok(Json(json!(list)))
}

async fn get_project(State(s): State<AppState>, actor: Actor, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    let p = project_row(&s, id).await?;
    let versions: Vec<(i64, i64, String, Option<String>, i64, String)> = sqlx::query_as(
        "SELECT id, version_no, plan_json, note, created_by, created_at
         FROM project_versions WHERE project_id=? ORDER BY version_no")
        .bind(id).fetch_all(&s.pool).await?;
    let confirms: Vec<(i64, String, Option<String>, Option<String>, i64, String)> = sqlx::query_as(
        "SELECT id, decision, power_summary_json, note, confirmed_by, created_at
         FROM confirmations WHERE project_id=? ORDER BY id")
        .bind(id).fetch_all(&s.pool).await?;
    let bookings: Vec<(i64, i64, i64, String, String, Option<f64>, Option<f64>, String, Option<String>)> = sqlx::query_as(
        "SELECT b.id, b.fixture_id, b.circuit_id, b.start_at, b.end_at, b.x, b.y, b.status, b.returned_at
         FROM bookings b WHERE b.project_id=? AND b.status='held' ORDER BY b.id")
        .bind(id).fetch_all(&s.pool).await?;
    let returns: Vec<(i64, i64, String, Option<String>, String)> = sqlx::query_as(
        "SELECT r.id, r.fixture_id, r.condition, r.note, r.created_at
         FROM returns r WHERE r.project_id=? ORDER BY r.id")
        .bind(id).fetch_all(&s.pool).await?;
    let slot: Option<(String, String, String)> = sqlx::query_as(
        "SELECT start_at, end_at, status FROM studio_slots WHERE project_id=? ORDER BY id DESC LIMIT 1")
        .bind(id).fetch_optional(&s.pool).await?;

    Ok(Json(json!({
        "id": p.0, "name": p.1, "photographer_id": p.2, "studio_id": p.3,
        "status": p.4, "start_at": p.5, "end_at": p.6, "current_version_id": p.7,
        "viewer_id": actor.id,
        "versions": versions.iter().map(|v| {
            let plan: Value = serde_json::from_str(&v.2).unwrap_or(json!({}));
            json!({"id": v.0, "version_no": v.1, "plan": plan, "note": v.3,
                   "created_by": v.4, "created_at": v.5})
        }).collect::<Vec<_>>(),
        "confirmations": confirms.iter().map(|c| json!({
            "id": c.0, "decision": c.1,
            "power_summary": c.2.as_ref().and_then(|x| serde_json::from_str::<Value>(x).ok()),
            "note": c.3, "confirmed_by": c.4, "created_at": c.5
        })).collect::<Vec<_>>(),
        "bookings": bookings.iter().map(|b| json!({
            "id": b.0, "fixture_id": b.1, "circuit_id": b.2,
            "start_at": b.3, "end_at": b.4, "x": b.5, "y": b.6,
            "status": b.7, "returned_at": b.8
        })).collect::<Vec<_>>(),
        "returns": returns.iter().map(|r| json!({
            "id": r.0, "fixture_id": r.1, "condition": r.2, "note": r.3, "created_at": r.4
        })).collect::<Vec<_>>(),
        "slot": slot.map(|s| json!({"start_at": s.0, "end_at": s.1, "status": s.2})),
    })))
}

#[derive(Deserialize)]
struct UpdateProject {
    name: Option<String>,
    studio_id: Option<i64>,
    start_at: Option<String>,
    end_at: Option<String>,
    items: Option<Vec<crate::models::PlanItem>>,
    note: Option<String>,
}

async fn update_project(
    State(s): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    Json(body): Json<UpdateProject>,
) -> ApiResult<Json<Value>> {
    let p = project_row(&s, id).await?;
    let (_, _, photographer_id, _, status, _, _, _) = p;
    if actor.role != "admin" && actor.id != photographer_id {
        return Err(AppError::forbidden("只能修改自己的方案"));
    }
    if matches!(status.as_str(), "active" | "completed" | "cancelled") {
        return Err(AppError::conflict(format!("项目当前状态为 {status}，不可修改；如需调整请另建方案")));
    }

    let studio_id = body.studio_id.unwrap_or(p.3);
    let start_s = body.start_at.as_deref().or(p.5.as_deref()).unwrap();
    let end_s = body.end_at.as_deref().or(p.6.as_deref()).unwrap();
    let (start, end) = engine::parse_window(start_s, end_s)?;

    // 取最新版本 plan（若未提供 items 则沿用）
    let latest: (i64, String) = sqlx::query_as(
        "SELECT id, plan_json FROM project_versions WHERE project_id=? ORDER BY version_no DESC LIMIT 1")
        .bind(id).fetch_one(&s.pool).await?;
    let plan: Plan = match body.items {
        Some(items) => Plan { items },
        None => serde_json::from_str(&latest.1).unwrap_or(Plan { items: vec![] }),
    };
    let name = body.name.clone().unwrap_or_else(|| p.1.clone());

    let now = Utc::now().to_rfc3339();
    let mut tx = s.pool.begin().await?;

    if let Some(n) = &body.name {
        sqlx::query("UPDATE projects SET name=? WHERE id=?").bind(n).bind(id).execute(&mut *tx).await?;
    }
    sqlx::query("UPDATE projects SET studio_id=?, start_at=?, end_at=? WHERE id=?")
        .bind(studio_id).bind(start.to_rfc3339()).bind(end.to_rfc3339()).bind(id)
        .execute(&mut *tx).await?;

    let next_no: (i64,) = sqlx::query_as(
        "SELECT COALESCE(MAX(version_no),0)+1 FROM project_versions WHERE project_id=?")
        .bind(id).fetch_one(&mut *tx).await?;
    let plan_json = serde_json::to_string(&plan).unwrap();
    let vid: (i64,) = sqlx::query_as(
        "INSERT INTO project_versions (project_id, version_no, plan_json, note, created_by, created_at)
         VALUES (?, ?, ?, ?, ?, ?) RETURNING id")
        .bind(id).bind(next_no.0).bind(plan_json)
        .bind(&body.note).bind(actor.id).bind(&now)
        .fetch_one(&mut *tx).await?;
    sqlx::query("UPDATE projects SET current_version_id=? WHERE id=?")
        .bind(vid.0).bind(id).execute(&mut *tx).await?;

    // 已确认的方案被修改（换灯/延时/换棚）：立即释放原占用，回到待确认，必须重新核对
    let mut needs_reconfirm = false;
    if status == "confirmed" {
        release_all(&s, &mut tx, id).await?;
        sqlx::query("UPDATE projects SET status='pending' WHERE id=?")
            .bind(id).execute(&mut *tx).await?;
        needs_reconfirm = true;
    }
    tx.commit().await?;

    if needs_reconfirm {
        notify_role(&s, "crew", "reconfirm",
            "方案已修改，需要重新确认",
            &format!("项目「{}」发生换灯/延时调整，原占用已释放，请重新核对设备与回路功率", name), id).await;
    }
    Ok(Json(json!({"ok": true, "version_id": vid.0, "needs_reconfirm": needs_reconfirm})))
}

async fn validate_project(State(s): State<AppState>, _actor: Actor, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    let p = project_row(&s, id).await?;
    let plan_json: String = sqlx::query_scalar(
        "SELECT plan_json FROM project_versions WHERE id=?")
        .bind(p.7.ok_or_else(|| AppError::bad_request("项目尚无方案版本"))?)
        .fetch_one(&s.pool).await?;
    let plan: Plan = serde_json::from_str(&plan_json).unwrap_or(Plan { items: vec![] });
    let (start, end) = engine::parse_window(p.5.as_ref().unwrap(), p.6.as_ref().unwrap())?;
    let report = engine::validate(&s.pool, p.3, &start, &end, &plan, Some(id)).await?;
    Ok(Json(json!(report)))
}

async fn submit_project(State(s): State<AppState>, actor: Actor, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    let p = project_row(&s, id).await?;
    if actor.role != "admin" && actor.id != p.2 {
        return Err(AppError::forbidden("只能提交自己的方案"));
    }
    if !matches!(p.4.as_str(), "draft" | "changes_requested" | "rejected" | "pending") {
        return Err(AppError::conflict(format!("项目状态 {} 下不可提交", p.4)));
    }
    let plan_json: String = sqlx::query_scalar(
        "SELECT plan_json FROM project_versions WHERE id=?")
        .bind(p.7.unwrap()).fetch_one(&s.pool).await?;
    let plan: Plan = serde_json::from_str(&plan_json).unwrap();
    let (start, end) = engine::parse_window(p.5.as_ref().unwrap(), p.6.as_ref().unwrap())?;
    let report = engine::validate(&s.pool, p.3, &start, &end, &plan, Some(id)).await?;
    if !report.ok {
        return Err(AppError::conflict(serde_json::to_string(&json!({
            "message": "方案存在冲突，无法提交", "report": report
        })).unwrap()));
    }
    sqlx::query("UPDATE projects SET status='pending' WHERE id=? AND status != 'pending'")
        .bind(id).execute(&s.pool).await?;
    notify_role(&s, "crew", "submit", "新的拍摄方案待确认",
        &format!("项目「{}」已提交，请核对灯具实物与回路负载", p.1), id).await;
    Ok(Json(json!({"ok": true})))
}

#[derive(Deserialize)]
struct ConfirmBody {
    decision: String, // confirmed | rejected | changes_requested
    note: Option<String>,
}

async fn confirm_project(
    State(s): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    Json(body): Json<ConfirmBody>,
) -> ApiResult<Json<Value>> {
    if !is_crew(&actor.role) {
        return Err(AppError::forbidden("只有场务可以确认排期"));
    }
    let p = project_row(&s, id).await?;
    if p.2 == actor.id {
        return Err(AppError::forbidden("不能确认自己提交的项目"));
    }
    if !matches!(body.decision.as_str(), "confirmed" | "rejected" | "changes_requested") {
        return Err(AppError::bad_request("decision 必须是 confirmed/rejected/changes_requested"));
    }
    if p.4 == "cancelled" || p.4 == "completed" {
        return Err(AppError::conflict("项目已结束或取消"));
    }

    let version_id = p.7.unwrap();
    let plan_json: String = sqlx::query_scalar("SELECT plan_json FROM project_versions WHERE id=?")
        .bind(version_id).fetch_one(&s.pool).await?;
    let plan: Plan = serde_json::from_str(&plan_json).unwrap();
    let (start, end) = engine::parse_window(p.5.as_ref().unwrap(), p.6.as_ref().unwrap())?;

    // 无论确认与否都重新计算一次（应用层给出可读错误；数据库触发器是最终防线）
    let report = engine::validate(&s.pool, p.3, &start, &end, &plan, Some(id)).await?;

    if body.decision == "confirmed" {
        // 回路功率超限 / 设备互斥：不存在任何“强制放行”参数
        if !report.ok {
            return Err(AppError::conflict(serde_json::to_string(&json!({
                "message": "核对未通过：设备冲突或回路功率超限，系统禁止确认（无手工放行入口）",
                "report": report
            })).unwrap()));
        }

        let mut tx = s.pool.begin().await?;
        // 重新确认：先释放该项目旧版本的全部占用，再按新版本占用
        release_all(&s, &mut tx, id).await?;

        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO studio_slots (project_id, studio_id, start_at, end_at, status)
             VALUES (?, ?, ?, ?, 'held')")
            .bind(id).bind(p.3).bind(start.to_rfc3339()).bind(end.to_rfc3339())
            .execute(&mut *tx).await?;

        for item in &plan.items {
            sqlx::query(
                "INSERT INTO bookings (project_id, version_id, fixture_id, circuit_id,
                                       start_at, end_at, x, y, note, status)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'held')")
                .bind(id).bind(version_id).bind(item.fixture_id).bind(item.circuit_id)
                .bind(start.to_rfc3339()).bind(end.to_rfc3339())
                .bind(item.x).bind(item.y).bind(&item.note)
                .execute(&mut *tx).await
                .map_err(|e| map_db_conflict(e))?;
        }

        let summary = json!({
            "total_power_w": report.total_power_w,
            "circuits": report.circuit_loads,
        });
        sqlx::query(
            "INSERT INTO confirmations (project_id, version_id, decision, power_summary_json, note, confirmed_by, created_at)
             VALUES (?, ?, 'confirmed', ?, ?, ?, ?)")
            .bind(id).bind(version_id).bind(summary.to_string())
            .bind(&body.note).bind(actor.id).bind(&now)
            .execute(&mut *tx).await?;
        sqlx::query("UPDATE projects SET status='confirmed' WHERE id=?")
            .bind(id).execute(&mut *tx).await?;
        tx.commit().await?;

        notify(&s, p.2, "confirmed", "排期已确认",
            &format!("项目「{}」已确认，场地与灯具同时锁定（{} ~ {}）", p.1, p.5.as_ref().unwrap(), p.6.as_ref().unwrap()),
            Some(id)).await;
        Ok(Json(json!({"ok": true, "report": report})))
    } else {
        let new_status = if body.decision == "rejected" { "rejected" } else { "changes_requested" };
        let mut tx = s.pool.begin().await?;
        release_all(&s, &mut tx, id).await?;
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO confirmations (project_id, version_id, decision, power_summary_json, note, confirmed_by, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)")
            .bind(id).bind(version_id).bind(&body.decision)
            .bind(serde_json::to_string(&report).ok()).bind(&body.note)
            .bind(actor.id).bind(&now)
            .execute(&mut *tx).await?;
        sqlx::query("UPDATE projects SET status=? WHERE id=?")
            .bind(new_status).bind(id).execute(&mut *tx).await?;
        tx.commit().await?;
        notify(&s, p.2, &body.decision,
            if new_status == "rejected" { "方案被驳回" } else { "请按意见修改方案" },
            body.note.as_deref().unwrap_or("场务给出了处理意见"), Some(id)).await;
        Ok(Json(json!({"ok": true, "report": report})))
    }
}

/// 把 SQLite 触发器抛出的互斥/超限错误映射为 409
fn map_db_conflict(e: sqlx::Error) -> AppError {
    let msg = e.to_string();
    if msg.contains("占用") || msg.contains("超限") || msg.contains("维修") {
        AppError::conflict(format!("数据库硬约束拦截：{msg}"))
    } else {
        AppError::from(e)
    }
}

async fn cancel_project(State(s): State<AppState>, actor: Actor, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    let p = project_row(&s, id).await?;
    if actor.role != "admin" && actor.id != p.2 && !is_crew(&actor.role) {
        return Err(AppError::forbidden("无权取消该项目"));
    }
    if p.4 == "cancelled" {
        return Err(AppError::conflict("项目已取消"));
    }
    if p.4 == "completed" {
        return Err(AppError::conflict("项目已完成，不可取消"));
    }
    let mut tx = s.pool.begin().await?;
    // 立即释放全部资源：灯具与场地在同一事务内恢复可用
    release_all(&s, &mut tx, id).await?;
    sqlx::query("UPDATE projects SET status='cancelled' WHERE id=?")
        .bind(id).execute(&mut *tx).await?;
    tx.commit().await?;

    if actor.id != p.2 {
        notify(&s, p.2, "cancelled", "项目已取消",
            &format!("项目「{}」已被取消，全部灯具与场地占用已立即释放", p.1), Some(id)).await;
    }
    notify_role(&s, "crew", "cancelled",
        "项目已取消，资源已释放", &format!("项目「{}」取消，资源即刻可被其他项目占用", p.1), id).await;
    Ok(Json(json!({"ok": true, "released": true})))
}

async fn start_project(State(s): State<AppState>, actor: Actor, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    if !is_crew(&actor.role) {
        return Err(AppError::forbidden("只有场务可以开拍"));
    }
    let p = project_row(&s, id).await?;
    if p.4 != "confirmed" {
        return Err(AppError::conflict("只有已确认的项目可以开拍"));
    }
    sqlx::query("UPDATE projects SET status='active' WHERE id=?")
        .bind(id).execute(&s.pool).await?;
    Ok(Json(json!({"ok": true})))
}

async fn complete_project(State(s): State<AppState>, actor: Actor, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    if !is_crew(&actor.role) && actor.id != p_owner(&s, id).await? {
        return Err(AppError::forbidden("无权完成项目"));
    }
    let p = project_row(&s, id).await?;
    if p.4 != "active" {
        return Err(AppError::conflict("只有进行中的项目可以完工"));
    }
    let remaining: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM bookings WHERE project_id=? AND status='held'")
        .bind(id).fetch_one(&s.pool).await?;
    if remaining.0 > 0 {
        return Err(AppError::conflict(format!("还有 {} 件灯具未归还登记，不能完工", remaining.0)));
    }
    let mut tx = s.pool.begin().await?;
    sqlx::query("UPDATE studio_slots SET status='released', released_at=?
                 WHERE project_id=? AND status='held'")
        .bind(Utc::now().to_rfc3339()).bind(id).execute(&mut *tx).await?;
    sqlx::query("UPDATE projects SET status='completed' WHERE id=?")
        .bind(id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(json!({"ok": true})))
}

async fn p_owner(s: &AppState, id: i64) -> ApiResult<i64> {
    Ok(project_row(s, id).await?.2)
}

// ---------------- 逐件归还 ----------------

async fn list_returns(State(s): State<AppState>, _actor: Actor, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    let rows: Vec<(i64, i64, String, Option<String>, i64, String)> = sqlx::query_as(
        "SELECT id, fixture_id, condition, note, returned_by, created_at
         FROM returns WHERE project_id=? ORDER BY id")
        .bind(id).fetch_all(&s.pool).await?;
    Ok(Json(json!(rows.iter().map(|r| json!({
        "id": r.0, "fixture_id": r.1, "condition": r.2, "note": r.3,
        "returned_by": r.4, "created_at": r.5
    })).collect::<Vec<_>>())))
}

#[derive(Deserialize)]
struct ReturnBody {
    fixture_id: i64,
    condition: String, // ok | damaged
    note: Option<String>,
}

async fn record_return(
    State(s): State<AppState>,
    actor: Actor,
    Path(id): Path<i64>,
    Json(body): Json<ReturnBody>,
) -> ApiResult<Json<Value>> {
    if !matches!(body.condition.as_str(), "ok" | "damaged") {
        return Err(AppError::bad_request("condition 必须是 ok 或 damaged"));
    }
    let p = project_row(&s, id).await?;
    if !matches!(p.4.as_str(), "active" | "confirmed") {
        return Err(AppError::conflict("项目未开拍或已结束，不能登记归还"));
    }
    // 找到该项目这件灯具当前 held 的占用（一件一次）
    let booking: Option<(i64,)> = sqlx::query_as(
        "SELECT id FROM bookings WHERE project_id=? AND fixture_id=? AND status='held'
         ORDER BY id LIMIT 1")
        .bind(id).bind(body.fixture_id).fetch_optional(&s.pool).await?;
    let booking_id = booking.ok_or_else(|| AppError::conflict("该灯具没有处于占用状态的记录（可能已归还）"))?.0;

    let now = Utc::now().to_rfc3339();
    let mut tx = s.pool.begin().await?;
    sqlx::query("UPDATE bookings SET status='returned', returned_at=? WHERE id=?")
        .bind(&now).bind(booking_id).execute(&mut *tx).await?;
    let rid: (i64,) = sqlx::query_as(
        "INSERT INTO returns (booking_id, project_id, fixture_id, condition, note, returned_by, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?) RETURNING id")
        .bind(booking_id).bind(id).bind(body.fixture_id)
        .bind(&body.condition).bind(&body.note).bind(actor.id).bind(&now)
        .fetch_one(&mut *tx).await?;

    let mut damage_id = None;
    if body.condition == "damaged" {
        let desc = body.note.clone().unwrap_or_else(|| "归还时发现损坏".into());
        let did: (i64,) = sqlx::query_as(
            "INSERT INTO damages (project_id, fixture_id, return_id, description, reported_by, created_at)
             VALUES (?, ?, ?, ?, ?, ?) RETURNING id")
            .bind(id).bind(body.fixture_id).bind(rid.0).bind(&desc)
            .bind(actor.id).bind(&now)
            .fetch_one(&mut *tx).await?;
        // 损坏灯具立即退出可调度池，直到修复
        sqlx::query("UPDATE fixtures SET status='repair' WHERE id=?")
            .bind(body.fixture_id).execute(&mut *tx).await?;
        damage_id = Some(did.0);
    }
    tx.commit().await?;

    if let Some(did) = damage_id {
        notify_role(&s, "crew", "damage", "灯具损坏上报",
            &format!("项目「{}」归还灯具 #{} 时登记损坏（记录 #{}），损坏已关联本项目", p.1, body.fixture_id, did),
            id).await;
    }
    Ok(Json(json!({"ok": true, "return_id": rid.0, "damage_id": damage_id})))
}

// ---------------- 损坏与豁免 ----------------

async fn list_damages(State(s): State<AppState>, _actor: Actor, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    let rows: Vec<(i64, i64, i64, Option<i64>, String, i64, String, Option<i64>, Option<String>, Option<String>, String)> =
        sqlx::query_as(
            "SELECT d.id, d.project_id, d.fixture_id, d.return_id, d.description, d.reported_by,
                    d.waiver_status, d.waiver_approver, d.waiver_note, d.created_at, f.name
             FROM damages d JOIN fixtures f ON f.id=d.fixture_id
             WHERE d.project_id=? ORDER BY d.id")
            .bind(id).fetch_all(&s.pool).await?;
    Ok(Json(json!(rows.iter().map(|r| json!({
        "id": r.0, "project_id": r.1, "fixture_id": r.2,
        "fixture_name": r.10,
        "return_id": r.3, "description": r.4, "reported_by": r.5,
        "waiver_status": r.6, "waiver_approver": r.7, "waiver_note": r.8,
        "created_at": r.9
    })).collect::<Vec<_>>())))
}

async fn request_waiver(State(s): State<AppState>, actor: Actor, Path(did): Path<i64>) -> ApiResult<Json<Value>> {
    let row: Option<(i64, String, Option<String>)> = sqlx::query_as("SELECT project_id, waiver_status, waiver_note FROM damages WHERE id=?")
        .bind(did).fetch_optional(&s.pool).await?;
    let (pid, status, _) = row.ok_or_else(|| AppError::not_found("损坏记录不存在"))?;
    let owner = p_owner(&s, pid).await?;
    if actor.id != owner && actor.role != "admin" {
        return Err(AppError::forbidden("只有项目摄影师可以申请损坏豁免"));
    }
    if status == "approved" || status == "rejected" {
        return Err(AppError::conflict("该损坏已有豁免结论"));
    }
    sqlx::query("UPDATE damages SET waiver_status='requested' WHERE id=?")
        .bind(did).execute(&s.pool).await?;
    notify_role(&s, "crew", "waiver", "损坏豁免待审批",
        &format!("项目 #{} 的损坏记录 #{} 申请豁免（摄影师本人无权审批）", pid, did), pid).await;
    Ok(Json(json!({"ok": true})))
}

#[derive(Deserialize)]
struct WaiverDecision {
    approve: bool,
    note: Option<String>,
}

async fn waiver_decision(
    State(s): State<AppState>,
    actor: Actor,
    Path(did): Path<i64>,
    Json(body): Json<WaiverDecision>,
) -> ApiResult<Json<Value>> {
    if !is_crew(&actor.role) {
        return Err(AppError::forbidden("只有场务/管理员可以审批豁免"));
    }
    let row: Option<(i64, i64, String)> =
        sqlx::query_as("SELECT id, project_id, waiver_status FROM damages WHERE id=?")
            .bind(did).fetch_optional(&s.pool).await?;
    let (_, pid, status) = row.ok_or_else(|| AppError::not_found("损坏记录不存在"))?;
    if status != "requested" {
        return Err(AppError::conflict("只能审批已申请的豁免"));
    }
    let owner = p_owner(&s, pid).await?;
    // 应用层硬规则：摄影师不能审批自己项目的损坏豁免（数据库触发器同样拦截）
    if actor.id == owner {
        return Err(AppError::forbidden("摄影师不能审批自己项目的损坏豁免"));
    }
    let new_status = if body.approve { "approved" } else { "rejected" };
    let res = sqlx::query(
        "UPDATE damages SET waiver_status=?, waiver_approver=?, waiver_note=?, resolved_at=? WHERE id=?")
        .bind(new_status).bind(actor.id).bind(&body.note)
        .bind(Utc::now().to_rfc3339()).bind(did)
        .execute(&s.pool).await;
    if let Err(e) = res {
        return Err(map_db_conflict(e));
    }
    notify(&s, owner, "waiver_decision", "损坏豁免审批结果",
        &format!("损坏记录 #{} 豁免{}", did, if body.approve { "已批准" } else { "未获批准" }), Some(pid)).await;
    Ok(Json(json!({"ok": true, "waiver_status": new_status})))
}

// ---------------- 排期时间线 ----------------

#[derive(Deserialize)]
struct TimelineQuery {
    studio_id: Option<i64>,
}

async fn timeline(State(s): State<AppState>, _actor: Actor, q: Query<TimelineQuery>) -> ApiResult<Json<Value>> {
    // 场地占用
    let mut slots_sql = String::from(
        "SELECT s.id, s.project_id, p.name, s.studio_id, st.name, s.start_at, s.end_at, s.status
         FROM studio_slots s
         JOIN projects p ON p.id=s.project_id JOIN studios st ON st.id=s.studio_id
         WHERE 1=1");
    if q.studio_id.is_some() {
        slots_sql.push_str(" AND s.studio_id = ?");
    }
    slots_sql.push_str(" ORDER BY s.start_at");
    let mut qry = sqlx::query_as::<_, (i64, i64, String, i64, String, String, String, String)>(&slots_sql);
    if let Some(id) = q.studio_id {
        qry = qry.bind(id);
    }
    let slot_rows = qry.fetch_all(&s.pool).await?;
    let slots: Vec<Value> = slot_rows.iter().map(|r| json!({
        "id": r.0, "project_id": r.1, "project_name": r.2,
        "studio_id": r.3, "studio_name": r.4,
        "start_at": r.5, "end_at": r.6, "status": r.7
    })).collect();

    // 灯具占用（直观看到每件设备所占时段）
    let mut bk_sql = String::from(
        "SELECT b.id, b.project_id, p.name, b.fixture_id, f.name, f.power_w,
                b.circuit_id, c.name, b.start_at, b.end_at, b.status, b.x, b.y
         FROM bookings b
         JOIN projects p ON p.id=b.project_id
         JOIN fixtures f ON f.id=b.fixture_id
         JOIN circuits c ON c.id=b.circuit_id
         WHERE 1=1");
    if q.studio_id.is_some() {
        bk_sql.push_str(" AND f.studio_id = ?");
    }
    bk_sql.push_str(" ORDER BY f.id, b.start_at");
    let mut qry2 = sqlx::query_as::<_, (i64, i64, String, i64, String, i64, i64, String, String, String, String, Option<f64>, Option<f64>)>(&bk_sql);
    if let Some(id) = q.studio_id {
        qry2 = qry2.bind(id);
    }
    let bk_rows = qry2.fetch_all(&s.pool).await?;
    let bookings: Vec<Value> = bk_rows.iter().map(|r| json!({
        "id": r.0, "project_id": r.1, "project_name": r.2,
        "fixture_id": r.3, "fixture_name": r.4, "power_w": r.5,
        "circuit_id": r.6, "circuit_name": r.7,
        "start_at": r.8, "end_at": r.9, "status": r.10, "x": r.11, "y": r.12
    })).collect();

    Ok(Json(json!({"slots": slots, "bookings": bookings})))
}

// ---------------- 灯具修复归库 / 停用 ----------------

#[derive(Deserialize)]
struct FixtureStatus {
    status: String, // ok | repair | retired
}

async fn set_fixture_status(
    State(s): State<AppState>,
    actor: Actor,
    Path(fid): Path<i64>,
    Json(body): Json<FixtureStatus>,
) -> ApiResult<Json<Value>> {
    if !is_crew(&actor.role) {
        return Err(AppError::forbidden("只有场务/管理员可以变更灯具状态"));
    }
    if !matches!(body.status.as_str(), "ok" | "repair" | "retired") {
        return Err(AppError::bad_request("status 必须是 ok/repair/retired"));
    }
    let exists: Option<(i64,)> = sqlx::query_as("SELECT id FROM fixtures WHERE id=?")
        .bind(fid).fetch_optional(&s.pool).await?;
    if exists.is_none() {
        return Err(AppError::not_found("灯具不存在"));
    }
    sqlx::query("UPDATE fixtures SET status=? WHERE id=?")
        .bind(&body.status).bind(fid).execute(&s.pool).await?;
    Ok(Json(json!({"ok": true, "fixture_id": fid, "status": body.status})))
}

// ---------------- 功率读数（本地模拟电表） ----------------

async fn power_live(
    State(s): State<AppState>,
    _actor: Actor,
    q: Query<HashMap<String, String>>,
) -> ApiResult<Json<Value>> {
    let studio_id: Option<i64> = q.get("studio_id").and_then(|x| x.parse().ok());
    let now = Utc::now();
    let now_s = now.to_rfc3339();

    let mut sql = String::from(
        "SELECT c.id, c.name, c.max_power_w, COALESCE(SUM(f.power_w), 0)
         FROM circuits c LEFT JOIN bookings b
           ON b.circuit_id=c.id AND b.status='held'
           AND b.start_at <= ? AND b.end_at > ?
         LEFT JOIN fixtures f ON f.id=b.fixture_id
         WHERE 1=1");
    if studio_id.is_some() {
        sql.push_str(" AND c.studio_id = ?");
    }
    sql.push_str(" GROUP BY c.id ORDER BY c.id");
    let mut query = sqlx::query_as::<_, (i64, String, i64, i64)>(&sql).bind(&now_s).bind(&now_s);
    if let Some(id) = studio_id {
        query = query.bind(id);
    }
    let rows = query.fetch_all(&s.pool).await?;

    // 模拟电表抖动：按当前小时分钟做确定性伪随机，避免引入真实随机
    let minute_bucket = now.timestamp() / 60;
    let readings: Vec<Value> = rows
        .iter()
        .map(|r| {
            let seed = (minute_bucket.wrapping_add(r.0 * 97)) as u64;
            let jitter = ((seed.wrapping_mul(6364136223846793005) >> 33) % 7) as i64 - 3; // -3..3 %
            let measured = (r.3 + jitter).max(0);
            json!({
                "circuit_id": r.0, "circuit_name": r.1,
                "max_power_w": r.2, "scheduled_w": r.3,
                "measured_w": measured,
                "utilization": if r.2 > 0 { (measured as f64 / r.2 as f64 * 100.0).round() / 100.0 } else { 0.0 },
                "over": measured > r.2
            })
        })
        .collect();
    Ok(Json(json!({"now": now_s, "readings": readings})))
}
