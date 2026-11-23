//! Throwaway host for manual checks of the proxy DLL; it stands in for the Discord client.
//!
//! Copy the built `phoecord_loader.dll` next to this exe as `version.dll`, then run it: it loads
//! `version.dll` from its own folder (full path), calls a forwarded export (and asserts it works), waits for the worker
//! to write its log and exits 0 on success. Set `LOCALAPPDATA` to a scratch folder to keep the log
//! out of your profile.

#[cfg(windows)]
fn main() {
    use std::{os::windows::ffi::OsStrExt, time::Duration};
    use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

    let exe = std::env::current_exe().expect("exe path");
    let dll = exe.with_file_name("version.dll");
    let wide: Vec<u16> = dll.as_os_str().encode_wide().chain([0]).collect();
    // SAFETY: `wide` is a NUL-terminated path that outlives the call.
    let module = unsafe { LoadLibraryW(wide.as_ptr()) };
    assert!(!module.is_null(), "cannot load {}", dll.display());

    // SAFETY: NUL-terminated export name; the signature below is that of GetFileVersionInfoSizeW.
    let f = unsafe { GetProcAddress(module, c"GetFileVersionInfoSizeW".as_ptr().cast()) }
        .expect("export missing");
    let f: extern "system" fn(*const u16, *mut u32) -> u32 =
        // SAFETY: see above.
        unsafe { std::mem::transmute(f) };
    // Ask for the version block of kernel32.dll, which always has one.
    let k32 = std::path::PathBuf::from(std::env::var_os("SystemRoot").expect("SystemRoot"))
        .join("System32")
        .join("kernel32.dll");
    let k32: Vec<u16> = k32.as_os_str().encode_wide().chain([0]).collect();
    let mut handle = 0u32;
    let size = f(k32.as_ptr(), &mut handle);
    println!("GetFileVersionInfoSizeW(kernel32.dll) = {size}");
    assert!(size > 0, "forwarded call failed");

    // Give the worker thread time to write its log lines, then exit normally.
    std::thread::sleep(Duration::from_millis(1000));
    println!("host finished normally");
}

#[cfg(not(windows))]
fn main() {}
