//! Generates the export-forwarding stubs of the proxy DLL from `exports.txt` (one export name of
//! the system `version.dll` per line; the Windows-only test `exports_match_system_dll` checks the
//! list against the real DLL). Output: `$OUT_DIR/forwarders.rs`, included by `src/proxy_dll.rs`.
use std::{env, fmt::Write as _, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=exports.txt");
    println!("cargo:rerun-if-changed=build.rs");
    let list = fs::read_to_string("exports.txt").expect("exports.txt");
    let names: Vec<&str> = list
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();

    let mut out = String::new();
    writeln!(out, "pub const EXPORT_NAMES: [&str; {}] = [", names.len()).unwrap();
    for n in &names {
        writeln!(out, "    {n:?},").unwrap();
    }
    out.push_str("];\n\n");

    // One stub per export. A filled table slot is a plain `jmp`; an empty slot calls the resolver
    // (which loads the real DLL and fills the slot) with the integer argument registers preserved.
    // The version APIs take no floating-point arguments, so xmm registers are not saved.
    out.push_str("#[cfg(all(windows, target_arch = \"x86_64\"))]\ncore::arch::global_asm!(\n");
    for (i, n) in names.iter().enumerate() {
        let stub = format!(
            ".globl {n}\n{n}:\n    mov rax, qword ptr [rip + {{table}} + {off}]\n    test rax, rax\n    jz 2f\n    jmp rax\n2:\n    push rcx\n    push rdx\n    push r8\n    push r9\n    sub rsp, 40\n    mov ecx, {i}\n    call {{resolve}}\n    add rsp, 40\n    pop r9\n    pop r8\n    pop rdx\n    pop rcx\n    jmp rax\n",
            off = i * 8
        );
        writeln!(out, "    {stub:?},").unwrap();
    }
    out.push_str("    table = sym FORWARD_TABLE,\n    resolve = sym resolve_export,\n);\n");
    fs::write(
        PathBuf::from(env::var("OUT_DIR").unwrap()).join("forwarders.rs"),
        out,
    )
    .unwrap();

    // Export the stubs from the cdylib (MSVC only; global_asm symbols are not exported by default).
    // Only with the `dll` feature: without it the stubs are not compiled in (the proxy uses this
    // crate for the IPC alone) and exporting them would not link.
    if env::var_os("CARGO_FEATURE_DLL").is_some()
        && env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
        && env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("x86_64")
    {
        for n in &names {
            println!("cargo:rustc-cdylib-link-arg=/EXPORT:{n}");
        }
    }
}
