use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct User {
    pub id: i64,
    pub name: String,
    pub role: String,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Venue {
    pub id: i64,
    pub name: String,
    pub width_m: f64,
    pub height_m: f64,
    pub active: i64,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Circuit {
    pub id: i64,
    pub venue_id: i64,
    pub name: String,
    pub max_watts: i64,
    pub active: i64,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Fixture {
    pub id: i64,
    pub code: String,
    pub model: String,
    pub power_watts: i64,
    pub active: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Project {
    pub id: i64,
    pub name: String,
    pub photographer_id: i64,
    pub status: String,
    pub venue_id: Option<i64>,
    pub shoot_start: Option<String>,
    pub shoot_end: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// 前端提交的一盏灯
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LightInput {
    pub fixture_id: Option<i64>,
    #[serde(default)]
    pub label: Option<String>,
    pub circuit_id: i64,
    pub watts: i64,
    pub pos_x: f64,
    pub pos_y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct PlanVersion {
    pub id: i64,
    pub project_id: i64,
    pub version: i64,
    pub note: String,
    pub created_at: String,
    pub lights_json: String,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct ConfirmationRec {
    pub id: i64,
    pub project_id: i64,
    pub plan_version_id: i64,
    pub crew_id: i64,
    pub crew_name: String,
    pub result: String,
    pub remark: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct BookedLightRec {
    pub id: i64,
    pub project_id: i64,
    pub plan_version_id: i64,
    pub line_no: i64,
    pub fixture_id: Option<i64>,
    pub circuit_id: i64,
    pub watts: i64,
    pub pos_x: f64,
    pub pos_y: f64,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReturnItem {
    pub fixture_id: i64,
    pub condition: String,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct ReturnRec {
    pub id: i64,
    pub project_id: i64,
    pub fixture_id: i64,
    pub fixture_code: String,
    pub condition: String,
    pub note: String,
    pub returned_by: i64,
    pub returned_by_name: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct DamageRec {
    pub id: i64,
    pub project_id: i64,
    pub fixture_id: i64,
    pub fixture_code: String,
    pub description: String,
    pub waiver: String,
    pub decided_by: Option<i64>,
    pub decided_by_name: Option<String>,
    pub decided_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Slot {
    pub id: i64,
    pub project_id: i64,
    pub venue_id: i64,
    pub start_at: String,
    pub end_at: String,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct FixtureSlotRec {
    pub id: i64,
    pub project_id: i64,
    pub fixture_id: i64,
    pub start_at: String,
    pub end_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectDetail {
    #[serde(flatten)]
    pub p: Project,
    pub photographer: User,
    pub versions: Vec<PlanVersion>,
    pub confirmations: Vec<ConfirmationRec>,
    pub booked: Vec<BookedLightRec>,
    pub returns: Vec<ReturnRec>,
    pub damages: Vec<DamageRec>,
    pub venue_slot: Option<Slot>,
    pub fixture_slots: Vec<FixtureSlotRec>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct ProjectSummary {
    pub id: i64,
    pub name: String,
    pub photographer_id: i64,
    pub photographer_name: String,
    pub status: String,
    pub venue_id: Option<i64>,
    pub venue_name: Option<String>,
    pub shoot_start: Option<String>,
    pub shoot_end: Option<String>,
    pub version: Option<i64>,
    pub last_result: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BusyProject {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FixtureBusy {
    pub fixture_id: i64,
    pub project: BusyProject,
    pub start: String,
    pub end: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CircuitLoad {
    pub circuit_id: i64,
    pub circuit_name: String,
    pub used_watts: i64,
    pub added_watts: i64,
    pub total_watts: i64,
    pub max_watts: i64,
    pub overload: bool,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct MetaResp {
    pub users: Vec<User>,
    pub venues: Vec<Venue>,
    pub circuits: Vec<Circuit>,
    pub fixtures: Vec<Fixture>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct TimelineSlot {
    pub id: i64,
    pub kind: String,
    pub ref_id: i64,
    pub ref_name: String,
    pub project_id: i64,
    pub project_name: String,
    pub owner_name: String,
    pub start_at: String,
    pub end_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TimelineResp {
    pub venue_slots: Vec<TimelineSlot>,
    pub fixture_slots: Vec<TimelineSlot>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct NotificationRec {
    pub id: i64,
    pub user_id: Option<i64>,
    pub kind: String,
    pub message: String,
    pub read: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct PowerReading {
    pub circuit_id: i64,
    pub circuit_name: String,
    pub max_watts: i64,
    pub nominal_watts: i64,
    pub measured_watts: i64,
    pub overload: bool,
}
