CREATE TABLE sessions (
    id INTEGER PRIMARY KEY,
    start_ts INTEGER NOT NULL,
    end_ts   INTEGER,
    process_name TEXT NOT NULL,
    exe_path TEXT NOT NULL,
    window_title TEXT NOT NULL,
    is_idle INTEGER NOT NULL DEFAULT 0,
    last_heartbeat_ts INTEGER NOT NULL
);
CREATE INDEX idx_sessions_start ON sessions(start_ts);
