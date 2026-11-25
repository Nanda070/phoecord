//! The Windows named pipe under the loader ↔ proxy IPC ([`crate::ipc`]).
//!
//! All the `unsafe` of the IPC is here, so that the proxy (which may not use `unsafe`) can serve
//! the pipe through this safe API. The server end is created with a security descriptor that
//! grants access to the current user's SID only (no `Everyone`, no `SYSTEM`, no `Administrators`),
//! refuses remote clients, and refuses to share the name with another server. The client end is a
//! plain `std` open of the pipe's path, with an anonymous impersonation level.

use std::{
    fs::{File, OpenOptions},
    io, mem,
    os::windows::{
        fs::OpenOptionsExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    ptr,
};
use windows_sys::Win32::{
    Foundation::{ERROR_PIPE_CONNECTED, HANDLE, INVALID_HANDLE_VALUE, LocalFree},
    Security::{
        Authorization::{
            ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
            SDDL_REVISION_1,
        },
        GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
        TokenUser,
    },
    Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX},
    System::{
        Pipes::{
            ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
        },
        Threading::{GetCurrentProcess, OpenProcessToken},
    },
};

/// Size of the pipe's in and out buffers.
const BUFFER: u32 = 64 * 1024;

/// `SECURITY_ANONYMOUS`: the server on the other end cannot impersonate us.
const SECURITY_ANONYMOUS: u32 = 0;

/// The current user's SID as a string (`S-1-5-21-…`).
pub fn current_user_sid() -> io::Result<String> {
    // SAFETY: every pointer passed is to a local that outlives the call. The token handle is
    // owned by an `OwnedHandle` straight away, so it is closed on every path. `TOKEN_USER` holds
    // a pointer, so its buffer is a `Vec<u64>` (8-byte aligned) of the size the first call asked
    // for; `User.Sid` points into that buffer and is only used while the buffer is alive. The
    // string `ConvertSidToStringSidW` allocates is NUL-terminated, copied out, then freed with
    // `LocalFree` as the API requires.
    unsafe {
        let mut token: HANDLE = ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(io::Error::last_os_error());
        }
        let token = OwnedHandle::from_raw_handle(token);
        let mut need = 0u32;
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            ptr::null_mut(),
            0,
            &mut need,
        );
        if need == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut buf = vec![0u64; (need as usize).div_ceil(8)];
        if GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            buf.as_mut_ptr().cast(),
            need,
            &mut need,
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        let user = &*(buf.as_ptr() as *const TOKEN_USER);
        let mut s: *mut u16 = ptr::null_mut();
        if ConvertSidToStringSidW(user.User.Sid, &mut s) == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut n = 0;
        while *s.add(n) != 0 {
            n += 1;
        }
        let sid = String::from_utf16_lossy(std::slice::from_raw_parts(s, n));
        LocalFree(s.cast());
        Ok(sid)
    }
}

/// The pipe path for the current user: `\\.\pipe\phoecord-<SID>`.
pub fn current_pipe_name() -> io::Result<String> {
    Ok(crate::ipc::pipe_name(&current_user_sid()?))
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

/// The server end of the pipe: hands out one connected [`File`] per client.
pub struct PipeServer {
    name: Vec<u16>,
    /// `D:P(A;;GA;;;<SID>)`: a protected DACL with one entry, generic-all for the user.
    sddl: Vec<u16>,
    first: bool,
    /// An instance already created and waiting for the next client, so that there is no moment
    /// without a listening instance between two clients.
    pending: Option<OwnedHandle>,
}

impl PipeServer {
    /// Creates the pipe `name` for the current user. Fails when a server already owns the name.
    pub fn bind(name: &str) -> io::Result<PipeServer> {
        let sid = current_user_sid()?;
        let mut s = PipeServer {
            name: wide(name),
            sddl: wide(&format!("D:P(A;;GA;;;{sid})")),
            first: true,
            pending: None,
        };
        s.pending = Some(s.create_instance()?);
        s.first = false;
        Ok(s)
    }

    fn create_instance(&self) -> io::Result<OwnedHandle> {
        // SAFETY: `sddl` and `name` are NUL-terminated and outlive the calls; `sd` is allocated by
        // the conversion call, used only for this `CreateNamedPipeW` (which copies it) and freed
        // right after with `LocalFree`; `sa` lives across the call. The returned handle is owned
        // by an `OwnedHandle` unless it is `INVALID_HANDLE_VALUE`.
        unsafe {
            let mut sd: PSECURITY_DESCRIPTOR = ptr::null_mut();
            if ConvertStringSecurityDescriptorToSecurityDescriptorW(
                self.sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut sd,
                ptr::null_mut(),
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
            let sa = SECURITY_ATTRIBUTES {
                nLength: mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: sd,
                bInheritHandle: 0,
            };
            let mut open = PIPE_ACCESS_DUPLEX;
            if self.first {
                open |= FILE_FLAG_FIRST_PIPE_INSTANCE;
            }
            let h = CreateNamedPipeW(
                self.name.as_ptr(),
                open,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_UNLIMITED_INSTANCES,
                BUFFER,
                BUFFER,
                0,
                &sa,
            );
            let err = io::Error::last_os_error();
            LocalFree(sd);
            if h == INVALID_HANDLE_VALUE {
                return Err(err);
            }
            Ok(OwnedHandle::from_raw_handle(h))
        }
    }

    /// Waits for a client and returns the connection. An error means this attempt failed (a
    /// client that vanished, say); the server is still usable, call again (after a pause when it
    /// keeps failing).
    pub fn accept(&mut self) -> io::Result<File> {
        let h = match self.pending.take() {
            Some(h) => h,
            None => self.create_instance()?,
        };
        // SAFETY: `h` is a valid server-end pipe handle owned by us; a null `OVERLAPPED` makes
        // the call block until a client connects.
        let ok = unsafe { ConnectNamedPipe(h.as_raw_handle(), ptr::null_mut()) };
        if ok == 0 {
            let e = io::Error::last_os_error();
            // The client connected between `CreateNamedPipeW` and here: that is a success.
            if e.raw_os_error() != Some(ERROR_PIPE_CONNECTED as i32) {
                self.pending = Some(h);
                return Err(e);
            }
        }
        self.pending = self.create_instance().ok();
        Ok(File::from(h))
    }
}

/// Opens the pipe as a client. Does not wait: no server gives `NotFound`, a server whose
/// instances are all busy gives `ERROR_PIPE_BUSY` (231); the caller retries.
pub fn connect(name: &str) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .security_qos_flags(SECURITY_ANONYMOUS)
        .open(name)
}

/// Sends `msgs` to the proxy in order over one new connection, then closes it.
pub fn send_all(name: &str, msgs: &[crate::ipc::Msg]) -> io::Result<()> {
    use std::io::Write;
    let mut f = connect(name)?;
    for m in msgs {
        let line = crate::ipc::encode_line(m).map_err(io::Error::other)?;
        f.write_all(&line)?;
    }
    f.flush()
}
