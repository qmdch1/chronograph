#![windows_subsystem = "windows"]

mod aggregator;
mod icon;
mod app;
mod autostart;
mod db;
mod idle;
mod single_instance;
mod tracker;
mod tray;

use aggregator::AppEvent;
use anyhow::Result;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, watch};
use tracing::info;
use tracing_subscriber::EnvFilter;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("chronograph=info".parse()?))
        .init();

    let _instance = single_instance::SingleInstance::acquire()?;

    let args: Vec<String> = std::env::args().collect();
    let start_hidden = args.contains(&"--minimized".to_string());

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    let db = rt.block_on(db::Db::open())?;
    let pool = db.pool.clone();

    info!("DB opened, starting background tasks");

    let (event_tx, event_rx) = mpsc::channel::<AppEvent>(256);

    rt.spawn({
        let tx = event_tx.clone();
        async move {
            let (inner_tx, mut inner_rx) = mpsc::channel::<tracker::TrackerEvent>(64);
            tokio::spawn(async move {
                if let Err(e) = tracker::run(inner_tx).await {
                    tracing::error!("tracker: {e}");
                }
            });
            while let Some(ev) = inner_rx.recv().await {
                let _ = tx.send(AppEvent::Tracker(ev)).await;
            }
        }
    });

    rt.spawn({
        let tx = event_tx.clone();
        async move {
            let (inner_tx, mut inner_rx) = mpsc::channel::<idle::IdleEvent>(64);
            tokio::spawn(async move {
                if let Err(e) = idle::run(inner_tx).await {
                    tracing::error!("idle: {e}");
                }
            });
            while let Some(ev) = inner_rx.recv().await {
                let _ = tx.send(AppEvent::Idle(ev)).await;
            }
        }
    });

    let (active_tx, active_rx) = watch::channel::<Option<tracker::WindowInfo>>(None);

    let _aggregator_handle = rt.spawn({
        let pool = pool.clone();
        async move {
            if let Err(e) = aggregator::run(pool, event_rx, active_tx).await {
                tracing::error!("aggregator: {e}");
            }
        }
    });

    let ctx_slot: Arc<Mutex<Option<egui::Context>>> = Arc::new(Mutex::new(None));
    single_instance::SingleInstance::listen_for_show(ctx_slot.clone());
    let (_tray_icon, tray_rx) = tray::create(ctx_slot.clone())?;

    let rt_handle = rt.handle().clone();
    app::run(pool, rt_handle, event_tx, active_rx, tray_rx, ctx_slot, start_hidden)?;

    Ok(())
}
