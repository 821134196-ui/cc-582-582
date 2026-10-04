-- 摄影棚灯光排期系统 SQLite schema

PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS users (
    id   INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('photographer', 'crew', 'admin'))
);

CREATE TABLE IF NOT EXISTS studios (
    id       INTEGER PRIMARY KEY,
    name     TEXT NOT NULL,
    width_m  REAL NOT NULL DEFAULT 10,
    height_m REAL NOT NULL DEFAULT 8
);

CREATE TABLE IF NOT EXISTS circuits (
    id          INTEGER PRIMARY KEY,
    studio_id   INTEGER NOT NULL REFERENCES studios(id),
    name        TEXT NOT NULL,
    max_power_w INTEGER NOT NULL CHECK (max_power_w > 0)
);

CREATE TABLE IF NOT EXISTS fixtures (
    id                  INTEGER PRIMARY KEY,
    studio_id           INTEGER NOT NULL REFERENCES studios(id),
    name                TEXT NOT NULL,
    model               TEXT,
    power_w             INTEGER NOT NULL CHECK (power_w > 0),
    default_circuit_id  INTEGER REFERENCES circuits(id),
    status              TEXT NOT NULL DEFAULT 'ok'
                        CHECK (status IN ('ok', 'repair', 'retired'))
);

