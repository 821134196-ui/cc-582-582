use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PlanItem {
    pub fixture_id: i64,
    pub circuit_id: i64,
    #[serde(default)]
    pub x: Option<f64>,
    #[serde(default)]
    pub y: Option<f64>,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Plan {
    #[serde(default)]
    pub items: Vec<PlanItem>,
}

#[derive(Debug, Serialize)]
pub struct CircuitLoad {
    pub circuit_id: i64,
    pub circuit_name: String,
    pub max_power_w: i64,
    pub used_power_w: i64,
    pub planned_power_w: i64,
    pub total_after_w: i64,
    pub headroom_w: i64,
    pub overloaded: bool,
    pub fixtures: Vec<PlannedFixtureOnCircuit>,
}

#[derive(Debug, Serialize)]
pub struct PlannedFixtureOnCircuit {
    pub fixture_id: i64,
    pub fixture_name: String,
    pub power_w: i64,
}

#[derive(Debug, Serialize)]
pub struct Conflict {
    pub kind: String, // "fixture" | "studio"
    pub resource_id: i64,
    pub resource_name: String,
    pub project_id: i64,
    pub project_name: String,
    pub start_at: DateTime<Utc>,
    pub end_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct ValidationReport {
    pub ok: bool,
    pub total_power_w: i64,
    pub circuit_loads: Vec<CircuitLoad>,
    pub fixture_conflicts: Vec<Conflict>,
    pub studio_conflict: Option<Conflict>,
    pub errors: Vec<String>,
}
