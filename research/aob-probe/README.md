# aob-probe

Throwaway tool for stage 1.6.3. Reads a PE file (never a process), finds strings, `core::panic::Location` structs and `rip`-relative references to them in `.text`, drafts AOB signatures around each reference and verifies them with `phoecord_loader::aob::Pattern`, so a signature that passes here behaves the same in the loader. Only point it at your own builds (such as `../gpui-reference`).

```powershell
cd research/aob-probe
cargo build --release
$E = "../gpui-reference/target/release/gpui-reference.exe"
./target/release/aob-probe strings $E "gpui_reference::Theme"      # string offsets (RVA)
./target/release/aob-probe sig     $E "gpui_reference::Theme" 16 16 # xrefs + drafted, verified signatures
./target/release/aob-probe locsig  $E "elements\uniform_list.rs" 370 24 24   # panic Location by file[:line]
./target/release/aob-probe ptrsig  $E "Plugins" 24 24              # static &str tables
./target/release/aob-probe check   $E "3D FF FF 00 00 75 4E ?? ??"  # count matches in .text
```

Limits: x86-64 only, no disassembler (it wildcards the rel32 of `call`, `jmp`, `jcc`, `lea`/`mov` rip-relative and SSE rip-relative loads, and nothing else), so drafted signatures still need a human look at the surrounding code.
