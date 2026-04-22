use anyhow::Result;
use tokio::sync::mpsc;
use tokio::time::{Duration, interval};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

pub const IDLE_THRESHOLD_SECS: u64 = 60;
const POLL_INTERVAL_MS: u64 = 5_000;

#[derive(Debug, Clone, PartialEq)]
pub enum IdleEvent {
    Idle,
    Active,
}

pub async fn run(tx: mpsc::Sender<IdleEvent>) -> Result<()> {
    let mut ticker = interval(Duration::from_millis(POLL_INTERVAL_MS));
    let mut last_state = IdleEvent::Active;

    loop {
        ticker.tick().await;

        let idle_ms = idle_time_ms();
        let new_state = if idle_ms >= IDLE_THRESHOLD_SECS * 1000 {
            IdleEvent::Idle
        } else {
            IdleEvent::Active
        };

        if new_state != last_state {
            last_state = new_state.clone();
            if tx.send(new_state).await.is_err() {
                break;
            }
        }
    }

    Ok(())
}

fn idle_time_ms() -> u64 {
    unsafe {
        let mut info = LASTINPUTINFO {
            cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };
        if GetLastInputInfo(&mut info).as_bool() {
            let tick_count = windows::Win32::System::SystemInformation::GetTickCount();
            tick_count.saturating_sub(info.dwTime) as u64
        } else {
            0
        }
    }
}
