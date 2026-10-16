//! Making a kernel kill legible.
//!
//! Android runs every app process behind a seccomp filter, and a system
//! call outside that filter is not refused with an error — the process is
//! shot where it stands with `SIGSYS`. All the shell says is
//!
//! ```text
//! [1]+  Bad system call  ./spiritchat-relay ...
//! ```
//!
//! which is indistinguishable, to anyone running this, from "the program
//! crashed". It isn't a crash: nothing in our code went wrong, the host
//! simply forbids a call something in the dependency tree made. The
//! difference matters, because the fix is in entirely different places.
//!
//! The kernel does say which call it was — `si_syscall` in the signal's
//! `siginfo` — it just goes nowhere unless someone is listening. So we
//! listen: catch `SIGSYS`, print the number (and the name, when it's one
//! we can name), then die exactly as we would have, with the same exit
//! status, so nothing downstream sees different behaviour.
//!
//! Everything here runs in a signal handler, so it is written to the
//! async-signal-safe subset: a stack buffer, one `write(2)`, atomics, and
//! no allocation anywhere.

/// Arms the reporter. Cheap, idempotent in effect, and a no-op on
/// platforms without seccomp — call it before anything else in `main`.
pub fn report_blocked_syscalls() {
    imp::install();
}

#[cfg(target_os = "linux")]
mod imp {
    use std::os::raw::{c_int, c_void};
    use std::sync::atomic::{AtomicU64, Ordering};

    /// One bit per system call number. A call blocked inside a hot loop
    /// would otherwise print the same line until the pipe filled; the
    /// first report is the whole story, so later ones are dropped.
    static REPORTED: [AtomicU64; 8] = [
        AtomicU64::new(0),
        AtomicU64::new(0),
        AtomicU64::new(0),
        AtomicU64::new(0),
        AtomicU64::new(0),
        AtomicU64::new(0),
        AtomicU64::new(0),
        AtomicU64::new(0),
    ];

    /// The `_sigsys` arm of `siginfo_t`, which the `libc` crate does not
    /// expose. The three leading `int`s are common to every `siginfo_t`;
    /// the union that follows is 8-byte aligned on LP64, hence the pad.
    #[repr(C)]
    struct SeccompSiginfo {
        _signo: c_int,
        _errno: c_int,
        _code: c_int,
        _pad: c_int,
        _call_addr: *mut c_void,
        syscall: c_int,
        _arch: u32,
    }

    pub fn install() {
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = report as *const () as usize;
            // SA_NODEFER so the re-raise below is delivered immediately
            // rather than left pending until the handler returns.
            action.sa_flags = libc::SA_SIGINFO | libc::SA_NODEFER;
            libc::sigemptyset(&mut action.sa_mask);
            libc::sigaction(libc::SIGSYS, &action, std::ptr::null_mut());
        }
    }

    unsafe extern "C" fn report(signal: c_int, info: *mut libc::siginfo_t, _context: *mut c_void) {
        if !info.is_null() {
            let info = &*(info as *const SeccompSiginfo);
            if first_report_of(info.syscall) {
                let mut line = [0u8; 224];
                let mut at = 0;
                at = append(
                    &mut line,
                    at,
                    b"[relay] the host's system call filter (seccomp) killed this process: call ",
                );
                at = append_number(&mut line, at, info.syscall);
                if let Some(name) = syscall_name(info.syscall) {
                    at = append(&mut line, at, b" (");
                    at = append(&mut line, at, name.as_bytes());
                    at = append(&mut line, at, b")");
                }
                at = append(&mut line, at, b" is not permitted here. This is the host\n");
                at = append(
                    &mut line,
                    at,
                    b"[relay] refusing the call, not a fault in the relay.\n",
                );
                libc::write(2, line.as_ptr() as *const c_void, at);
            }
        }

        // Die the way we would have without a handler: same signal, same
        // exit status, so a supervisor's restart policy is unaffected.
        libc::signal(libc::SIGSYS, libc::SIG_DFL);
        libc::raise(signal);
    }

    fn first_report_of(number: c_int) -> bool {
        if !(0..512).contains(&number) {
            return true;
        }
        let word = (number / 64) as usize;
        let bit = 1u64 << (number % 64);
        REPORTED[word].fetch_or(bit, Ordering::Relaxed) & bit == 0
    }

    /// Names for the calls plausibly missing from a restrictive filter —
    /// not the whole table, which would be pages of it for no gain. An
    /// unnamed number is still the answer; it just needs looking up.
    fn syscall_name(number: c_int) -> Option<&'static str> {
        // Everything from 424 on was allocated with the same number on
        // every architecture, so it needs no per-architecture table.
        let shared = match number {
            424 => "pidfd_send_signal",
            425 => "io_uring_setup",
            426 => "io_uring_enter",
            427 => "io_uring_register",
            434 => "pidfd_open",
            435 => "clone3",
            436 => "close_range",
            437 => "openat2",
            439 => "faccessat2",
            441 => "epoll_pwait2",
            442 => "mount_setattr",
            _ => "",
        };
        if !shared.is_empty() {
            return Some(shared);
        }

        #[cfg(target_arch = "aarch64")]
        let specific = match number {
            56 => "openat",
            78 => "readlinkat",
            79 => "newfstatat",
            198 => "socket",
            200 => "bind",
            203 => "connect",
            220 => "clone",
            233 => "madvise",
            261 => "prlimit64",
            278 => "getrandom",
            283 => "membarrier",
            291 => "statx",
            293 => "rseq",
            _ => "",
        };
        #[cfg(target_arch = "x86_64")]
        let specific = match number {
            9 => "mmap",
            10 => "mprotect",
            28 => "madvise",
            41 => "socket",
            42 => "connect",
            49 => "bind",
            56 => "clone",
            257 => "openat",
            262 => "newfstatat",
            302 => "prlimit64",
            318 => "getrandom",
            324 => "membarrier",
            332 => "statx",
            334 => "rseq",
            _ => "",
        };
        #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
        let specific = "";

        if specific.is_empty() {
            None
        } else {
            Some(specific)
        }
    }

    fn append(buffer: &mut [u8], at: usize, text: &[u8]) -> usize {
        let room = buffer.len().saturating_sub(at);
        let taken = text.len().min(room);
        buffer[at..at + taken].copy_from_slice(&text[..taken]);
        at + taken
    }

    fn append_number(buffer: &mut [u8], at: usize, value: c_int) -> usize {
        let mut digits = [0u8; 12];
        let mut written = 0;
        let negative = value < 0;
        // Through i64 so i32::MIN negates without overflowing.
        let mut remaining = (value as i64).unsigned_abs();
        if remaining == 0 {
            digits[0] = b'0';
            written = 1;
        }
        while remaining > 0 {
            digits[written] = b'0' + (remaining % 10) as u8;
            remaining /= 10;
            written += 1;
        }
        let mut at = if negative {
            append(buffer, at, b"-")
        } else {
            at
        };
        while written > 0 {
            written -= 1;
            at = append(buffer, at, &digits[written..written + 1]);
        }
        at
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    pub fn install() {}
}

