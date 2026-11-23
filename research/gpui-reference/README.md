# gpui-reference

A small GPUI app used to study what GPUI leaves in a release binary (PhoeCord stage 1.6.2). It is **not** a workspace member and its binary is **not** committed.

- **GPUI:** `zed-industries/zed` at commit `f126950f9d3b62666f97437ae5d4ccac2272c4db` (2026-11-22), crates `gpui` and `gpui_platform`, pinned by `rev` in `Cargo.toml` and by `Cargo.lock`.
- **What it shows:** a `Theme` global read through an `ActiveTheme`-style accessor (the same pattern as Zed's `GlobalTheme`), a tab list (`Messages`, `Plugins`, `About`), and a `uniform_list` of message rows with a click-to-switch dark/light theme.
- **Profile:** `lto = "fat"`, `codegen-units = 1`, `strip = true`, `panic = "unwind"`, `opt-level = 3`.
- **Toolchain used:** rustc 1.99.1, `x86_64-pc-windows-msvc`, Windows 11, Visual Studio 2022 Community build tools. First build downloads the Zed git checkout (large) and takes several minutes; later builds about 1 minute.

```powershell
cd research/gpui-reference
cargo build --release
# result: target/release/gpui-reference.exe  (about 8.7 MB, ignored by git)
```

To inspect it: see [`../aob-probe/`](../aob-probe/) and [`docs/research/gpui.md`](../../docs/research/gpui.md), section "GPUI in a release binary".

Functions marked `#[inline(never)]` in `src/main.rs` (`theme`, `render_tab_list`, `render_message_row`, `render_messages`) exist only so each stays a separate symbol-less function in the binary; they do not change which strings survive.
