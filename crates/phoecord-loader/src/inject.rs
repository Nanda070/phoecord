//! Launcher injection (task 2.1.7): `CreateProcess` suspended, `QueueUserAPC` to call
//! `LoadLibraryW` on the target's main thread before it runs any of its own code, then
//! `ResumeThread`. This is how Phoenix is reached: it does not statically or delay-import a DLL
//! this loader could sideload (`docs/research/phoenix-recon.md`, "Binary inspection"), and a
//! live-observed third-party tool uses exactly this method (`docs/ARCHITECTURE.md`, "Phoenix is
//! different").
//!
//! The classic APC-to-`LoadLibraryW` trick relies on `kernel32.dll` sitting at the same base
//! address in this process and the new one. That holds within one Windows boot session: ASLR
//! randomizes a system DLL's base once at boot, shared by every process that maps it, not
//! re-randomized per process. If that ever stopped holding, the APC would call garbage in the
//! target and the new thread would crash on resume — there is no success signal read back here
//! beyond whatever the injected DLL itself reports once it runs (`entry::report_to_proxy` over
//! the pipe, for `phoecord-loader.dll`).
//!
//! The small allocation `queue_load_library` makes in the target for the DLL path is never freed:
//! freeing it before the APC actually runs (asynchronous, after [`resume`]) would be a
//! use-after-free in someone else's process. A few hundred bytes leaked for the process's life is
//! the standard, accepted cost of this technique.

