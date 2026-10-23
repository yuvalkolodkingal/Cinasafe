//! Process-level protections (brief 10): a crash must not leave a dump of a
//! process that held keys, decrypted frames or titles on the host's disk.

/// Linux: RLIMIT_CORE = 1 stops both core files (smaller than any core) and
/// piped handlers such as systemd-coredump (the kernel skips the pipe when the
/// limit is exactly 1). The limit is inherited by WebKit's child processes.
/// `PR_SET_DUMPABLE = 0` also keeps other same-user processes from attaching
/// a debugger to this one.
#[cfg(target_os = "linux")]
pub fn no_crash_dumps() {
    let one = libc::rlimit { rlim_cur: 1, rlim_max: 1 };
    // SAFETY: plain syscalls with valid arguments.
    unsafe {
        libc::setrlimit(libc::RLIMIT_CORE, &one);
        libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0);
    }
}

/// Windows: Windows Error Reporting can write a dump of a crashed process.
/// Crashes (unhandled exceptions, including in libmpv) and panics end the
/// process at once instead, before WER sees them. WebView2's own processes
/// are outside our control (docs/THREAT_MODEL.md).
#[cfg(windows)]
pub fn no_crash_dumps() {
    use windows_sys::Win32::System::Diagnostics::Debug::{
        SetErrorMode, SetUnhandledExceptionFilter, EXCEPTION_POINTERS, SEM_FAILCRITICALERRORS, SEM_NOGPFAULTERRORBOX,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};

    unsafe extern "system" fn on_crash(_: *const EXCEPTION_POINTERS) -> i32 {
        unsafe { TerminateProcess(GetCurrentProcess(), 0xC000_0409) };
        0
    }
    // SAFETY: process-wide settings with valid arguments; the filter only terminates.
    unsafe {
        SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX);
        SetUnhandledExceptionFilter(Some(on_crash));
    }
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default(info);
        // panic = "abort" would otherwise end in __fastfail, which goes to WER.
        unsafe { TerminateProcess(GetCurrentProcess(), 101) };
    }));
}

#[cfg(not(any(target_os = "linux", windows)))]
pub fn no_crash_dumps() {}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    #[test]
    fn core_limit_is_one() {
        // Runs in a child process so the test binary keeps its own limits.
        if std::env::var_os("CINASAFE_HARDENING_CHILD").is_some() {
            super::no_crash_dumps();
            let mut r = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
            unsafe { libc::getrlimit(libc::RLIMIT_CORE, &mut r) };
            assert_eq!((r.rlim_cur, r.rlim_max), (1, 1));
            assert_eq!(unsafe { libc::prctl(libc::PR_GET_DUMPABLE) }, 0);
            return;
        }
        let exe = std::env::current_exe().unwrap();
        let status = std::process::Command::new(exe)
            .args(["--exact", "hardening::tests::core_limit_is_one", "--nocapture"])
            .env("CINASAFE_HARDENING_CHILD", "1")
            .status()
            .unwrap();
        assert!(status.success());
    }
}