#[cfg(all(test, target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")))]
mod tests {
    use std::os::raw::c_void;

    // The one thing that can silently be wrong here is the offset of
    // `si_syscall` inside `siginfo_t`, which we had to spell out by hand
    // because `libc` does not expose that arm of the union. Get it wrong
    // and the reporter still prints confidently — just a number from the
    // wrong four bytes, which is worse than printing nothing.
    //
    // So prove it end to end against the real kernel: forbid one
    // harmless call in a child process, make that call, and check the
    // child both dies the way it should and names the call it made.

    #[repr(C)]
    struct SockFilter {
        code: u16,
        jt: u8,
        jf: u8,
        k: u32,
    }

    #[repr(C)]
    struct SockFprog {
        len: u16,
        filter: *const SockFilter,
    }

    const SECCOMP_RET_TRAP: u32 = 0x0003_0000;
    const SECCOMP_RET_ALLOW: u32 = 0x7fff_0000;

    #[test]
    fn it_names_the_system_call_the_kernel_refused() {
        let forbidden = libc::SYS_getpid as u32;

        let mut pipe_ends = [0i32; 2];
        assert_eq!(unsafe { libc::pipe(pipe_ends.as_mut_ptr()) }, 0);
        let (read_end, write_end) = (pipe_ends[0], pipe_ends[1]);

        let child = unsafe { libc::fork() };
        assert!(child >= 0, "fork failed");

        if child == 0 {
            // Child. Nothing here allocates or takes a lock, which is the
            // rule for the gap between fork and exec in a threaded process.
            unsafe {
                libc::close(read_end);
                libc::dup2(write_end, 2);
                libc::close(write_end);

                super::report_blocked_syscalls();

                // Load the call number, trap on the forbidden one, allow
                // everything else. Offset 0 is `seccomp_data.nr`.
                let program = [
                    SockFilter { code: 0x20, jt: 0, jf: 0, k: 0 },
                    SockFilter { code: 0x15, jt: 0, jf: 1, k: forbidden },
                    SockFilter { code: 0x06, jt: 0, jf: 0, k: SECCOMP_RET_TRAP },
                    SockFilter { code: 0x06, jt: 0, jf: 0, k: SECCOMP_RET_ALLOW },
                ];
                let descriptor = SockFprog { len: 4, filter: program.as_ptr() };

                // PR_SET_NO_NEW_PRIVS, then PR_SET_SECCOMP in filter mode.
                libc::prctl(38, 1, 0, 0, 0);
                libc::prctl(22, 2, &descriptor as *const SockFprog as *const c_void);

                libc::getpid();
                libc::_exit(99); // not reached: the call above is fatal
            }
        }

        unsafe { libc::close(write_end) };

        let mut printed = Vec::new();
        let mut chunk = [0u8; 256];
        loop {
            let read = unsafe {
                libc::read(read_end, chunk.as_mut_ptr() as *mut c_void, chunk.len())
            };
            if read <= 0 {
                break;
            }
            printed.extend_from_slice(&chunk[..read as usize]);
        }
        unsafe { libc::close(read_end) };

        let mut status = 0i32;
        unsafe { libc::waitpid(child, &mut status, 0) };

        let printed = String::from_utf8_lossy(&printed);
        assert!(
            libc::WIFSIGNALED(status) && libc::WTERMSIG(status) == libc::SIGSYS,
            "child should still die of SIGSYS, exactly as it would unhandled; \
             status {status:#x}, it printed: {printed}"
        );
        assert!(
            printed.contains(&format!("call {forbidden} ")),
            "the reporter should name call {forbidden}, but printed: {printed}"
        );
    }
}