#[cfg(windows)]
mod imp {
    use std::{
        collections::BTreeMap,
        ffi::{OsStr, OsString},
        io, mem,
        os::windows::ffi::OsStrExt,
        path::Path,
        ptr,
    };
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE, PAPCFUNC},
        System::{
            Diagnostics::Debug::WriteProcessMemory,
            LibraryLoader::{GetModuleHandleW, GetProcAddress},
            Memory::{MEM_COMMIT, MEM_RESERVE, PAGE_READWRITE, VirtualAllocEx},
            Threading::{
                CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, CreateProcessW, PROCESS_INFORMATION,
                QueueUserAPC, ResumeThread, STARTUPINFOW,
            },
        },
    };

    /// A process started suspended: its main thread exists but has not run a single instruction
    /// of its own code. Dropping without calling [`resume`] leaves it suspended forever (the
    /// caller is expected to always resume or explicitly leak it; there is no "kill" helper here
    /// on purpose, this module only starts processes, never ends someone else's).
    pub struct Suspended {
        pid: u32,
        process: HANDLE,
        thread: HANDLE,
    }

    fn wide(s: &OsStr) -> Vec<u16> {
        s.encode_wide().chain([0]).collect()
    }

    /// Quotes one argument for a Win32 command line (MSVCRT/`CommandLineToArgvW` rules: double
    /// quotes and backslashes before them are escaped; a quote is added only if the argument is
    /// empty or has whitespace or a quote in it).
    fn quote_arg(arg: &str, out: &mut String) {
        let needs_quotes = arg.is_empty() || arg.contains([' ', '\t', '"']);
        if !needs_quotes {
            out.push_str(arg);
            return;
        }
        out.push('"');
        let mut backslashes = 0usize;
        for c in arg.chars() {
            match c {
                '\\' => {
                    backslashes += 1;
                    continue;
                }
                '"' => {
                    out.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                    out.push('"');
                    backslashes = 0;
                    continue;
                }
                _ => {
                    out.extend(std::iter::repeat_n('\\', backslashes));
                    backslashes = 0;
                }
            }
            out.push(c);
        }
        out.extend(std::iter::repeat_n('\\', backslashes * 2));
        out.push('"');
    }

    /// Builds `exe arg1 arg2 ...`, each argument quoted as Win32 expects.
    fn command_line(exe: &str, args: &[&str]) -> String {
        let mut line = String::new();
        quote_arg(exe, &mut line);
        for a in args {
            line.push(' ');
            quote_arg(a, &mut line);
        }
        line
    }

    /// The environment block for `CreateProcessW` with `CREATE_UNICODE_ENVIRONMENT`: `base` with
    /// every `extra` variable set over it (names compare case-insensitively, as Windows does),
    /// sorted by name, each `NAME=value` NUL-terminated, the block ended by one more NUL.
    fn env_block(
        base: impl IntoIterator<Item = (OsString, OsString)>,
        extra: &[(&str, &str)],
    ) -> Vec<u16> {
        let mut vars: BTreeMap<String, (OsString, OsString)> = BTreeMap::new();
        let key = |n: &OsStr| n.to_string_lossy().to_uppercase();
        for (n, v) in base {
            vars.insert(key(&n), (n, v));
        }
        for (n, v) in extra {
            vars.insert(key(OsStr::new(n)), (OsString::from(n), OsString::from(v)));
        }
        let mut block = Vec::new();
        for (n, v) in vars.values() {
            block.extend(n.encode_wide());
            block.push(u16::from(b'='));
            block.extend(v.encode_wide());
            block.push(0);
        }
        if block.is_empty() {
            block.push(0);
        }
        block.push(0);
        block
    }

    /// Starts `exe` suspended, with `args` after it on the command line (each quoted). The
    /// current directory is `exe`'s own folder, matching how the real launcher starts it.
    pub fn spawn_suspended(exe: &Path, args: &[&str]) -> io::Result<Suspended> {
        spawn(exe, args, None)
    }

    /// [`spawn_suspended`], with this process's environment plus `env` set over it: how
    /// `phoecord launch` hands Phoenix its proxy variables and the loader its transport
    /// (`crate::choice`).
    pub fn spawn_suspended_with_env(
        exe: &Path,
        args: &[&str],
        env: &[(&str, &str)],
    ) -> io::Result<Suspended> {
        spawn(exe, args, Some(env_block(std::env::vars_os(), env)))
    }

    fn spawn(exe: &Path, args: &[&str], env: Option<Vec<u16>>) -> io::Result<Suspended> {
        let exe_str = exe.to_str().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "exe path is not valid UTF-8")
        })?;
        let mut cmdline = wide(std::ffi::OsStr::new(&command_line(exe_str, args)));
        let app_name = wide(exe.as_os_str());
        let cwd = exe.parent().map(|p| wide(p.as_os_str()));

        let mut si: STARTUPINFOW = unsafe { mem::zeroed() };
        si.cb = mem::size_of::<STARTUPINFOW>() as u32;
        let mut pi: PROCESS_INFORMATION = unsafe { mem::zeroed() };
        let flags = if env.is_some() {
            CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT
        } else {
            CREATE_SUSPENDED
        };

        // SAFETY: `app_name` and `cmdline` are NUL-terminated wide strings kept alive for the
        // call; `cmdline`'s buffer is writable and large enough (`CreateProcessW` may rewrite it
        // in place, never grows it). `cwd` is NUL-terminated or null. `env` is null (inherit) or a
        // UTF-16 block built by `env_block` (every entry NUL-terminated, the block ended by an
        // extra NUL), matching `CREATE_UNICODE_ENVIRONMENT`, kept alive for the call. `si`/`pi`
        // are correctly sized, zeroed structs the call fills in. No handles are inherited
        // (`bInheritHandles = 0`), so this process's own open handles never leak into the target.
        let ok = unsafe {
            CreateProcessW(
                app_name.as_ptr(),
                cmdline.as_mut_ptr(),
                ptr::null(),
                ptr::null(),
                0, // bInheritHandles
                flags,
                env.as_ref().map_or(ptr::null(), |e| e.as_ptr().cast()),
                cwd.as_ref().map_or(ptr::null(), |c| c.as_ptr()),
                &si,
                &mut pi,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Suspended {
            pid: pi.dwProcessId,
            process: pi.hProcess,
            thread: pi.hThread,
        })
    }

    /// Writes `dll`'s path into the target and queues an APC on its (still suspended) main thread
    /// that will call `LoadLibraryW` on it once [`resume`] lets the thread run. The write never
    /// touches the suspended thread's own memory or registers: only process-wide address space
    /// the thread has not looked at yet.
    pub fn queue_load_library(p: &Suspended, dll: &Path) -> io::Result<()> {
        let path = wide(dll.as_os_str());
        let len = path.len() * 2; // UTF-16 code units -> bytes

        // SAFETY: `p.process` is a valid, still-open process handle from `spawn_suspended`.
        // `VirtualAllocEx` with `MEM_COMMIT | MEM_RESERVE` and `PAGE_READWRITE` returns a fresh,
        // writable region of at least `len` bytes in that process, or null on failure.
        let remote = unsafe {
            VirtualAllocEx(
                p.process,
                ptr::null(),
                len,
                MEM_COMMIT | MEM_RESERVE,
                PAGE_READWRITE,
            )
        };
        if remote.is_null() {
            return Err(io::Error::last_os_error());
        }

        // SAFETY: `remote` is exactly the region just allocated in `p.process`, large enough for
        // `len` bytes; `path.as_ptr()` is a local, valid buffer of `len` bytes for the duration of
        // this call. `written` is unused by the caller, a valid out-pointer is not required.
        let wrote = unsafe {
            WriteProcessMemory(
                p.process,
                remote,
                path.as_ptr().cast(),
                len,
                ptr::null_mut(),
            )
        };
        if wrote == 0 {
            return Err(io::Error::last_os_error());
        }

        // SAFETY: `kernel32.dll` is loaded (by the OS loader, before any user code runs) at the
        // same base address in every process for the life of this boot session; `GetProcAddress`
        // on our own copy of it therefore gives the same address `LoadLibraryW` has in `p.process`.
        // `b"LoadLibraryW\0"` is a NUL-terminated byte string, valid for the call.
        let load_library_w = unsafe {
            let kernel32 = GetModuleHandleW(wide(OsStr::new("kernel32.dll")).as_ptr());
            if kernel32.is_null() {
                return Err(io::Error::last_os_error());
            }
            GetProcAddress(kernel32, c"LoadLibraryW".as_ptr().cast())
        };
        if load_library_w.is_none() {
            return Err(io::Error::last_os_error());
        }

        // SAFETY: `p.thread` is the target's own (suspended) main thread, which has not resumed
        // yet, so queuing an APC on it is always valid. `load_library_w` (a `FARPROC`) and
        // `PAPCFUNC` are both a bare, optional `extern "system" fn` pointer of the same size;
        // reinterpreting one as the other changes no bits, only the number of arguments the
        // compiler believes it takes, and `LoadLibraryW(LPCWSTR)`'s single pointer-sized argument
        // is exactly what a `PAPCFUNC` callback receives on the Win64 calling convention. The
        // queued parameter, `remote`, stays valid in `p.process`'s address space (it is never
        // freed: see the module doc comment).
        let queued = unsafe {
            let apc: PAPCFUNC =
                mem::transmute::<windows_sys::Win32::Foundation::FARPROC, _>(load_library_w);
            QueueUserAPC(apc, p.thread, remote as usize)
        };
        if queued == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Lets the target's main thread run; its queued APC (if any) fires before any of its own
    /// code does. Returns the process id; the process and thread handles are closed here (closing
    /// a handle does not stop the process, it only drops this process's reference to it).
    pub fn resume(p: Suspended) -> io::Result<u32> {
        // SAFETY: `p.thread` is the valid, still-suspended thread handle from `spawn_suspended`,
        // used here for the last time before both handles are closed below.
        let prev = unsafe { ResumeThread(p.thread) };
        let err = if prev == u32::MAX {
            Some(io::Error::last_os_error())
        } else {
            None
        };
        // SAFETY: both handles came from the one `CreateProcessW` call in `spawn_suspended` and
        // are not used anywhere else in this type; closing them here, exactly once, is final.
        unsafe {
            CloseHandle(p.thread);
            CloseHandle(p.process);
        }
        match err {
            Some(e) => Err(e),
            None => Ok(p.pid),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn plain_argument_is_not_quoted() {
            let mut out = String::new();
            quote_arg("hello", &mut out);
            assert_eq!(out, "hello");
        }

        #[test]
        fn argument_with_space_is_quoted() {
            let mut out = String::new();
            quote_arg("hello world", &mut out);
            assert_eq!(out, "\"hello world\"");
        }

        #[test]
        fn empty_argument_is_quoted() {
            let mut out = String::new();
            quote_arg("", &mut out);
            assert_eq!(out, "\"\"");
        }

        #[test]
        fn a_lone_backslash_with_no_whitespace_needs_no_quoting() {
            // No space, tab or quote in the argument: CommandLineToArgvW does not treat a bare
            // backslash specially outside a quoted argument, so nothing here needs escaping.
            let mut out = String::new();
            quote_arg("a\\", &mut out);
            assert_eq!(out, "a\\");
        }

        #[test]
        fn trailing_backslashes_before_the_closing_quote_are_doubled() {
            // Quoted because of the space; the single backslash right before the closing quote
            // must become two, or the parser would read it as escaping that quote.
            let mut out = String::new();
            quote_arg("a \\", &mut out);
            let mut expected = String::from("\"a ");
            expected.push_str(&"\\".repeat(2));
            expected.push('"');
            assert_eq!(out, expected);
        }

        #[test]
        fn embedded_quote_is_escaped() {
            let mut out = String::new();
            quote_arg("a\"b", &mut out);
            assert_eq!(out, "\"a\\\"b\"");
        }

        #[test]
        fn backslash_before_quote_is_doubled_then_escaped() {
            // One real backslash followed by a literal quote needs 2*1+1 = 3 backslashes before
            // that quote (an even count would instead close the quoted argument).
            let mut out = String::new();
            quote_arg("a\\\"b", &mut out);
            let mut expected = String::from("\"a");
            expected.push_str(&"\\".repeat(3));
            expected.push('"');
            expected.push('b');
            expected.push('"');
            assert_eq!(out, expected);
        }

        #[test]
        fn the_environment_block_sets_over_the_base_and_is_sorted() {
            let base = [
                (OsString::from("Path"), OsString::from(r"C:\bin")),
                (OsString::from("https_proxy"), OsString::from("old")),
            ];
            let block = env_block(base, &[("HTTPS_PROXY", "http://127.0.0.1:1"), ("A", "1")]);
            let text = String::from_utf16(&block).unwrap();
            assert_eq!(
                text,
                "A=1\0HTTPS_PROXY=http://127.0.0.1:1\0Path=C:\\bin\0\0"
            );
            assert_eq!(env_block([], &[]), [0, 0]);
        }

        #[test]
        fn full_command_line_joins_with_spaces() {
            assert_eq!(
                command_line("C:\\x\\a.exe", &["--flag", "value with space"]),
                "C:\\x\\a.exe --flag \"value with space\""
            );
        }
    }
}

#[cfg(windows)]
pub use imp::{Suspended, queue_load_library, resume, spawn_suspended, spawn_suspended_with_env};
