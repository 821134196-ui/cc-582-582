-- 用户
CREATE TABLE users (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    name       TEXT NOT NULL,
    role       TEXT NOT NULL CHECK (role IN ('photographer', 'crew'))
);

-- 场地（摄影棚）
CREATE TABLE venues (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT NOT NULL,
    width_m     REAL NOT NULL DEFAULT 12.0,   -- 平面图宽（米）
    height_m    REAL NOT NULL DEFAULT 8.0,    -- 平面图高（米）
    active      INTEGER NOT NULL DEFAULT 1
);

-- 回路（挂在场地下，带功率上限）
CREATE TABLE circuits (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    venue_id     INTEGER NOT NULL REFERENCES venues(id),
    name         TEXT NOT NULL,          -- 例如 A1 / A2
    max_watts    INTEGER NOT NULL CHECK (max_watts > 0),
    active       INTEGER NOT NULL DEFAULT 1
);

-- 灯具（实物设备，单件；power_watts 为典型功率）
CREATE TABLE fixtures (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    code         TEXT NOT NULL UNIQUE,    -- 例如 LED-300-02
    model        TEXT NOT NULL,
    power_watts  INTEGER NOT NULL CHECK (power_watts > 0),
    active       INTEGER NOT NULL DEFAULT 1
);

-- 项目（拍摄方案）
CREATE TABLE projects (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    name           TEXT NOT NULL,
    photographer_id INTEGER NOT NULL REFERENCES users(id),
    status         TEXT NOT NULL DEFAULT 'draft'
                   CHECK (status IN ('draft','submitted','confirmed','returning','completed','cancelled')),
    venue_id       INTEGER REFERENCES venues(id),
    shoot_start    TEXT,                 -- ISO8601，UTC
    shoot_end      TEXT,
    created_at     TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at    TEXT NOT NULL DEFAULT (datetime('now'))
);

-- 方案版本：每次保存产生新版本，永不覆盖
CREATE TABLE plan_versions (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id  INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    version     INTEGER NOT NULL,
    note        TEXT NOT NULL DEFAULT '',
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    -- 灯位：[{fixture_id|null, model, x, y, circuit_id, watts}]
    lights_json TEXT NOT NULL,
    UNIQUE (project_id, version)
);

-- 已确认方案占用的灯位（由最近一次确认的版本快照生成）
CREATE TABLE booked_lights (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id      INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    plan_version_id INTEGER NOT NULL REFERENCES plan_versions(id),
    line_no         INTEGER NOT NULL,       -- 方案内的行号
    fixture_id      INTEGER REFERENCES fixtures(id), -- 待定灯具可空
    circuit_id      INTEGER NOT NULL REFERENCES circuits(id),
    watts           INTEGER NOT NULL CHECK (watts > 0),
    pos_x           REAL NOT NULL,
    pos_y           REAL NOT NULL,
    label           TEXT NOT NULL DEFAULT '',
    created_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

-- 场地时段占用
CREATE TABLE venue_slots (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id  INTEGER NOT NULL UNIQUE REFERENCES projects(id) ON DELETE CASCADE,
    venue_id    INTEGER NOT NULL REFERENCES venues(id),
    start_at    TEXT NOT NULL,
    end_at      TEXT NOT NULL
);

-- 灯具时段占用（confirmed -> returned 期间）
CREATE TABLE fixture_slots (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id  INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    fixture_id  INTEGER NOT NULL REFERENCES fixtures(id),
    start_at    TEXT NOT NULL,
    end_at      TEXT NOT NULL
);

-- 确认记录（每次重新确认都追加，保留历史）
CREATE TABLE confirmations (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id       INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    plan_version_id  INTEGER NOT NULL REFERENCES plan_versions(id),
    crew_id          INTEGER NOT NULL REFERENCES users(id),
    result           TEXT NOT NULL CHECK (result IN ('confirmed','rejected','revoked')),
    remark           TEXT NOT NULL DEFAULT '',
    created_at       TEXT NOT NULL DEFAULT (datetime('now'))
);

-- 归还登记（逐件）
CREATE TABLE returns (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id    INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    fixture_id    INTEGER NOT NULL REFERENCES fixtures(id),
    condition     TEXT NOT NULL CHECK (condition IN ('ok','damaged')),
    note          TEXT NOT NULL DEFAULT '',
    returned_by   INTEGER NOT NULL REFERENCES users(id),
    created_at    TEXT NOT NULL DEFAULT (datetime('now'))
);

-- 损坏记录，关联项目
CREATE TABLE damages (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id    INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    fixture_id    INTEGER NOT NULL REFERENCES fixtures(id),
    description   TEXT NOT NULL DEFAULT '',
    waiver        TEXT NOT NULL DEFAULT 'pending'
                  CHECK (waiver IN ('pending','approved','denied')),
    decided_by    INTEGER REFERENCES users(id),
    decided_at    TEXT,
    created_at    TEXT NOT NULL DEFAULT (datetime('now'))
);

-- 通知（本地模拟）
CREATE TABLE notifications (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id     INTEGER REFERENCES users(id),  -- NULL = 广播
    kind        TEXT NOT NULL,
    message     TEXT NOT NULL,
    read        INTEGER NOT NULL DEFAULT 0,
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

-- 重叠查询索引
CREATE INDEX idx_fixture_slots_fixture_time ON fixture_slots(fixture_id, start_at, end_at);
CREATE INDEX idx_venue_slots_venue_time     ON venue_slots(venue_id, start_at, end_at);
CREATE INDEX idx_booked_lights_project      ON booked_lights(project_id);
CREATE INDEX idx_returns_project            ON returns(project_id);
CREATE INDEX idx_damages_project            ON damages(project_id);
