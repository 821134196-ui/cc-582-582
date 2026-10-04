use crate::error::{AppError, ApiResult};
use crate::models::{CircuitLoad, Conflict, Plan, PlannedFixtureOnCircuit, ValidationReport};
use chrono::{DateTime, Utc};
use sqlx::SqlitePool;

/// 解析并校验 ISO8601 时间；结束必须晚于开始
pub fn parse_window(start: &str, end: &str) -> ApiResult<(DateTime<Utc>, DateTime<Utc>)> {
    let s = DateTime::parse_from_rfc3339(start)
        .map_err(|_| AppError::bad_request("开始时间格式应为 RFC3339，例如 2026-10-10T09:00:00Z"))?
        .with_timezone(&Utc);
    let e = DateTime::parse_from_rfc3339(end)
        .map_err(|_| AppError::bad_request("结束时间格式应为 RFC3339，例如 2026-10-10T18:00:00Z"))?
        .with_timezone(&Utc);
    if e <= s {
        return Err(AppError::bad_request("结束时间必须晚于开始时间"));
    }
    Ok((s, e))
}

/// 校验方案：器材合法性、重复项、灯具互斥、场地互斥、回路功率。
/// exclude_project_id: 重新确认/修改时，忽略本项目原有的占用。
pub async fn validate(
    pool: &SqlitePool,
    studio_id: i64,
    start: &DateTime<Utc>,
    end: &DateTime<Utc>,
    plan: &Plan,
    exclude_project_id: Option<i64>,
) -> ApiResult<ValidationReport> {
    let mut errors = Vec::new();

    if plan.items.is_empty() {
        errors.push("方案至少需要一盏灯".into());
    }

    // 同一方案内灯具不得重复
    let mut seen = std::collections::HashSet::new();
    for it in &plan.items {
        if !seen.insert(it.fixture_id) {
            errors.push(format!("灯具 #{} 在方案中重复出现", it.fixture_id));
        }
    }

    let start_s = start.to_rfc3339();
    let end_s = end.to_rfc3339();

    // 灯具信息 + 合法性（存在、属于本棚、回路属于本棚、功率）
    #[derive(sqlx::FromRow)]
    struct FixtureRow {
        id: i64,
        name: String,
        power_w: i64,
        studio_id: i64,
        status: String,
    }
    let fixtures: Vec<FixtureRow> = sqlx::query_as("SELECT id, name, power_w, studio_id, status FROM fixtures")
        .fetch_all(pool)
        .await?;
    let circuits_in_studio: Vec<(i64, String, i64)> =
        sqlx::query_as("SELECT id, name, max_power_w FROM circuits WHERE studio_id = ?")
            .bind(studio_id)
            .fetch_all(pool)
            .await?;

    let mut total_power: i64 = 0;
    // circuit_id -> (max, planned_sum, fixtures on circuit)
    let mut per_circuit: std::collections::HashMap<i64, (i64, i64, Vec<PlannedFixtureOnCircuit>)> =
        std::collections::HashMap::new();

    for item in &plan.items {
        let fx = match fixtures.iter().find(|f| f.id == item.fixture_id) {
            Some(f) => f,
            None => {
                errors.push(format!("灯具 #{} 不存在", item.fixture_id));
                continue;
            }
        };
        if fx.studio_id != studio_id {
            errors.push(format!("灯具「{}」不属于所选场地", fx.name));
        }
        if fx.status != "ok" {
            errors.push(format!("灯具「{}」当前状态为 {}，不可使用", fx.name, fx.status));
        }
        let c = match circuits_in_studio.iter().find(|(id, _, _)| *id == item.circuit_id) {
            Some(c) => c,
            None => {
                errors.push(format!("灯具「{}」所选回路 #{} 不存在或不属于本场地", fx.name, item.circuit_id));
                continue;
            }
        };
        total_power += fx.power_w;
        let entry = per_circuit.entry(item.circuit_id).or_insert_with(|| {
            (c.2, 0, Vec::new())
        });
        entry.1 += fx.power_w;
        entry.2.push(PlannedFixtureOnCircuit {
            fixture_id: fx.id,
            fixture_name: fx.name.clone(),
            power_w: fx.power_w,
        });
    }

    // ---- 灯具互斥冲突 ----
    let mut fixture_conflicts: Vec<Conflict> = Vec::new();
    for item in &plan.items {
        let rows: Vec<(i64, i64, String, String, String, String, String)> = sqlx::query_as(
            "SELECT b.fixture_id, b.project_id, f.name, p.name, b.start_at, b.end_at, p.status
             FROM bookings b
             JOIN fixtures f ON f.id = b.fixture_id
             JOIN projects p ON p.id = b.project_id
             WHERE b.fixture_id = ? AND b.status = 'held'
               AND b.start_at < ? AND b.end_at > ?
               AND b.project_id IS NOT ?",
        )
        .bind(item.fixture_id)
        .bind(&end_s)
        .bind(&start_s)
        .bind(exclude_project_id.unwrap_or(-1))
        .fetch_all(pool)
        .await?;
        for (fid, pid, fname, pname, bs, be, _pst) in rows {
            errors.push(format!(
                "灯具「{}」在所选时段已被项目「{}」占用（{} ~ {}）",
                fname, pname, bs, be
            ));
            fixture_conflicts.push(Conflict {
                kind: "fixture".into(),
                resource_id: fid,
                resource_name: fname,
                project_id: pid,
                project_name: pname,
                start_at: DateTime::parse_from_rfc3339(&bs).unwrap().with_timezone(&Utc),
                end_at: DateTime::parse_from_rfc3339(&be).unwrap().with_timezone(&Utc),
            });
        }
    }

    // ---- 场地冲突 ----
    let mut studio_conflict: Option<Conflict> = None;
    let srow: Option<(i64, i64, String, String, String, String)> = sqlx::query_as(
        "SELECT s.studio_id, s.project_id, st.name, p.name, s.start_at, s.end_at
         FROM studio_slots s
         JOIN studios st ON st.id = s.studio_id
         JOIN projects p ON p.id = s.project_id
         WHERE s.studio_id = ? AND s.status = 'held'
           AND s.start_at < ? AND s.end_at > ?
           AND s.project_id IS NOT ?
         LIMIT 1",
    )
    .bind(studio_id)
    .bind(&end_s)
    .bind(&start_s)
    .bind(exclude_project_id.unwrap_or(-1))
    .fetch_optional(pool)
    .await?;
    if let Some((sid, pid, sname, pname, ss, se)) = srow {
        errors.push(format!("场地在该时段已被项目「{}」占用", pname));
        studio_conflict = Some(Conflict {
            kind: "studio".into(),
            resource_id: sid,
            resource_name: sname,
            project_id: pid,
            project_name: pname,
            start_at: DateTime::parse_from_rfc3339(&ss).unwrap().with_timezone(&Utc),
            end_at: DateTime::parse_from_rfc3339(&se).unwrap().with_timezone(&Utc),
        });
    }

    // ---- 回路功率：本方案功率 + 时段重叠的既有占用 ----
    let mut circuit_loads = Vec::new();
    for (cid, (max_w, planned_w, fxlist)) in per_circuit {
        let used: (i64,) = sqlx::query_as(
            "SELECT COALESCE(SUM(f.power_w), 0)
             FROM bookings b JOIN fixtures f ON f.id = b.fixture_id
             WHERE b.circuit_id = ? AND b.status = 'held'
               AND b.start_at < ? AND b.end_at > ?
               AND b.project_id IS NOT ?",
        )
        .bind(cid)
        .bind(&end_s)
        .bind(&start_s)
        .bind(exclude_project_id.unwrap_or(-1))
        .fetch_one(pool)
        .await?;
        let used_w = used.0;
        let total = used_w + planned_w;
        let overloaded = total > max_w;
        if overloaded {
            let cname = circuits_in_studio.iter().find(|(id, _, _)| *id == cid).map(|c| c.1.clone()).unwrap_or_default();
            errors.push(format!(
                "回路「{}」功率超限：既有 {}W + 本方案 {}W = {}W > 上限 {}W",
                cname, used_w, planned_w, total, max_w
            ));
        }
        let cname = circuits_in_studio
            .iter()
            .find(|(id, _, _)| *id == cid)
            .map(|c| c.1.clone())
            .unwrap_or_default();
        circuit_loads.push(CircuitLoad {
            circuit_id: cid,
            circuit_name: cname,
            max_power_w: max_w,
            used_power_w: used_w,
            planned_power_w: planned_w,
            total_after_w: total,
            headroom_w: max_w - total,
            overloaded,
            fixtures: fxlist,
        });
    }
    circuit_loads.sort_by_key(|c| c.circuit_id);

    let ok = errors.is_empty();
    Ok(ValidationReport {
        ok,
        total_power_w: total_power,
        circuit_loads,
        fixture_conflicts,
        studio_conflict,
        errors,
    })
}