CREATE TABLE IF NOT EXISTS projects (
    id                 INTEGER PRIMARY KEY,
    name               TEXT NOT NULL,
    photographer_id    INTEGER NOT NULL REFERENCES users(id),
    studio_id          INTEGER NOT NULL REFERENCES studios(id),
    status             TEXT NOT NULL DEFAULT 'draft'
                       CHECK (status IN ('draft', 'pending', 'confirmed', 'active',
                                         'changes_requested', 'rejected',
                                         'completed', 'cancelled')),
    start_at           TEXT,
    end_at             TEXT,
    current_version_id INTEGER,
    created_at         TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS project_versions (
    id         INTEGER PRIMARY KEY,
    project_id INTEGER NOT NULL REFERENCES projects(id),
    version_no INTEGER NOT NULL,
    plan_json  TEXT NOT NULL,   -- { items: [{fixture_id, circuit_id, x, y, note}] }
    note       TEXT,
    created_by INTEGER NOT NULL REFERENCES users(id),
    created_at TEXT NOT NULL,
    UNIQUE (project_id, version_no)
);

CREATE TABLE IF NOT EXISTS confirmations (
    id                 INTEGER PRIMARY KEY,
    project_id         INTEGER NOT NULL REFERENCES projects(id),
    version_id         INTEGER NOT NULL REFERENCES project_versions(id),
    decision           TEXT NOT NULL
                       CHECK (decision IN ('confirmed', 'rejected', 'changes_requested')),
    power_summary_json TEXT,
    note               TEXT,
    confirmed_by       INTEGER NOT NULL REFERENCES users(id),
    created_at         TEXT NOT NULL
);

-- 场地占用：确认排期时与灯具占用在同一事务内写入
CREATE TABLE IF NOT EXISTS studio_slots (
    id          INTEGER PRIMARY KEY,
    project_id  INTEGER NOT NULL REFERENCES projects(id),
    studio_id   INTEGER NOT NULL REFERENCES studios(id),
    start_at    TEXT NOT NULL,
    end_at      TEXT NOT NULL,
    status      TEXT NOT NULL DEFAULT 'held'
                CHECK (status IN ('held', 'released')),
    released_at TEXT
);

-- 灯具占用（核心互斥资源）
CREATE TABLE IF NOT EXISTS bookings (
    id          INTEGER PRIMARY KEY,
    project_id  INTEGER NOT NULL REFERENCES projects(id),
    version_id  INTEGER NOT NULL REFERENCES project_versions(id),
    fixture_id  INTEGER NOT NULL REFERENCES fixtures(id),
    circuit_id  INTEGER NOT NULL REFERENCES circuits(id),
    start_at    TEXT NOT NULL,
    end_at      TEXT NOT NULL,
    x           REAL,
    y           REAL,
    note        TEXT,
    status      TEXT NOT NULL DEFAULT 'held'
                CHECK (status IN ('held', 'returned', 'cancelled')),
    returned_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_bookings_fixture
    ON bookings(fixture_id, start_at, end_at) WHERE status = 'held';
CREATE INDEX IF NOT EXISTS idx_bookings_circuit
    ON bookings(circuit_id, start_at, end_at) WHERE status = 'held';
CREATE INDEX IF NOT EXISTS idx_slots_studio
    ON studio_slots(studio_id, start_at, end_at) WHERE status = 'held';

CREATE TABLE IF NOT EXISTS returns (
    id          INTEGER PRIMARY KEY,
    booking_id  INTEGER NOT NULL UNIQUE REFERENCES bookings(id),
    project_id  INTEGER NOT NULL REFERENCES projects(id),
    fixture_id  INTEGER NOT NULL REFERENCES fixtures(id),
    condition   TEXT NOT NULL CHECK (condition IN ('ok', 'damaged')),
    note        TEXT,
    returned_by INTEGER NOT NULL REFERENCES users(id),
    created_at  TEXT NOT NULL
);

-- 损坏记录关联本次项目；豁免需他人审批
CREATE TABLE IF NOT EXISTS damages (
    id               INTEGER PRIMARY KEY,
    project_id       INTEGER NOT NULL REFERENCES projects(id),
    fixture_id       INTEGER NOT NULL REFERENCES fixtures(id),
    return_id        INTEGER REFERENCES returns(id),
    description      TEXT NOT NULL,
    reported_by      INTEGER NOT NULL REFERENCES users(id),
    waiver_status    TEXT NOT NULL DEFAULT 'none'
                     CHECK (waiver_status IN ('none', 'requested', 'approved', 'rejected')),
    waiver_approver  INTEGER REFERENCES users(id),
    waiver_note      TEXT,
    created_at       TEXT NOT NULL,
    resolved_at      TEXT
);

CREATE TABLE IF NOT EXISTS notifications (
    id         INTEGER PRIMARY KEY,
    user_id    INTEGER NOT NULL REFERENCES users(id),
    kind       TEXT NOT NULL,
    title      TEXT NOT NULL,
    body       TEXT,
    project_id INTEGER REFERENCES projects(id),
    is_read    INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL
);

-- ============ 数据库级硬约束（任何路径都无法手工绕过） ============

-- 同一灯具时间重叠互斥
CREATE TRIGGER IF NOT EXISTS trg_bookings_fixture_mutex
BEFORE INSERT ON bookings
WHEN NEW.status = 'held'
BEGIN
    SELECT CASE WHEN EXISTS (
        SELECT 1 FROM bookings b
        WHERE b.fixture_id = NEW.fixture_id
          AND b.status = 'held'
          AND b.id != NEW.id
          AND b.start_at < NEW.end_at
          AND b.end_at   > NEW.start_at
    ) THEN RAISE(ABORT, '灯具已被其他项目占用（互斥冲突）') END;
END;

-- 同一回路：重叠时段累计功率不得超过回路上限（逐行插入时累计生效）
CREATE TRIGGER IF NOT EXISTS trg_bookings_no_overload
BEFORE INSERT ON bookings
WHEN NEW.status = 'held'
BEGIN
    SELECT CASE WHEN EXISTS (
        SELECT 1 FROM circuits c
        WHERE c.id = NEW.circuit_id
          AND (
                SELECT COALESCE(SUM(f.power_w), 0)
                FROM bookings b JOIN fixtures f ON f.id = b.fixture_id
                WHERE b.circuit_id = NEW.circuit_id
                  AND b.status = 'held'
                  AND b.id != NEW.id
                  AND b.start_at < NEW.end_at
                  AND b.end_at   > NEW.start_at
              )
              + (SELECT power_w FROM fixtures WHERE id = NEW.fixture_id)
              > c.max_power_w
    ) THEN RAISE(ABORT, '回路功率超过上限，禁止放行') END;
END;

-- 维修/报废灯具不得占用
CREATE TRIGGER IF NOT EXISTS trg_bookings_fixture_usable
BEFORE INSERT ON bookings
WHEN NEW.status = 'held'
BEGIN
    SELECT CASE WHEN EXISTS (
        SELECT 1 FROM fixtures f WHERE f.id = NEW.fixture_id AND f.status != 'ok'
    ) THEN RAISE(ABORT, '灯具处于维修/报废状态，不可占用') END;
END;

-- 同一场地时间重叠互斥
CREATE TRIGGER IF NOT EXISTS trg_slots_studio_mutex
BEFORE INSERT ON studio_slots
WHEN NEW.status = 'held'
BEGIN
    SELECT CASE WHEN EXISTS (
        SELECT 1 FROM studio_slots s
        WHERE s.studio_id = NEW.studio_id
          AND s.status = 'held'
          AND s.id != NEW.id
          AND s.start_at < NEW.end_at
          AND s.end_at   > NEW.start_at
    ) THEN RAISE(ABORT, '场地时段已被占用') END;
END;

-- 摄影师不能审批自己项目的损坏豁免
CREATE TRIGGER IF NOT EXISTS trg_damages_no_self_waiver
BEFORE UPDATE OF waiver_status ON damages
WHEN NEW.waiver_status = 'approved'
BEGIN
    SELECT CASE WHEN EXISTS (
        SELECT 1 FROM projects p
        WHERE p.id = NEW.project_id
          AND p.photographer_id = NEW.waiver_approver
    ) THEN RAISE(ABORT, '摄影师不能审批自己项目的损坏豁免') END;
END;
