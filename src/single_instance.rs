use anyhow::{Result, bail};
use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, ERROR_PIPE_CONNECTED, HANDLE, INVALID_HANDLE_VALUE};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, WaitNamedPipeW,
    NAMED_PIPE_MODE, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, ReadFile, WriteFile,
    FILE_FLAGS_AND_ATTRIBUTES, FILE_GENERIC_WRITE, FILE_SHARE_NONE,
    OPEN_EXISTING, PIPE_ACCESS_INBOUND,
};
use windows::core::PCWSTR;

const PIPE_NAME: &str = r"\\.\pipe\ChronographShowWindow";

pub struct SingleInstance {
    handle: HANDLE,
}

impl SingleInstance {
    /// Returns Ok if this is the first instance.
    /// Sends a show-window signal to the existing instance and returns Err if already running.
    pub fn acquire() -> Result<Self> {
        let name: Vec<u16> = "Local\\ChronographSingleInstance\0"
            .encode_utf16()
            .collect();

        let handle = unsafe {
            CreateMutexW(None, true, PCWSTR(name.as_ptr()))
        }?;

        let last_err = unsafe { windows::Win32::Foundation::GetLastError() };
        if last_err == ERROR_ALREADY_EXISTS {
            unsafe { CloseHandle(handle) }?;
            // 기존 인스턴스에 창 표시 신호 전달
            let _ = send_show_signal();
            bail!("chronograph is already running");
        }

        Ok(Self { handle })
    }

    /// 백그라운드 스레드에서 show-window 신호를 수신합니다.
    pub fn listen_for_show(ctx_slot: std::sync::Arc<std::sync::Mutex<Option<egui::Context>>>) {
        std::thread::spawn(move || {
            loop {
                let pipe_name: Vec<u16> = format!("{}\0", PIPE_NAME).encode_utf16().collect();

                let pipe = unsafe {
                    CreateNamedPipeW(
                        PCWSTR(pipe_name.as_ptr()),
                        PIPE_ACCESS_INBOUND,
                        NAMED_PIPE_MODE(
                            PIPE_TYPE_BYTE.0 | PIPE_READMODE_BYTE.0 | PIPE_WAIT.0,
                        ),
                        1,   // max instances
                        64,  // out buffer
                        64,  // in buffer
                        0,   // default timeout
                        None,
                    )
                };

                if pipe == INVALID_HANDLE_VALUE {
                    std::thread::sleep(std::time::Duration::from_secs(1));
                    continue;
                }

                // 클라이언트 연결 대기 (블로킹)
                let connected = unsafe { ConnectNamedPipe(pipe, None) };
                let last_err = unsafe { windows::Win32::Foundation::GetLastError() };

                if connected.is_ok() || last_err == ERROR_PIPE_CONNECTED {
                    let mut buf = [0u8; 1];
                    let mut read = 0u32;
                    let _ = unsafe {
                        ReadFile(pipe, Some(&mut buf), Some(&mut read), None)
                    };
                    show_window(&ctx_slot);
                }

                let _ = unsafe { CloseHandle(pipe) };
            }
        });
    }
}

fn send_show_signal() -> Result<()> {
    let pipe_name: Vec<u16> = format!("{}\0", PIPE_NAME).encode_utf16().collect();

    // 최대 2초 대기
    let _ = unsafe { WaitNamedPipeW(PCWSTR(pipe_name.as_ptr()), 2000) };

    let handle = unsafe {
        CreateFileW(
            PCWSTR(pipe_name.as_ptr()),
            FILE_GENERIC_WRITE.0,
            FILE_SHARE_NONE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        )
    }?;

    let buf = [1u8];
    let mut written = 0u32;
    let _ = unsafe { WriteFile(handle, Some(&buf), Some(&mut written), None) };
    let _ = unsafe { CloseHandle(handle) };
    Ok(())
}

fn show_window(ctx_slot: &std::sync::Arc<std::sync::Mutex<Option<egui::Context>>>) {
    if let Some(ctx) = ctx_slot.lock().unwrap().as_ref() {
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        ctx.request_repaint();
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        unsafe { let _ = CloseHandle(self.handle); }
    }
}
