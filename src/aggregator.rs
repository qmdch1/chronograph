use anyhow::Result;
use chrono::Utc;
use sqlx::SqlitePool;
use tokio::sync::{mpsc, watch};
use tokio::time::{Duration, interval};
use tracing::{info, warn};

use crate::idle::IdleEvent;
use crate::tracker::{TrackerEvent, WindowInfo};

const HEARTBEAT_INTERVAL_SECS: u64 = 30;

pub type ActiveWindowSender = watch::Sender<Option<WindowInfo>>;

pub enum AppEvent {
    Tracker(TrackerEvent),
    Idle(IdleEvent),
    Pause,
    Resume,
}

struct ActiveSession {
    id: i64,
    window: WindowInfo,
    is_idle: bool,
}

pub async fn run(
    pool: SqlitePool,
    mut rx: mpsc::Receiver<AppEvent>,
    active_tx: ActiveWindowSender,
) -> Result<()> {
    recover_crashed_sessions(&pool).await?;

    let mut current: Option<ActiveSession> = None;
    let mut paused = false;
    let mut heartbeat = interval(Duration::from_secs(HEARTBEAT_INTERVAL_SECS));
    heartbeat.tick().await;

    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                if let Some(ref sess) = current {
                    let now = Utc::now().timestamp_millis();
                    let _ = sqlx::query(
                        "UPDATE sessions SET last_heartbeat_ts = ? WHERE id = ?"
                    )
                    .bind(now)
                    .bind(sess.id)
                    .execute(&pool)
                    .await;
                }
            }

            event = rx.recv() => {
                let Some(event) = event else { break };

                match event {
                    AppEvent::Pause => {
                        if !paused {
                            paused = true;
                            if let Some(sess) = current.take() {
                                close_session(&pool, sess.id).await;
                            }
                            let _ = active_tx.send(None);
                            info!("tracking paused");
                        }
                    }

                    AppEvent::Resume => {
                        if paused {
                            paused = false;
                            info!("tracking resumed");
                        }
                    }

                    AppEvent::Tracker(TrackerEvent::FocusChanged(win)) => {
                        if paused { continue; }
                        let is_idle = current.as_ref().map(|s| s.is_idle).unwrap_or(false);

                        if let Some(sess) = current.take() {
                            close_session(&pool, sess.id).await;
                        }

                        info!(process = %win.process_name, title = %win.window_title, "focus changed");

                        let id = if !is_idle {
                            open_session(&pool, &win, false).await
                        } else {
                            None
                        };

                        let _ = active_tx.send(if is_idle { None } else { Some(win.clone()) });
                        current = id.map(|id| ActiveSession { id, window: win, is_idle: false });
                    }

                    AppEvent::Idle(idle_event) => {
                        if paused { continue; }
                        let idle = idle_event == IdleEvent::Idle;

                        if let Some(ref mut sess) = current {
                            if sess.is_idle != idle {
                                let old_id = sess.id;
                                close_session(&pool, old_id).await;

                                let win = sess.window.clone();
                                let new_id = if !idle {
                                    open_session(&pool, &win, false).await
                                } else {
                                    None
                                };

                                sess.id = new_id.unwrap_or(0);
                                sess.is_idle = idle;

                                let _ = active_tx.send(if idle { None } else { Some(win) });
                            }
                        }
                    }
                }
            }
        }
    }

    if let Some(sess) = current {
        close_session(&pool, sess.id).await;
    }

    Ok(())
}

async fn open_session(pool: &SqlitePool, win: &WindowInfo, is_idle: bool) -> Option<i64> {
    let now = Utc::now().timestamp_millis();
    match sqlx::query(
        "INSERT INTO sessions (start_ts, process_name, exe_path, window_title, is_idle, last_heartbeat_ts)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(now)
    .bind(&win.process_name)
    .bind(&win.exe_path)
    .bind(&win.window_title)
    .bind(is_idle as i64)
    .bind(now)
    .execute(pool)
    .await
    {
        Ok(r) => Some(r.last_insert_rowid()),
        Err(e) => { warn!("open_session failed: {e}"); None }
    }
}

async fn close_session(pool: &SqlitePool, id: i64) {
    if id == 0 { return; }
    let now = Utc::now().timestamp_millis();
    if let Err(e) = sqlx::query(
        "UPDATE sessions SET end_ts = ?, last_heartbeat_ts = ? WHERE id = ?"
    )
    .bind(now).bind(now).bind(id)
    .execute(pool).await
    {
        warn!("close_session({id}) failed: {e}");
    }
}

async fn recover_crashed_sessions(pool: &SqlitePool) -> Result<()> {
    let rows = sqlx::query_as::<_, (i64, i64)>(
        "SELECT id, last_heartbeat_ts FROM sessions WHERE end_ts IS NULL"
    )
    .fetch_all(pool)
    .await?;

    if rows.is_empty() { return Ok(()); }

    info!("recovering {} crashed session(s)", rows.len());
    for (id, _) in rows {
        sqlx::query("UPDATE sessions SET end_ts = last_heartbeat_ts WHERE id = ?")
            .bind(id).execute(pool).await?;
    }
    Ok(())
}
