use anyhow::Result;
use tokio::sync::mpsc;
use tokio::time::{Duration, interval};
use wineventhook::{AccessibleObjectId, EventFilter, WindowEventHook, raw_event};

const POLL_INTERVAL_MS: u64 = 1000;

#[derive(Debug, Clone)]
pub struct WindowInfo {
    pub process_name: String,
    pub exe_path: String,
    pub window_title: String,
}

impl PartialEq for WindowInfo {
    fn eq(&self, other: &Self) -> bool {
        self.process_name == other.process_name && self.window_title == other.window_title
    }
}

#[derive(Debug, Clone)]
pub enum TrackerEvent {
    FocusChanged(WindowInfo),
}

pub async fn run(tx: mpsc::Sender<TrackerEvent>) -> Result<()> {
    let (hook_tx, mut hook_rx) = tokio::sync::mpsc::unbounded_channel();
    let hook = WindowEventHook::hook(
        EventFilter::default().event(raw_event::SYSTEM_FOREGROUND),
        hook_tx,
    )
    .await?;

    let mut poll_tick = interval(Duration::from_millis(POLL_INTERVAL_MS));
    poll_tick.tick().await; // 첫 tick 즉시 소비

    let mut last_sent: Option<WindowInfo> = None;

    loop {
        tokio::select! {
            // wineventhook 이벤트 (빠른 반응)
            event = hook_rx.recv() => {
                let Some(event) = event else { break };
                if event.object_type() != AccessibleObjectId::Window {
                    continue;
                }
                if let Some(info) = get_active_window_filtered() {
                    if Some(&info) != last_sent.as_ref() {
                        last_sent = Some(info.clone());
                        if tx.send(TrackerEvent::FocusChanged(info)).await.is_err() {
                            break;
                        }
                    }
                }
            }

            // 폴링 (hook이 놓친 경우 커버 — VS Code, Electron 앱 등)
            _ = poll_tick.tick() => {
                if let Some(info) = get_active_window_filtered() {
                    if Some(&info) != last_sent.as_ref() {
                        last_sent = Some(info.clone());
                        if tx.send(TrackerEvent::FocusChanged(info)).await.is_err() {
                            break;
                        }
                    }
                }
            }
        }
    }

    hook.unhook().await?;
    Ok(())
}

fn get_active_window_filtered() -> Option<WindowInfo> {
    let w = x_win::get_active_window().ok()?;
    let mut name = w.info.name.clone();

    // name이 비어있으면 exe 경로에서 파일명으로 폴백
    if name.is_empty() {
        name = std::path::Path::new(&w.info.path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
    }

    if name.is_empty() {
        return None;
    }

    Some(WindowInfo {
        process_name: name,
        exe_path: w.info.path,
        window_title: w.title,
    })
}
