//! Development-session shutdown without interrupting capture persistence.
use std::sync::atomic::{AtomicBool, Ordering};
static STOP: AtomicBool = AtomicBool::new(false);

pub fn install() {
    if std::env::var_os("LENNETTE_DEV_LEASE").is_none() {
        return;
    }
    #[cfg(windows)]
    unsafe {
        windows_sys::Win32::System::Console::SetConsoleCtrlHandler(Some(console_signal), 1);
    }
}
#[cfg(windows)]
unsafe extern "system" fn console_signal(event: u32) -> i32 {
    if event <= 2 {
        STOP.store(true, Ordering::Release);
        1
    } else {
        0
    }
}
pub fn should_stop() -> bool {
    if STOP.load(Ordering::Acquire) {
        return true;
    }
    let Ok(path) = std::env::var("LENNETTE_DEV_LEASE") else {
        return false;
    };
    !std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age.as_secs() < 8)
}
