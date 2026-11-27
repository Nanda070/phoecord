Checked against code: not applicable (the only PhoeCord code that touches Phoenix is `discover` and `launch` for 2.1.1, which start it and read its version, nothing more); against a local install of Discord Phoenix, file version `0.1.0.2208`, read 2026-11-25/26 (part 1 includes a static pass of the installed binary on 2026-11-26). Part 1's exact addresses and structure offsets are kept in the team's private recon notes, not in this file (ask Nanda for access); the byte signatures PhoeCord uses are public in `signatures/phoenix.toml` since 2026-11-27 (2.2.10); this file keeps everything that is useful without them, plus all of part 2.

# Desktop Phoenix reconnaissance (stage 2.1)

Findings about an official Phoenix build, and the steps that produce them. Part 1 holds tasks **2.1.1** (discovery and update scheme), **2.1.2** (binary inspection), **2.1.5** and **2.1.6** (allocator, HTTP transport, gateway dispatcher); part 2 holds **2.1.3** (proxy behavior), **2.1.4** (gateway wire format) and **2.3.7** (degradation). Task 2.1.9 completes the report: loading path and the decisions for 2.2–2.4.

**Status of part 2: partly run (2026-11-25 through 2026-11-27, Windows).** Proxy behavior, the gateway encoding and compression, the gateway URL, the fixtures, the event comparison with an Electron client and the fresh-install check of 2.1.8 have Windows results. The three degradation scenarios pass. Uploads, voice and media-while-scrolling ran on 2026-11-27 (past the account screen via sunbird's patch, see Step 5). Linux is "not run". What an unrun step expects is a hypothesis, not a finding.

**The account used for the 2026-11-25 runs was not accepted by this client** ("This app isn't available for your account" after `READY`); nothing past that screen was seen then, and no way past it was tried. **On 2026-11-26 the same account was accepted** (the window opened on the friends list, with no change on our side), so the runs of that day see the signed-in client. What the study of another loader says about Phoenix is in [`phoenix-desktop.md`](phoenix-desktop.md).

# Part 1. The binary and the loading path (2.1.1, 2.1.2, 2.1.5–2.1.7)

Facts in part 1 come from a local install, read with read-only tools (`phoecord dev inspect-pe`, PowerShell, `Get-AuthenticodeSignature`, resource APIs used only to read the manifest) and a static pass over the installed binary (headless Ghidra on a private project, never run against Phoenix). No sunbird code or binaries are reproduced (see [`phoenix-desktop.md`](phoenix-desktop.md) §0). **Exact addresses, byte signatures and structure offsets from this static pass are not published here**: they are reverse-engineering detail about a specific shipped build and live in the team's private recon notes instead (consistent with [`docs/AGENTS.md`](../AGENTS.md), "Never commit … disassembly" — the same rule that keeps `local_dumps/transport_dump.txt` gitignored). What follows is everything useful without them.

## Binary inspection (2.1.2)

**Target:** `C:\Users\<user>\AppData\Local\DiscordPhoenix\app\Discord.exe`, 518,388,664 bytes, `FileVersion`/`ProductVersion` `0.1.0.2208`, `ProductName` "Discord Phoenix", signed by Discord Inc. (valid Authenticode signature; so are every other `.exe`/`.dll` directly under `app\`).

**PE headers.** PE32+ (x86-64), Windows GUI subsystem. No .NET (CLR) metadata — confirms a native (Rust) client, not Electron/.NET. `DllCharacteristics = 0xc160`: `HIGH_ENTROPY_VA | DYNAMIC_BASE | NX_COMPAT | GUARD_CF | TERMINAL_SERVER_AWARE`. `GUARD_CF` means Control Flow Guard is on — an inline detour must still produce valid CFG-reachable targets or the hooked thread's indirect calls through hooked pointers can crash under CFG bitmap enforcement; this needs to be verified in 2.1.5/2.1.6 when the actual detour is written.

**Import table — 50 DLLs, 0 delay-loaded.** Of the sideload candidates from 2.1.2's checklist, five are imported directly (not delay-loaded):

| DLL | Functions imported |
|:---|:---|
| `version.dll` | 3 |
| `winhttp.dll` | 5 |
| `winmm.dll` | 2 |
| `dbghelp.dll` | 1 |
| `dwmapi.dll` | 5 |

`wininet.dll` is **not** imported (Phoenix's HTTP stack does not go through WinINet — consistent with an in-process Rust HTTP client rather than the system one). The rest of the import table is the expected native-Windows set: `kernel32.dll` (326 functions — the bulk of the surface), `ws2_32.dll`, `user32.dll`, `advapi32.dll`, `crypt32.dll`/`wintrust.dll` (certificate and signature checks), `d3d11.dll`/`d3d12.dll`/`dxgi.dll`/`dwrite.dll` (GPUI's rendering backend), `webauthn.dll`, several `api-ms-win-core-*.dll` forwarders, and `icuuc.dll` (ICU, text layout/i18n). No `wininet.dll`, no .NET, no sunbird-specific name was found; this matches a self-contained Rust binary.

**DLL search restrictions.** `kernel32.dll` imports include `LoadLibraryA/W`, `LoadLibraryExA/W` **and `SetDefaultDllDirectories`** — so the process does call `SetDefaultDllDirectories` at some point (the import alone doesn't say with which flags or how early; that needs a disassembly-level check at the call site, which this read-only pass does not do). The Load Configuration directory's `DependentLoadFlags` is **0** — the image itself requests no `LOAD_LIBRARY_SEARCH_*` restriction at the PE level. The embedded manifest (resource `RT_MANIFEST`, id 1; id 2 absent) carries `requestedExecutionLevel asInvoker`, DPI/heap settings, and one `<dependency>` on `Microsoft.Windows.Common-Controls` 6.0.0.0 — **no `<dllDirectory>` element**. So: nothing at the PE-header or manifest level restricts `Discord.exe`'s DLL search path; any restriction, if present, is applied at run time through the `SetDefaultDllDirectories` call kernel32 exposes being imported. Whether that call actually narrows the search before `version.dll` would be probed is an open question for 2.1.5 (needs either a debugger breakpoint on `SetDefaultDllDirectories` or an empirical test: drop a stub `version.dll` next to `Discord.exe` and see if it loads).

**TLS and sections.** One TLS directory with 4 callbacks (typical of Rust's TLS-based thread-local destructors/initializers, not evidence of anti-tamper). 15 sections; besides the usual `.text`/`.rdata`/`.data`/`.pdata`/`.reloc`, there are Rust/LLVM-specific sections `.eh_fram`(e), `.fptable`, `.itt_not`(ify, Intel VTune instrumentation — a debug/profiling leftover, not stripped from this build), `.xdata`, and two Intel-IPP-library sections `IPPCODE`/`IPPDATA` (`r-x`/`rw-` — a statically linked codec or crypto library using Intel's Integrated Performance Primitives). The full section table (virtual addresses, raw offsets and sizes), which is the base for AOB scans in 2.1.5/2.1.6/2.1.7, is in the private recon notes.

**Install layout** (`%LOCALAPPDATA%\DiscordPhoenix`):

```
DiscordPhoenix/
  app/                          current build, unversioned folder (no per-version subfolder like Electron Discord's app-<ver>)
    Discord.exe                 main binary (518 MB)
    "Discord Phoenix Crash Handler.exe"
    DiscordHook.dll, DiscordHook64.dll          (named like Electron Discord's low-level keyboard/mouse hook DLLs)
    DiscordHookHelper.exe, DiscordHookHelper64.exe
    DiscordPhoenixSetup.exe     installer/updater binary, also the uninstall entry point
    DiscordSystemHelper.exe     registered as the Windows service "DiscordSystemHelperPhoenix" (stopped by default)
    discord_media.exe, video-codec-probe.exe, discord_wer.dll
    krisp-*.kef                 Krisp voice-processing models (noise cancel / background voice cancel), shipped flat, not as a module
    modules/
      discord_clips-0/discord_clips/   one versioned module today: discord_clips.exe + its own onnxruntime.dll, msvcp140*.dll, vcruntime140*.dll
  modules/state.json            { "modules": {} } — empty; module state tracked here is separate from the on-disk modules/ folder above
  sunbird/
    debug-shell-token           32-byte token file (local dev/debug hook — name matches sunbird's own debug shell, confirming Phoenix is sunbird-derived)
    plugins/                    empty on this install
  app.lock, app.port, device_prefs.json(.lock), session.json, accounts.json, perf-run.json
  cache/, clips/, crashes/, logs/, store/, voice-data/, webview/
```

**Update scheme.** Unlike Electron Discord/Canary (`%LOCALAPPDATA%\Discord\app-<version>\`, updated by `Update.exe --processStart`, with the install registered to run `Update.exe`), Phoenix installs flat as `app\` with no version suffix in the path, and the registry uninstall entry (`HKCU\...\Uninstall\DiscordPhoenix`) points at `DiscordPhoenixSetup.exe --uninstall` directly rather than at a separate `Update.exe`. The per-module `modules/discord_clips-0/` naming (`<module>-<ver>`) suggests modules update independently of the main `app\Discord.exe`, tracked through `modules\state.json`; that file is empty on this install, so the mechanism that populates it during an actual update was not observed here. `DiscordSystemHelperPhoenix` is a separate installed Windows service (`C:\Program Files\Common Files\Discord\DiscordPhoenix\DiscordSystemHelper.exe service --channel phoenix`), parallel to the existing `DiscordSystemHelper` for Electron Discord — this is outside `%LOCALAPPDATA%` and will need its own entry in the installer's discovery/uninstall logic (2.2.6, 2.2.8).

**Answers to the task's "Done when" questions:**
- *Can `version.dll` sideload?* Not ruled out by anything in the PE header or manifest (`DependentLoadFlags = 0`, no `<dllDirectory>`). The binary does call `SetDefaultDllDirectories`, so an empirical sideload test (drop a stub `version.dll` next to `Discord.exe`, see if its `DllMain` runs) is still needed before 2.1.7 decides sideload vs. launcher — tracked as an open question below.
- *Which DLLs are imported?* 50, listed above; `version.dll`, `winhttp.dll`, `winmm.dll`, `dbghelp.dll`, `dwmapi.dll` present, `wininet.dll` absent, none delay-loaded.
- *What restrictions exist?* None recorded in the PE header (`DependentLoadFlags = 0`) or the manifest; whatever restriction exists (if any) is applied at run time via `SetDefaultDllDirectories`, not declared statically.

## Discovery and update scheme (2.1.1)

Everything here was read from the file system, the registry and the logs of the local install (Phoenix `0.1.0.2208`, Windows 11); nothing was started for it. Paths are written with `%LOCALAPPDATA%` instead of the user's profile folder.

**Where Phoenix is, and what `phoecord` does with it** (`discover`, `phoecord paths`, `phoecord launch --channel phoenix`):

| What | Where | Used by |
|:---|:---|:---|
| Client folder and executable | `%LOCALAPPDATA%\DiscordPhoenix\app\Discord.exe` | `discover` (`Channel::Phoenix`, `Client.root` = `app`); `launch` starts this file from this folder |
| Version | the `FileVersion` of `Discord.exe`, `0.1.0.2208` (read from the PE version resource with seeks, the 518 MB file is never loaded or run: `phoecord_installer::pe_version`) | `discover`, `phoecord paths` |
| Uninstall entry | `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\DiscordPhoenix`: `InstallLocation` = the `app` folder, `DisplayVersion` = `0.1.0.2208`, `UninstallString` = `app\DiscordPhoenixSetup.exe --uninstall` | not used; the file system is enough and also covers a copy that was never registered. A possible cross-check for 2.2.8 |
| Start-menu shortcut | `Discord Phoenix.lnk` in the user's Programs folder: target `app\Discord.exe`, no arguments, working directory `app` | `launch` copies the working directory |
| Autostart | none: the Run key holds entries for Discord and Discord Canary, not for Phoenix | |
| Data | `%LOCALAPPDATA%\DiscordPhoenix\` next to `app\` (`cache`, `logs`, `store`, `accounts.json`, `session.json`, `app.lock`, `app.port`, …; an `updates` folder is named in the installer's strings but absent here); more in the layout above | never read by PhoeCord; `logs\setup.log` and `logs\libdiscord.log` were read for this note |
| System helper (Windows service `DiscordSystemHelperPhoenix`) | `%ProgramFiles%\Common Files\Discord\DiscordPhoenix\DiscordSystemHelper.exe`, with its own `logs\` | not a client; not looked for. Installer and uninstaller (2.2.6, 2.2.8) have to know it exists |

The sum: one fixed folder, no `app-x.y.z` folders, no `Update.exe`. That is why `launch` runs the executable directly instead of through Squirrel's `Update.exe --processStart`, and why the version comes from the executable instead of the folder name. `--proxy-server` and the system proxy are what 2.1.3 found Phoenix honours, so `launch` passes the same `--proxy-server=http://<listen>` flag as for the other channels (and sets `HTTPS_PROXY`, which Phoenix ignores, for a client that does honour it). Phoenix keeps one instance (`app.lock`): a second start hands over to the running one, which keeps its old proxy settings, so quit Phoenix completely before `phoecord launch --channel phoenix`.

**Update scheme: in-place replacement of `app\`, not versioned folders; inferred, not observed.** No Phoenix update has happened on this install, so none of this is a recording of one. The evidence:

- `logs\setup.log` of the install: `install: DiscordPhoenixSetup.exe into …\DiscordPhoenix\app`, `installing version 0.1.0.2208 over no install`, `moved the build into place`. The build goes into `app` itself, and "over no install" implies an "over an existing install" case: the next build is installed over the same folder.
- `DiscordPhoenixSetup.exe` is copied into `app\` and is the uninstall entry. It is built from a `discord-desktop-updater` crate (source paths in its strings: `setup\payload.rs`, `signature.rs`, `swap_helper.rs`) and takes `--payload`, `--silent`, `--uninstall`, `--uninstall --remove-data`, `--channel phoenix`, `--pid` and `--phoenix-setup-quitask`; its strings also name `rollback`. A swap helper and a rollback point to "stage the new build, swap it in, keep the old one for a rollback"; the staging folder is probably the `updates` name in its folder list (a guess from one string).
- The client itself logs `discord_desktop_platform::updater::native` with `on: true, phoenix: true` at every start and reports `update_channel: release/phoenix_desktop`; so the updater runs inside the client, not as a separate Squirrel `Update.exe`.
- Modules are separate: `app\modules\<name>-<n>\…` (today `discord_clips-0`, logged as `module_dir`), tracked by `DiscordPhoenix\modules\state.json` (empty here), so modules can change without the main executable changing.

What this means for PhoeCord:

1. Discovery needs no knowledge of the update: the path is fixed and the version is read from the executable on every call, so an in-place update shows up as a new version number the next time `phoecord paths` or `launch` runs. A per-build signature set (keyed by version and `.text` hash) notices the new build the same way.
2. Anything PhoeCord puts inside `app\` (the `version.dll` loader of the sideload model) can be replaced or deleted when a build is swapped in. Launcher injection (task 2.1.7, ruling 2026-11-26) puts nothing in `app\`, so it does not have this problem. Only the installer (2.2.8) has to decide whether it wants anything to survive an update.
3. If a later Phoenix moves to versioned folders (`app-x.y.z` or similar), `phoenix_at` in `discover.rs` is the one place to change.

**Not known:**

- Linux and macOS. The plan names `~/.config/discord-phoenix` and Flatpak, and [`phoenix-desktop.md`](phoenix-desktop.md) names `discord-phoenix` on Linux, but no Linux or macOS Phoenix was available, so neither the executable's location nor its layout is observed. `discover` looks for nothing there; `phoecord launch -- <path to the executable>` still works with an explicit path.
- A real update: the contents of `app\` and of the `updates` folder before and after, the new `setup.log` lines, and whether a running client is replaced or the swap waits for it to quit. Worth one recording the first time Phoenix updates on the owner's machine.
- Whether Phoenix is also installed system-wide anywhere (only the per-user install and its Program Files service helper were found).

## Allocator, HTTP transport and Gateway dispatcher — findings, no addresses (2.1.5, 2.1.6)

Two static passes were made over the installed binary (2026-11-25 sliced on gateway strings; 2026-11-26 wider, with a working cross-reference index). Nothing was run inside Phoenix, so none of this is confirmed at run time. The addresses, byte signatures (for `signatures/*.toml`) and exact structure offsets behind every bullet below are in the team's private recon notes, kept there because they are reverse-engineering detail about one specific shipped build (not source code, not something that should sit in a public history of a project that also tells contributors never to commit disassembly).

- **Allocator (2.1.5).** The host's `__rust_alloc` and `handle_alloc_error` shims were located from the fallible-allocation call idiom, confirmed at two independent call sites. `__rust_dealloc` was also located on the second pass (its call-site fan-in is far larger than the allocator's own, as expected). A masked, build-specific call-site signature exists for the allocator entry; the shim entries themselves have no string cross-reference, so the robust discovery is "the function most other functions call but which itself never calls back", not a fixed byte window. This confirms the 2.2.2 fallback (local reply instead of body replacement) is the right default until the shims are pinned per build.
- **HTTP transport (2.1.5).** The first pass's dump did not contain the transport at all (it was sliced on gateway strings). The second pass located the `send` entry of the `reqwest`-based transport, its vtable, the response future's `poll` slot, and the in-memory layout of the request it builds (headers, body, URL, timeout, method field inferred). Each has a masked signature that matches uniquely in this build's `.text`, with enough leading whole instructions for an inline detour; none is checked on a second build yet, so none is in `signatures/` (2.2.9 blocks that). The response layout (status, headers, body) that `on_response` needs is still open, and so is everything at run time — nothing here has been exercised inside the client.
- **Gateway dispatcher (2.1.6).** The inbound per-event dispatch functions share one ABI: a context/out-pointer, the event name as a pointer+length, and a payload handle, with the result packed as a niche-optimized small enum rather than a two-register `Result`. A registry build/query call was found and anchored, though the task's "done when" (the registry table's exact address and entry layout) is not fully met yet. The **second pass additionally found an outgoing (client→server) send path** — this answers plan open question 3 and gives 2.2.4 something concrete to hook — and opcodes consistent with known Gateway operations (presence, voice state, guild member requests, and others); heartbeat, identify and resume are not produced by it, so where those are sent is still open. The client's connection loop treats the `READY` event name specially in four places; whether its payload also still goes through the ordinary event registry, or only this special path, is unverified.

**Open for 2.1.6, carried to the next pass:** the registry table's address and entry layout.
**Open for 2.1.5, carried to 2.2.2:** the response layout, and everything at run time (Control Flow Guard enforcement on a real detour has not been tested).

## Loading path (2.1.7)

**Decision: launcher injection, not `version.dll` sideload** (ruling, 2026-11-26; `docs/DECISIONS.md`). `phoecord dev inject <exe>` (`crates/phoecord-loader/src/inject.rs`) starts the client with `CreateProcess` suspended, queues an APC on its main thread that calls `LoadLibraryW` on PhoeCord's DLL, then resumes it. The DLL is in before the client runs any of its own code, and nothing is placed in `app\`.

**What was run.** First on a throwaway `cmd.exe`, then on the real Phoenix `0.1.0.2208`, with PhoeCord's own loader. `version.dll` stayed the real one from `System32`. `DllMain` and the worker thread ran, the worker detected the build without any RVA (a new `.text` SHA-256 for `0.1.0.2208`), found no signature table for it and left the UI layer off. The client kept running, no crash. Before that, a third-party tool already on the owner's machine had shown the same method on Phoenix in 858 ms (that figure is the tool's, not a bound for PhoeCord's loader).

**What it means.**

- The in-place update of `app\` (see 2.1.1) cannot remove PhoeCord: nothing of PhoeCord lives there. After an update only the check cache of 2.2.8 has to notice the new build.
- Phoenix is PhoeCord-modded only when started through `phoecord launch --channel phoenix` (2.2.8: a separate shortcut). Started from its own shortcut, it runs without PhoeCord.
- Hook timing: the DLL loads before the first instruction of the client, so the "too late from a worker thread" risk of the plan is answered for loading. Whether the worker installs hooks before the first registry read is a 2.2.2/2.2.3 question, since no hook exists yet.
- Sideload itself was never tested: `version.dll` is in the import table (above), but no stub was ever placed next to `Discord.exe`. The comment at the top of `inject.rs` says Phoenix imports no DLL the loader could sideload; the import table says otherwise, so the honest state is "not tested", not "impossible".

# Part 2. Proxy, gateway, degradation (2.1.3, 2.1.4, 2.3.7)

## Before you start

- **Risk.** These steps run a modified network path against a live account. Client modification and traffic interception break Discord's Terms ([`TERMS.md`](../../TERMS.md)). Use an account you can afford to lose.
- **Secrets.** Captured traffic contains the account token and personal data. Keep captures in a folder outside the repository and delete them when the findings are written down.
- **Trusted certificates.** The mitmproxy CA, unlike PhoeCord's, has no name constraint: while it is trusted, whoever holds its key can impersonate any site to you. Remove it as soon as the CA steps are done.
- **One client.** Quit Discord completely (tray icon too) before every launch. A second start hands over to the running process, which keeps its old environment.

Tools: a built `phoecord`, [mitmproxy](https://mitmproxy.org) (`winget install mitmproxy.mitmproxy`), and for the Linux steps `ss`.

Record the build first. With the loader installed, `%LOCALAPPDATA%\PhoeCord\loader.log` has the line `client build: version … .text sha256 …`. Without it:

```powershell
$exe = (Get-Process Discord* | Select-Object -First 1).Path   # while the client runs
(Get-Item $exe).VersionInfo.FileVersion
Get-FileHash $exe -Algorithm SHA256
```

| Fact | Value |
|:---|:---|
| Date, OS | 2026-11-25, Windows 11 25H2 (26200) |
| Phoenix version | file 0.1.0.2208; release `0.1.0-dev.20261123093910.gb71ec80`, channel `release/phoenix_desktop`, client build 678271 |
| Executable SHA-256 | `f4186cf28cb5289c0e6764de11eef8f91cb205b72284050582aa29b3c0e5ca33` |
| Signature | valid, Discord Inc. |
| Executable | `%LOCALAPPDATA%\DiscordPhoenix\app\Discord.exe` (the folder has no version in its name) |

The client logs to `%LOCALAPPDATA%\DiscordPhoenix\logs\libdiscord.log`, one JSON object per line; TLS failures show up there. Redact before quoting it.

## Proxy behavior

The questions: does Phoenix honour `HTTPS_PROXY`, does it honour the system proxy, does it trust the system certificate store, and does it pin. Task 2.1.8 picks the proxy reach option from the answers.

### How to tell where a connection went

Run this while the client is connected. A remote address of `127.0.0.1` on the proxy's port means that connection used the proxy; a public address on port 443 means it went around.

```powershell
$ids = (Get-Process Discord*).Id
Get-NetTCPConnection -State Established -OwningProcess $ids |
  Where-Object RemotePort -in 443, 8080, 18463 |
  Select-Object OwningProcess, RemoteAddress, RemotePort
```

Linux: `ss -tnp | grep -i discord`.

QUIC (UDP) would not appear here and would not pass an HTTP proxy. Check `Get-NetUDPEndpoint -OwningProcess $ids` as well; voice uses UDP too, so look before joining a call.

### Step 1. `HTTPS_PROXY` and `--proxy-server`

Start with the environment variable alone. `phoecord launch` also passes `--proxy-server`, which would hide which of the two worked.

```powershell
mitmweb --listen-port 8080          # terminal 1
```

```powershell
$env:HTTPS_PROXY = 'http://127.0.0.1:8080'   # terminal 2
Start-Process 'C:\path\to\Phoenix\Discord.exe'
```

Linux: `HTTPS_PROXY=http://127.0.0.1:8080 ./Discord`.

mitmproxy's list shows each host the client asks for, even when the TLS handshake then fails. A request arriving here answers the question; the certificate is step 3.

If nothing arrives, repeat with `https_proxy`, then `ALL_PROXY`, then the command-line flag `--proxy-server=http://127.0.0.1:8080`.

**Result.** With `HTTPS_PROXY`, `HTTP_PROXY` and `ALL_PROXY` all set (checked to reach a child process started the same way), nothing arrived and the client connected directly. With `--proxy-server` every TCP connection went to the proxy: `discord.com` and `status.discord.com` (ALPN `h2`, `http/1.1`) and `remote-auth-gateway.discord.gg` (no ALPN). A tunnel that decrypts nothing was enough for this step.

### Step 2. System proxy

Close the terminal that set `HTTPS_PROXY`, or run `Remove-Item Env:HTTPS_PROXY`.

```powershell
$k = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Internet Settings'
Get-ItemProperty $k | Select-Object ProxyEnable, ProxyServer, AutoConfigURL   # write these down
Set-ItemProperty $k ProxyServer '127.0.0.1:8080'
Set-ItemProperty $k ProxyEnable 1
```

**Result.** Honoured: with only the registry values set (followed by `InternetSetOption` 39 and 37, and the effective value checked with `WinHttpGetIEProxyConfigForCurrentUser`), the client's connections, gateway included, went to the proxy.

Start Phoenix from the Start menu and watch mitmproxy. Then restore the values you wrote down (`Set-ItemProperty $k ProxyEnable 0` if the proxy was off).

If this fails, the client may read the WinHTTP setting instead (elevated shell): `netsh winhttp show proxy`, `netsh winhttp set proxy 127.0.0.1:8080`, and afterwards `netsh winhttp reset proxy`.

Linux: `http_proxy` and `https_proxy` in the environment; on GNOME also the system proxy setting.

### Step 3. System certificate store

Use whichever method of step 1 or 2 delivered traffic (on this build: `--proxy-server`). First without any added CA: every intercepted handshake must fail. If requests succeed, the client does not verify certificates; record that and tell the owner.

Then PhoeCord's own CA, which is what the fallback uses:

```powershell
phoecord trust                       # CurrentUser\Root
$env:PHOECORD_LOG = 'phoecord=debug'
phoecord run                         # 127.0.0.1:18463
```

```powershell
Start-Process 'C:\path\to\Phoenix\Discord.exe' -ArgumentList '--proxy-server=http://127.0.0.1:18463'
```

| `phoecord run` shows | Meaning |
|:---|:---|
| `gateway connected (…)` | The proxy is used and the CA is trusted |
| `discord.com:443: …` with a TLS alert such as `UnknownCA` | The proxy is used, the certificate is refused |
| Nothing | No intercepted host came here: compare the hosts in step 1 with the `intercept` list in `config.toml` |

The proxy speaks HTTP/1.1 only. If the client requires HTTP/2, record the error.

**Result without the CA trusted.** `phoecord run` showed `discord.com:443: received fatal alert: UnknownCA`; the client logged `rustls_platform_verifier::verification::windows` / `invalid peer certificate: UnknownIssuer`. So the client verifies certificates, through the Windows verifier.

**Result with the CA trusted** (`phoecord trust`, removed afterwards): the connections to `discord.com` and `gateway.discord.gg` were intercepted and worked over HTTP/1.1, the proxy logged `gateway connected (Json, Zstd)`, and the client logged `signed in: READY`. No pin showed on those two hosts, so step 4 was not needed.

### Step 4. Bundled roots or pinning

Do this only if step 3 refused PhoeCord's CA. Repeat step 3 with mitmproxy's CA, to rule out the name constraint:

```powershell
certutil -user -addstore Root "$env:USERPROFILE\.mitmproxy\mitmproxy-ca-cert.cer"
# ... launch through port 8080, look, then remove it:
certutil -user -delstore Root mitmproxy
```

If mitmproxy's CA works and PhoeCord's does not, the client's verifier rejects something about our certificate (name constraints are the first suspect).

If both are refused, **that alone does not prove pinning.** A client with its own root set (for example the `webpki-roots` crate) refuses a system-trusted CA too. The two differ in what 2.1.8 can do: a root set can be extended in-process (option 3), a pin cannot be satisfied by any CA of ours. Look for the TLS stack's crate paths, which Rust binaries often keep in panic messages:

```bash
grep -a -o -E '(rustls|rustls-platform-verifier|rustls-native-certs|webpki-roots|native-tls|schannel|reqwest|hyper-util)-[0-9]+\.[0-9]+\.[0-9]+' Discord.exe | sort -u
```

A name found is evidence that the crate is linked; a name not found proves nothing. Record the output (crate names and versions only, no other bytes of the binary). Telling a pin from a root set for certain needs the verifier located in the binary (with 2.1.5).

Afterwards: `phoecord untrust` if this machine does not otherwise run PhoeCord.

### Step 5. Endpoints

With a working setup, use the client for a few minutes (open channels, send a message, upload a file, open settings) and list which hosts went through the proxy and which connected directly.

**Result (2026-11-26, signed in past the account screen, no user actions: start-up, the friends list, about 90 seconds idle with incoming messages).** Through `--proxy-server` (mitmproxy as a plain recorder): `discord.com`, `gateway.discord.gg`, `cdn.discordapp.com` (changelog config, bad-domains list), `status.discord.com`, `latency.discord.media` (`/rtc`) and the Spotify connection's `api.spotify.com` and `dealer.spotify.com`. Through `phoecord launch --channel phoenix` with the same idle session, all of the client's 15 established TCP connections went to `127.0.0.1:18463` and none to a public address; no UDP endpoint was open. Nothing went around the proxy in this window (the Sentry bypass of 2026-11-25 shows only when the client reports an error). **Not covered:** uploads, voice (UDP), and picture and attachment loading from `media.discordapp.net` / `cdn.discordapp.com` while scrolling a channel; they need someone at the keyboard.

**Result (2026-11-27, two `phoecord launch --channel phoenix --transport proxy` runs, PhoeCord's `upstream` pointed at mitmproxy as a recorder of `CONNECT` hosts, nothing decrypted by mitmproxy).** First run tunnel-only (`intercept = []`, no CA trusted), second with the PhoeCord CA trusted and the usual `intercept` list (gateway intercepted, full `READY` asked for). In both every TCP connection of the client went to `127.0.0.1:18463` and no UDP endpoint was open; the hosts were the 2026-11-26 set (`discord.com`, `gateway.discord.gg`, `cdn.discordapp.com`, `status.discord.com`, `latency.discord.media`, the Spotify hosts) plus `o64374.ingest.us.sentry.io`, **through the proxy** this time. Both runs stopped at the account screen ("This app isn't available for your account"), which this account had passed on 2026-11-26; the second run shows the screen does not depend on interception or on the full `READY`.

**Result (2026-11-27, `phoecord launch --channel phoenix --transport proxy --also-inject phoenix.dll`, past the account screen).** The account screen had come back, so this run used `--also-inject` with sunbird's own `phoenix.dll` next to PhoeCord's loader — the owner's call on using it for this task specifically is [issue #197](https://github.com/Nanda070/phoecord/issues/197); sunbird's own code is not read or touched by this, PhoeCord's loader just hands Windows another DLL to load (same `--also-inject` path a user would use to run any other mod alongside). The client opened straight to the friends list, no account screen. With the tunnel-only sandbox config from the first 2026-11-27 run still in place, three things were done at the keyboard (computer use, server `TEAM`): a short text file uploaded in an empty text channel (sent and processed normally), an existing image opened in another channel (rendered fine), and a voice channel joined for about 18 seconds (showed Connected, left cleanly). All of it still went through `127.0.0.1:18463`: the host list grew two entries not seen before, `discord-attachments-uploads-prd.storage.googleapis.com` (the file upload's actual storage backend, reached through the proxy) and a per-region `*.discord.media` voice/media relay host; no UDP endpoint appeared outside the proxy during the voice join. **Uploads, voice and media while scrolling are no longer open.** Media-while-scrolling was light (one image, not a long scroll); a thorough pass would want a channel with many images/embeds and a look at the volume of `cdn.discordapp.com` / `media.discordapp.net` / `images-ext-1.discordapp.net` traffic, but the mechanism (through the proxy, not around it) is answered. Linux is still not run.

### Results

| Question | Windows | Linux |
|:---|:---|:---|
| `HTTPS_PROXY` honoured | **no** (nor `HTTP_PROXY`, `ALL_PROXY`) | not run |
| `--proxy-server` honoured | **yes** | not run |
| System proxy honoured | **yes** | not run |
| Verifies certificates | **yes**, `rustls-platform-verifier` (Windows) | not run |
| PhoeCord CA in the system store accepted | **yes** (`CurrentUser\Root`), on `discord.com` and `gateway.discord.gg` | not run |
| mitmproxy CA in the system store accepted | not needed | not run |
| TLS crates seen | `rustls` 0.23.46, `rustls-platform-verifier` 0.7.1, `rustls-native-certs` 0.8.5, `aws-lc-rs` 1.18.2; no `webpki-roots` path | not run |
| HTTP/2 or QUIC required | HTTP/2 not required; no UDP seen outside voice (voice not tested); `h2` 0.4.20 and `quinn` 0.11.9 are linked | not run |
| Hosts through the proxy | intercepted: `discord.com`, `gateway.discord.gg`; tunnelled: `status.discord.com`, `cdn.discordapp.com`, `latency.discord.media`, `remote-auth-gateway.discord.gg` (signed out), connected-account services (`api.spotify.com`, `dealer.spotify.com`); signed in past the account screen (2026-11-26, and 2026-11-27 via sunbird's patch): the same set, and every TCP connection of the client went to the proxy; uploading a file and joining voice add `discord-attachments-uploads-prd.storage.googleapis.com` and a per-region `*.discord.media` relay, both still through the proxy | not run |
| Hosts that bypass it | `o64374.ingest.sentry.io` (error reporting), with the flag and with the system proxy; on 2026-11-27 (`phoecord launch`, two runs) `o64374.ingest.us.sentry.io` went through the proxy instead, so the bypass is not constant | not run |

### What the answers mean for 2.1.8

On Windows the first row applies, through `--proxy-server` or the system setting rather than `HTTPS_PROXY`: no hook is needed. `phoecord launch` already passes the flag; a client started any other way needs the flag in its shortcut or the system setting. **Ruling (2026-11-26): option 1, no hook** (`docs/DECISIONS.md`).

**Fresh install (2.1.8, 2026-11-26, Windows, a release build from `main`).** Empty config and data folders (`PHOECORD_CONFIG` / `PHOECORD_DATA`), then only `phoecord setup --accept-terms --channel phoenix`: it created the CA, trusted it in `CurrentUser\Root` (one Windows confirmation) and added the "Discord Phoenix (PhoeCord)" shortcut, whose command is `phoecord launch --channel phoenix`. That command, with nothing set by hand, chose the proxy (the signature check found no `http_poll`, `gateway_registry` or outgoing-send locator), started the proxy and Phoenix with the loader injected. Result: REST went through the proxy (NoTrack answered `POST /science` locally), the gateway was decoded by the proxy (`gateway connected (Json, Zstd)` once a gateway plugin was on), the loader reported to the proxy (`loader connected: build 0.1.0.2208`), and every established TCP connection of the client was to `127.0.0.1:18463`. **Passes.**

| Proxy setting | System CA | Option (`ARCHITECTURE.md`, "Phoenix and the proxy") |
|:---|:---|:---|
| Honoured | Accepted | 1: nothing to build |
| Ignored | Accepted | 2: the loader sets the proxy |
| Honoured | Refused, own root set | 3: the loader adds our CA to the process roots |
| Ignored | Refused, own root set | 2 **and** 3: the options are not exclusive |
| Any | Refused because of a pin | None of the three: the proxy cannot be the fallback, which needs an owner ruling |

Options 2 and 3 are hooks, so they need signatures for each build, the same thing whose absence makes the proxy the fallback. A fallback that depends on them does not cover an unknown build; the ruling for 2.1.8 has to say so.

## Gateway wire format

This section matters for the proxy fallback only: in-process, the registry hands out decoded JSON ([`phoenix-desktop.md`](phoenix-desktop.md) §3.2).

### Encoding and compression

With `phoecord run` working (step 3), the answer is the line it prints for each connection:

```
gateway connected (Json, Zstd)
```

The line appears only while an enabled plugin reads the gateway; otherwise the proxy passes the connection through without decoding and says nothing. For an observation run, enable `message_logger`, which changes nothing at start-up, and disable `no_track`, so that no request is answered locally.

`phoecord-proxy/src/codec.rs` reads `encoding` (`json`, `etf`) and `compress` (`zlib-stream`, `zstd-stream`, or none) from the gateway URL. Anything else prints `unknown gateway …; passing the gateway through untouched`, and that line is the finding: it names what the codec lacks. Also note a connection that is logged as connected but then fails to decode: the parameters may be familiar while the framing is not.

### Capture

With a CA the client accepts, save the server's frames with this addon. It skips client frames on purpose: `IDENTIFY` and `RESUME` carry the token. Save it outside the repository as `gateway_capture.py`:

```python
"""mitmproxy addon: raw server -> client gateway frames, one JSON line each."""
import base64, json, os, pathlib

OUT = pathlib.Path(os.environ["PHOENIX_CAPTURE"])


class Capture:
    def websocket_message(self, flow):
        host = flow.request.pretty_host
        if not (host == "gateway.discord.gg" or host.startswith("gateway-")):
            return
        msg = flow.websocket.messages[-1]
        if msg.from_client:
            return
        line = {"path": flow.request.path, "text": msg.is_text,
                "data": base64.b64encode(msg.content).decode()}
        with (OUT / f"{flow.id}.jsonl").open("a", encoding="utf-8") as f:
            f.write(json.dumps(line) + "\n")


addons = [Capture()]
```

```powershell
$env:PHOENIX_CAPTURE = "$env:USERPROFILE\phoenix-capture"
New-Item -ItemType Directory -Force $env:PHOENIX_CAPTURE
mitmdump --listen-port 8080 -s gateway_capture.py
```

Start the client after mitmdump: stream compression keeps one context per connection, so a capture that misses the first frame cannot be decompressed. In the client, wait for it to load and send one message, then quit.

Start Phoenix with `--proxy-server=http://127.0.0.1:8080`.

Run the same capture against today's Electron client (`Discord.exe --proxy-server=http://127.0.0.1:8080`) with the same account and actions, for the comparison.

### Fixtures

`tests/fixtures/phoenix/` gets at least `HELLO`, `READY` and `MESSAGE_CREATE`, decoded. They are **rewritten, not redacted**: keep keys, nesting and value types; replace every string, every snowflake (including snowflakes used as map keys) and every number that identifies something with invented values. Delete nothing from the shape. `READY` carries the email, phone, session ids, resume URL, analytics token, settings, relationships and server names; none of it may reach the repository.

**Recorded (2026-11-26):** `HELLO.json`, `READY.json` and `MESSAGE_CREATE.json` from one signed-in Phoenix session, decoded (zstd-stream, one context per connection) and rewritten with `scripts/scrub_fixture.py` (task 2.1.10). On top of the tool, `READY`'s millisecond timestamps, activity start times, decoration expiry and name colours were replaced by fixed values, and the guilds' `data_mode` enum was set back to its real value `"partial"` (the scrubber rewrites every string). Left as captured: Discord's experiment hashes and bucket numbers (`experiments`, `guild_experiments`, `apex_experiments`), which are the client's global rollout configuration, not account data. The raw capture, the decoded events and the id map were deleted after scrubbing.

### Event shapes

For each event name, compare the set of key paths and value types between the two captures. Key paths contain no personal data once snowflake keys are replaced.

### Results

| Question | Phoenix | Electron today |
|:---|:---|:---|
| Encoding | **JSON** | not run |
| Compression | **zstd-stream** | not run |
| Gateway version (`v=`) | **9** | **9** |
| Other URL parameters | `/?encoding=json&v=9&compress=zstd-stream`, nothing else | `/?encoding=etf&v=9&compress=zstd-stream` (Discord Canary 1.0.1364) |
| Decodes with the current codec | **yes** | yes (`etf` and `zstd-stream` are both handled by `codec.rs`) |
| Events only on this client | none seen: every event name of the Phoenix session also occurs on Electron | — |
| Fields added or removed | see below | — |

The Electron column is Discord Canary 1.0.1364 on the same account, recorded the same way a few minutes later: today's desktop client sends ETF, not JSON, and already uses `zstd-stream`.

**Event shapes (key paths and value types, snowflake map keys folded).** `HELLO`: identical. `MESSAGE_CREATE`, `PRESENCE_UPDATE`, `VOICE_STATE_UPDATE`, `SESSIONS_REPLACE`: no structural difference; the key-path differences come from the content of the particular messages and presences, and every type difference is a snowflake, which is a string in JSON and an integer in ETF. **`READY` differs, and it matters for plugins:**

- Every guild arrives with `data_mode: "partial"` on Phoenix (`"full"` on Electron): no `channels`, `roles`, `emojis`, `stickers` or `properties`; instead `channel_updates` and `partial_updates`, changes against what the client already has. Phoenix keeps its own cache (open question 9) and asks for a partial `READY`.
- No `user_settings_proto` on Phoenix.
- `private_channels` and `user_guild_settings` carry fewer fields; `read_state` entries gain `badge_count`, `last_acked_id` and `read_state_type`.
- `READY_SUPPLEMENTAL` has 48 key paths that Electron's lacks.

So on Phoenix, a plugin that builds its picture of channels or roles from `READY` (ShowHiddenChannels, PermissionsViewer, anything reading `guilds[].channels`) sees nothing there and needs the data from another source: REST, later `GUILD_CREATE` / `CHANNEL_*` events, or in-process from the client's store. This is a finding for 2.4.1.

No `undecodable gateway payload` line appeared at debug level during about 45 seconds of one connection, so no change to `codec.rs` is needed for what was seen. The URL parameters are not logged, and the client's full event traffic was not seen (see the status of part 2).

## Degradation

Task 2.3.7. The decisions behind the three scenarios are automated and run on every OS with fake locator results and a fake `.text`, in `crates/phoecord-loader/tests/degradation.rs`:

| # | Test | What it checks |
|:---|:---|:---|
| 1 | `scenario_1_unknown_build_is_proxy_only` | no table for the build: `Plan::unknown_build()`, every entry point through the proxy, every UI feature off, nothing in-process |
| 2 | `scenario_2_one_broken_signature_fails_alone` | a table with `missing_on_purpose` (`DE AD BE EF …`): exactly one resolve error, every other signature resolves |
| 2 | `scenario_2_one_missing_locator_costs_only_its_feature` | one missing UI locator turns off only that feature; a missing gateway registry takes dispatch and send to the proxy and keeps REST and the UI; `Plan::all_in_process()` is false, so the launch uses the proxy (all or nothing, ruling 2026-11-26) |
| 3 | `scenario_3_electron_client_is_not_phoenix` | `ClientKind::of` tells the Electron clients (Stable, PTB, Canary) from Phoenix by Phoenix's fixed `DiscordPhoenix` install folder |

What stays open is the live run inside a client, which needs a Windows PC with Phoenix: the scenarios below, read in `%LOCALAPPDATA%\PhoeCord\loader.log` after `phoecord setup --loader`. Scenarios 1 and 2 in a live client also need the locators (2.2.5) and the in-process transport (2.2.7).

| # | Scenario | How to produce it | Pass when | Live result |
|:---|:---|:---|:---|:---|
| 1 | Unknown build | No table for this build in `%LOCALAPPDATA%\PhoeCord\signatures\` (an empty folder, or only other builds) | Log: `unknown client build: UI layer stays off, network plugins unaffected`; the client runs; network plugins work through the proxy | **passes** (2026-11-26, Phoenix 0.1.0.2208): `phoecord launch` chose the proxy ("unknown client build" for every entry point), the loader logged exactly that line and reported `UI layer off (unknown client build)`, the gateway was decoded by the proxy, the client reached the friends list |
| 2 | One signature missing | A table for this build with one pattern that matches nothing | Log: `signature <name>: no match; its feature stays off`; only that feature is off | **passes**: with the proxy, the missing entry was logged as above and the others resolved; forced `--transport in-process`, the loader logged the missing response and gateway locators as off and still installed the `send` observer (2.2.2), the client kept working |
| 3 | Electron client | The loader beside today's `Discord.exe` | No hooks; the client behaves as without the loader; no crash over a normal session | **passes** on Discord Canary 1.0.1364 (the loader copied into `app-1.0.1364\`, `phoecord launch --channel canary`): one worker in the main process, `unknown client build: UI layer stays off`, no transport from `phoecord launch` so no network hook, the client signed in and both gateway connections went through the proxy; the DLL was removed afterwards |

For scenario 2, `signatures\broken.toml` with the version from the `client build:` log line:

```toml
[build."0.0.0"]   # replace with the logged version
missing_on_purpose = { aob = "DE AD BE EF DE AD BE EF DE AD BE EF" }
```

For scenario 3 the loader can now recognise Phoenix by its install folder (`phoecord_loader::version::ClientKind`), so it can skip every Phoenix hook in an Electron client instead of relying on "no signature matched"; calling it from the loader's worker comes with the in-process wiring (2.2.7).
On Discord Canary the live run of scenario 3 passed without that recognition (no signature matched and no transport variable came from `phoecord launch`).

## In-process findings from live runs (2.2.2–2.2.4, 2.3.3; 2026-11-26)

Phoenix `0.1.0.2208` on Windows, with the account accepted. Every run used temporary probes in a local loader build; addresses and offsets are in the private recon notes. What was learned:

- **2.2.2, `send`.** The read-only detour of part 1 (`hooks::detour`) was run live with a logging sink: it installs under Control Flow Guard, the client keeps working, and method and URL are read on every request. Request bodies were all reported unreadable until `hooks::abi` read `bytes::Bytes` in the order rustc stores it here: vtable, pointer, length, data (fixed in its own PR).
- **2.2.2, `poll`.** The response future's vtable is loaded by exactly one instruction in `send`'s future constructor, which gives a `rip` anchor for the `http_poll` locator (the vtable; `poll` is its fourth word, as in every Rust trait-object vtable). Swapping that slot with a pass-through works live. The output of `poll` is the Ready/Pending enum: when Ready, it carries the HTTP status as a `u16`, a header block the size of an `http::HeaderMap`, and the body as a **`Vec<u8>` (capacity, pointer, length), not a `bytes::Bytes`**; a `204` has an empty, unallocated body. Replacing a response body therefore needs the host's allocator (the `rust_alloc` / `rust_dealloc` shims), as planned.
- **2.2.2, request body.** The request's body is a `bytes::Bytes` with the usual five-function vtable; the one in use for bodies is the shared (reference-counted) kind. The static kind (no-op drop, never unique) is also in the binary, so a replaced request body can point at memory PhoeCord owns, with no host allocation at all.
- **A detour must not be slow or read widely.** A probe that followed every pointer in `poll`'s output stalled the client's start-up; one log line per call did not. Detours do bounded work and no pointer chasing.
- **2.2.3, the gateway.** The function the first static pass called the registry builder is not on the dispatch path: during a signed-in session it ran once, for one event name, against an empty map. Event names are matched inline by many store-level handlers instead: each is a method of a handler trait (reached through a vtable) that receives the store, the event name as `&str`, and on the stack a slice of **typed** actions (fixed-size records), not JSON. A detour at that level sees typed data and needs more than the four argument registers. The JSON-shaped boundary that `Engine::on_dispatch` wants is earlier, between the decompressed frame and its deserialisation, and is not located yet; a typed message → `serde_json::Value` converter exists and is one candidate for message events.
- **2.2.4, outgoing frames.** The outgoing encoder builds an ordered map `{ "op": <opcode>, "d": <payload> }` and returns the frame **serialised as a `String`** in its first argument (the input is the typed command, whose variant picks the opcode; one variant carries the opcode itself). A hook after the call reads the JSON text directly; changing it means replacing that `String` with the host allocator. Heartbeat, identify and resume do not go through it.
- **2.3.3, settings.** The user settings section render is one large function that picks the page by the settings navigation enum and builds it from the Mana/GPUI builders: `div()`, `Div::child`, a style builder, a switch (fixed-size element with its label, a checked flag and an `on_change` slot), a push of an `AnyElement` and a string-to-element builder; the Developer Mode switch is built there. These builders and the section render are located in this build. A PhoeCord page means hooking that render for one page value (reusing an unused or hidden page, as another loader does, or adding one to the navigation) and building the page with those builders; nothing is hooked yet. What the page shows and what a click changes is ready as plain data (`ui/settings.rs`, 2026-11-27), so the native part only has to draw a `Page` and report `Edit`s.

# Part 3. Summary and decisions for 2.2–2.4 (2.1.9)

Status of stage 2.1 on 2026-11-26: 2.1.1, 2.1.2, 2.1.5, 2.1.6 and 2.1.7 are done; 2.1.3, 2.1.4 and 2.1.8 have their Windows answers and a decision, with the parts listed under "Open questions" still to run. All of it is one build (`0.1.0.2208`), one Windows machine and one account that the client stops at an access screen after `READY`.

## What stage 2.1 found

| Area | Finding | Where |
|:---|:---|:---|
| Binary | PE32+ native Rust client, 518 MB, signed by Discord Inc., CFG on, no .NET; 50 imported DLLs, none delay-loaded; no DLL search restriction in the header or manifest | 2.1.2 |
| Install and update | One fixed folder `%LOCALAPPDATA%\DiscordPhoenix\app`, version from the executable; updates replace `app\` in place (inferred, not observed); modules update apart; a system helper service lives in Program Files | 2.1.1 |
| Loading path | Launcher injection works with PhoeCord's own loader: the DLL is in before the client runs, the build is detected, the loader degrades cleanly with no signatures | 2.1.7 |
| Proxy reach | `--proxy-server` and the system proxy are honoured, `HTTPS_PROXY` is not; certificates are verified by `rustls-platform-verifier` against the system store; a trusted PhoeCord CA is accepted; no pin seen; HTTP/1.1 is enough; Sentry goes around the proxy | 2.1.3 |
| Gateway wire format | JSON with `zstd-stream`, decoded by today's codec without changes | 2.1.4 |
| HTTP transport | `reqwest`-based; `send`, its vtable, the response future's `poll` slot and the request layout located; allocator shims located | 2.1.5 |
| Gateway | One ABI for the per-event handlers; the registry is a per-instance tree reached through the context, built at run time, not a static table; an outgoing send path exists (not for heartbeat, identify, resume); `READY` is handled specially in the connection loop | 2.1.6 |
| UI | The root view chain (`Root` → `Frame` → `Splash` → `DiscordApp` → router / `Workspace`) and candidate hook points (`DiscordApp::render`, `MenuView::render`, workspace regions); see [`re/gpui-lifecycle.md`](../re/gpui-lifecycle.md) | 2.2.1 notes |

Addresses and offsets for the binary rows are in the private recon notes, not here; the signatures in use are in `signatures/phoenix.toml`.

## Decisions for 2.2–2.4

What the reconnaissance confirmed, and what each later task takes from it:

| Decision | Status | Tasks it feeds |
|:---|:---|:---|
| Phoenix is reached by launcher injection on every launch; nothing is copied into `app\` | **changed** from the plan (sideload `version.dll`); ruling 2026-11-26 | 2.2.8 (shortcut, no reconcile of files after an update), 2.4.5 |
| The proxy fallback is option 1: `--proxy-server` plus the CA in the system store, no hook | **confirmed**; ruling 2026-11-26 | 2.1.8, 2.2.8 |
| In-process is the main transport on Windows Phoenix; the choice is made per launch, all or nothing, from a cached `phoecord check` of `Discord.exe` | **new**; rulings 2026-11-26 | 2.2.7 (no per-entry-point handoff), 2.2.8, 2.2.10 |
| REST in-process: inline entry on `send`, vtable swap for `poll` | **confirmed**: both located | 2.2.2 |
| Body replacement waits for pinned allocator shims; until then a local reply is the safe default | **confirmed** | 2.2.2 |
| Gateway in-process hooks the registry's build/insert path or the context, not a static table | **changed**: the plan expected per-domain tables at fixed addresses | 2.2.3, 2.2.5 (a locator returns a function, not a table address) |
| `Engine::on_send` gets a hook | **confirmed** for the ordinary send path; heartbeat, identify and resume are sent elsewhere | 2.2.4 |
| The proxy codec needs no change for Phoenix | **confirmed** for one connection | 2.1.4, 2.2.7 |
| Locators work on string anchors, call fan-in and the `.pdata` function table, never on a fixed address | **confirmed**: the allocator shims have no string anchor, so "called by many, calls nothing back" is the rule | 2.2.5, 2.2.6, 2.2.9 |
| UI hooks attach at the exit of `DiscordApp::render`, at `MenuView::render` and at the workspace regions | **proposed** by the GPUI notes, nothing hooked yet | 2.3.3, 2.3.4, 2.3.5 |
| Themes: no `GlobalTheme` / `ThemeColors` symbol was matched by name; whether Phoenix keeps its palette in code or reads it as data | **answered, 2026-11-27: compiled in** — every default value read so far (font names, a font-size/weight scale, a spacing/radius scale) is a fixed constant loaded by `lea`/immediate, not read from a file or deserialized; the colour fields themselves are still unlocated (RE, Nanda) | 2.3.1 part 2, 2.3.2 |
| NoTrack cannot block Phoenix's error reports through the proxy: Sentry connects directly (2026-11-25; on 2026-11-27 it went through the proxy, tunnelled, so not seen by plugins) | **new** | 2.4.1 (NoTrack on Phoenix needs an in-process answer or stays partial) |

## Open questions and risks

Updated 2026-11-27 against the phase-2 task table. Answered since stage 2.1: the response layout, the `Bytes` vtable, Control Flow Guard and the method encoding (2.2.2, run live); the handlers' payload type (typed actions, so 2.2.3 hooks the frame inflater, where every event, `READY` included, is still JSON); the encoder's input and output (2.2.4: a typed command in, the frame `String` out); signatures are public in `signatures/phoenix.toml` and compiled in (2.2.10); the two `Found<T>` types were merged.

Open, by what has to answer it (RE is Nanda's, ruling 2026-11-27):

- **Where each view renders:** answered by the UI probe (27.11): each view type has two generic wrappers that call its `render` through a closure; the render functions of 44 views are in the private notes. The composer (2.3.15), the call panel (2.3.16), where names are drawn (2.3.13: inside the rows and the DM header), the masked-link dialog (2.3.20) and the role menu (2.3.11) were found in a second pass; only `ManaTheme`'s fields (2.3.1) remain for the UI.
- **The client's per-frame handler** (2.2.3, 2.2.7): found live 27.11 with its arguments (the frame record with the JSON text as a `Vec<u8>`, the session object); open: building and handing it a frame PhoeCord makes (ken).
- **IDENTIFY and heartbeat** (2.2.4, 2.2.7): IDENTIFY is built once at connect by its own function, which serializes `ClientState` (found live 27.11 with `ClientState`'s layout: `guild_versions` is a `BTreeMap`, emptied for the call; open: installing the hook before IDENTIFY, about 0.35 s after start); the gateway's op 1 heartbeat path is still unconfirmed.
- **A ready response future** (2.2.2): `Ready`/`Pending` is resolved; **closed, 2026-11-27: a local reply does not need the real future's layout at all — `send` returns an ordinary trait-object fat pointer, so a detour can hand back a self-built vtable of its own; the ready answer's own byte layout was already known.** Only one `Ready(Err)` niche stays open, and it is not needed for a reply that only ever answers `Ok`. **Applied** (live on Phoenix 0.1.0.2208, 2026-11-27: NoTrack's `POST /science` answered locally (204) from the first request on, the client kept running normally).
- **`ManaTheme`'s fields** (2.3.1): found statically, 2026-11-27 — the first read (`ManaTheme`'s callers, a sibling global) found only font and spacing scales; a colour-token-name survey then found a sorted list of 749 design-token names whose positions match the offsets real callers read, so the palette is 749 packed `0xRRGGBBAA` values, four bytes each, behind a tag + pointer pair on the global. Open: a live read to confirm it, and the extra float after the colours. **The user-name render** (2.3.13, nothing found yet), **per-element voice anchors** (2.3.16: VolumeBooster's slider found, a CallTimer lead), **which `PermissionWatch` record is which prompt** (2.3.20), **the CDN fetch** (2.3.8, 2.3.9: CDN images pass neither the REST `send` hook nor WinHTTP/WinINet; GIF and MP4 go through Windows Media Foundation; static images may have come from the disk cache, so a pass with never-loaded images is next), **the role menu** (2.3.11 follow-up: built by a shared builder, needs an anchor by label).
- **2.1.3:** closed by the owner (2026-11-27). 2026-11-27 (ruling, [issue #197](https://github.com/Nanda070/phoecord/issues/197)): `phoecord launch --also-inject` with sunbird's own `phoenix.dll` got past the access screen, so uploads, voice and media-while-scrolling finally ran — a text upload, an image render and ~18s in a voice channel all went through the proxy, no UDP endpoint seen outside it. GIF/MP4 media go through Windows Media Foundation, which may bypass `--proxy-server`. Only Linux (and macOS) stay open, checked after the Windows release.
- **2.2.8, 2.2.9:** no Phoenix update has been observed yet: how `app\` and `updates` change, whether a running client is swapped, how often builds ship, and a second build to check every signature on.
- **Sideload:** never tested; not needed while injection works.
- **Linux and macOS:** checked after the Windows release (ruling 2026-11-27); no Phoenix location or layout is known; the proxy with a plain launch is the plan there.

Risks:

- **One build.** Every finding and every public signature is from `0.1.0.2208`; a new build turns the hooks off until its table is added (degradation keeps the client working).
- **The access gate.** Discord's own screen ("This app isn't available for your account") blocks some accounts in Phoenix, on and off; Sunbird gets past it next to PhoeCord (`--also-inject`). A build the gate rejects is unusable whatever PhoeCord does.
- **Public signatures.** Anyone, Discord included, can read what PhoeCord hooks (accepted, ruling 2026-11-27).
- **Detection time.** Hashing `.text` and resolving the table took about 1.1 s on Phoenix after the SIMD scan (#187); the hooks still go in before `READY`.
- **Terms.** Client modification and traffic interception break Discord's Terms ([`TERMS.md`](../../TERMS.md)); every live run uses an account that can be lost.

# GPUI symbol import and reference-build comparison (2.2.1)

**Project.** Local Ghidra project `PhoenixProject` (not committed — `docs/phase-2/AGENTS.md`, "Ghidra/IDA projects stay local"). Two programs: `Discord.exe` (Phoenix 0.1.0.2208) and `gpui-reference.exe` (built from `research/gpui-reference/`). A Version Tracking session `VT_gpui-reference.exe_Discord.exe` correlates them.

**Named hook targets.** 10 of the 2.1.5/2.1.6 hook targets are named symbols in the project: `http_send`, `http_send_poll`, `rust_alloc`, `rust_dealloc`, `gateway_registry_build`, `gateway_registry_insert`, `gateway_send_encode`, `gateway_send_budget`, `gateway_connection_loop`, `rust_realloc_candidate`.

**Reference-build comparison.** The session has 6 match sets, 11,300 total correlations:

| Status | Count |
|:--|--:|
| ACCEPTED | 261 |
| AVAILABLE (candidate, unreviewed) | 384 |
| BLOCKED (conflicts with another candidate) | 10,655 |
| REJECTED | 0 |

Narrowed to source functions in the `gpui::` namespace itself: 18 ACCEPTED, 5 AVAILABLE, 2,111 BLOCKED (≈2,134 `gpui::` correlations total) — BLOCKED dominates by about two orders of magnitude. That is the GPUI revision delta this task asks for, as a number instead of a guess: Version Tracking's matchers propose a correlation for almost every reference-build GPUI function, but for `gpui::` specifically the results are overwhelmingly ambiguous (several source functions correlating to the same destination address or vice versa) — consistent with heavy generic monomorphization producing byte-identical or near-identical bodies across type parameters, which is exactly what GPUI's `Element`/`Entity`/`Render` traits generate. Only the less-duplicated GPUI functions clear that ambiguity and get ACCEPTED, for example `gpui::app::App::finish_update` and `gpui::platform::Platform::read_from_clipboard_async<gpui_windows::platform::WindowsPlatform>`; `gpui::app::App::flush_effects`, `gpui::window::with_element_arena_erased` and `gpui::app::entity_map::EntityMap::end_lease_inner` are AVAILABLE (proposed, not yet reviewed). None of the type names in the task's own list — `App`, `Window`, `Entity`, `Render`, `Element`, `GlobalTheme`, `ThemeColors`, `Hsla` — occur as an exact match by themselves; the demangled names carry gpui's own module path (e.g. `gpui::window::Window::…`, `gpui::app::App::…`), so a search has to look for the type name after any `gpui::<module>::` prefix, not for `gpui::<Type>` literally. The bulk of the 261 ACCEPTED matches are `std`/`core`/`alloc` (100 of 261 by namespace), with the rest spread across GPUI's layout and asset stack (`taffy`, `slotmap`, `image`, `regex_automata`/`regex_syntax`, `usvg`/`tiny_skia_path`/`ttf_parser`) — confirms the session and the reference build line up; GPUI's own API surface is just the hardest part to pin by byte correlation alone.

**Why this was unreadable through headless scripting last round.** The blocker left open was `java.io.IOException: domain object(s) are busy/locked` / `Versioned destination file must be checked-out for update`. Opening a VT session always opens its destination program (`Discord.exe`) for update, even just to read match data; `analyzeHeadless … -process Discord.exe` makes the headless analyzer itself open that same file as `currentProgram` first, so the session's own open of it collides with the one `-process` already holds — one file can't be opened for update by two independent consumers in the same JVM. Fix: drop `-process`/`-import` and run the query as a `-preScript` (no file is designated, so the headless analyzer never opens `Discord.exe` on its own), open the VT session's domain object directly through the project API, and release it in a `finally` block — and read function names off the session's own `getSourceProgram()`/`getDestinationProgram()` directly rather than through the script's `currentProgram`-bound convenience methods (which return nothing without a `-process` target). The query scripts are local-only, next to the other project tooling in `local_dumps/re/tools/` (gitignored, consistent with "no disassembly in the repository"). Nothing was accepted or rejected by this round of scripting — read-only.

**Open for whoever reviews the project in the GUI next:** the 384 AVAILABLE candidates (5 of them `gpui::`) are unreviewed; the 10,655 BLOCKED associations need a human's pick between conflicting candidates, or a stricter/narrower Version Tracking pass (e.g. restricted to GPUI's non-generic entry points), before they say anything useful.

**Done when, from `ai/plan.md`:** "the project has named symbols for the hook targets identified in 2.1.5–2.1.6" — met (10 named, listed above). "The reference build comparison is recorded" — met, counts above.
