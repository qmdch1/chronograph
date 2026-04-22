use anyhow::{Result, bail};
use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, HANDLE};
use windows::Win32::System::Threading::CreateMutexW;
use windows::core::PCWSTR;

pub struct SingleInstance {
    handle: HANDLE,
}

impl SingleInstance {
    /// Returns Ok if this is the first instance.
    /// Returns Err if another instance is already running.
    pub fn acquire() -> Result<Self> {
        let name: Vec<u16> = "Local\\ChronographSingleInstance\0"
            .encode_utf16()
            .collect();

        let handle = unsafe {
            CreateMutexW(None, true, PCWSTR(name.as_ptr()))
        }?;

        // CreateMutexW sets last-error to ERROR_ALREADY_EXISTS when another
        // instance already holds the mutex, even if the call itself succeeds.
        let last_err = unsafe { windows::Win32::Foundation::GetLastError() };
        if last_err == ERROR_ALREADY_EXISTS {
            unsafe { CloseHandle(handle) }?;
            bail!("chronograph is already running");
        }

        Ok(Self { handle })
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        unsafe { let _ = CloseHandle(self.handle); }
    }
}
