use anyhow::Result;
use auto_launch::AutoLaunchBuilder;

const APP_NAME: &str = "Chronograph";

fn launcher() -> Result<auto_launch::AutoLaunch> {
    let exe = std::env::current_exe()?;
    let al = AutoLaunchBuilder::new()
        .set_app_name(APP_NAME)
        .set_app_path(exe.to_str().unwrap_or_default())
        .set_args(&["--minimized"])
        .build()?;
    Ok(al)
}

pub fn is_enabled() -> bool {
    launcher()
        .and_then(|al| al.is_enabled().map_err(Into::into))
        .unwrap_or(false)
}

pub fn enable() -> Result<()> {
    launcher()?.enable().map_err(Into::into)
}

pub fn disable() -> Result<()> {
    launcher()?.disable().map_err(Into::into)
}
