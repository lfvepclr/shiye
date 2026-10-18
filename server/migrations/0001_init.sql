-- 拾页 · 初始 schema
-- 约定：所有 id 为 UUIDv4 字符串；所有时间戳为 Unix 秒（INTEGER）。

CREATE TABLE IF NOT EXISTS groups (
    id          TEXT PRIMARY KEY,
    device_id   TEXT NOT NULL,
    title       TEXT NOT NULL,
    state       TEXT NOT NULL DEFAULT 'draft',
    page_count  INTEGER NOT NULL DEFAULT 0,
    done_count  INTEGER NOT NULL DEFAULT 0,
    docx_rev    INTEGER NOT NULL DEFAULT 0,
    error       TEXT,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_groups_device ON groups (device_id, created_at DESC);

CREATE TABLE IF NOT EXISTS pages (
    id          TEXT PRIMARY KEY,
    group_id    TEXT NOT NULL REFERENCES groups (id) ON DELETE CASCADE,
    ord         INTEGER NOT NULL,
    status      TEXT NOT NULL DEFAULT 'pending',
    w           INTEGER NOT NULL DEFAULT 0,
    h           INTEGER NOT NULL DEFAULT 0,
    bytes       INTEGER NOT NULL DEFAULT 0,
    ocr_ms      INTEGER,
    error       TEXT,
    created_at  INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_pages_group ON pages (group_id, ord);

CREATE TABLE IF NOT EXISTS blocks (
    page_id     TEXT NOT NULL REFERENCES pages (id) ON DELETE CASCADE,
    ord         INTEGER NOT NULL,
    label       TEXT NOT NULL,
    text        TEXT NOT NULL,
    x1          REAL NOT NULL DEFAULT 0,
    y1          REAL NOT NULL DEFAULT 0,
    x2          REAL NOT NULL DEFAULT 0,
    y2          REAL NOT NULL DEFAULT 0,
    poly        TEXT,
    PRIMARY KEY (page_id, ord)
);

CREATE TABLE IF NOT EXISTS shares (
    token       TEXT PRIMARY KEY,
    group_id    TEXT NOT NULL REFERENCES groups (id) ON DELETE CASCADE,
    expires_at  INTEGER,
    revoked     INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_shares_group ON shares (group_id);

CREATE TABLE IF NOT EXISTS settings (
    k TEXT PRIMARY KEY,
    v TEXT NOT NULL
);
