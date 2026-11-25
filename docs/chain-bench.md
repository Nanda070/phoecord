# Chain benchmark (task 1.1.10)

One-off report. Goal: the chain adds at most 1 ms p99 per gateway frame.

## What is measured

`cargo bench -p phoecord-proxy --bench chain` (source: `crates/phoecord-proxy/benches/chain.rs`).

Per frame: JSON decode (`Codec`) → `Proxy::on_dispatch` (account tracking, then `Chain::dispatch` through every enabled plugin) → JSON encode. The encode is always run, which is the worst case: the proxy re-encodes only when a plugin changed the frame, otherwise it forwards the original bytes.

- **defaults**: only the default-on plugin (`no_track`).
- **all_on**: every plugin `phoecord_plugins::builtin()` registers (34 plugins plus `commands`, including the 20 data parts from PR #68) switched on, each with its default settings.
- **READY ~5 MB**: generated, 5 MB of guilds (40 channels, 12 roles, 10 emojis, 25 members each), DMs and relationships. Run once before the other cases so account state is filled.
- **MESSAGE_CREATE**: 200 distinct generated frames (author, member, mention, link in the text, attachment), run in a burst; each frame is timed on its own for p50/p99 (20 000 samples).
- **REST**: one `GET /channels/{id}/messages` response with 50 messages through `Proxy::on_response`.

All payloads are generated from a fixed seed; nothing is real Discord data. Native plugins only: no WASM package is loaded, so the WASM path is not covered here (see `docs/phase-1/README.md`, risks).

## Machine

AMD Ryzen 5 7600 (6 cores / 12 threads), 32 GB RAM, Windows 11 Pro 10.0.26200, rustc 1.99.1 (release/bench profile, thin LTO), commit `89c2fc9` plus the bench (this PR). A desktop machine with other programs open: runs vary up to 2x, so the figures are the second of two runs and a ballpark.

## Results

Per-frame times (ms), from the p50/p99 table the bench prints:

| Case | p50 | p99 | max |
|:---|---:|---:|---:|
| MESSAGE_CREATE, defaults | 0.013 | 0.034 | 9.2 (one outlier) |
| MESSAGE_CREATE, all_on | 0.018 | 0.037 | 0.15 |
| READY 5 MB, defaults | 155 | 287 | 292 |
| READY 5 MB, all_on | 131 | 166 | 166 |

The first run gave MESSAGE_CREATE all_on p50 0.032 / p99 0.069 ms. The first MESSAGE_CREATE on a fresh proxy takes 0.06 ms, so the occasional 10 ms maximum is the OS scheduling, not lazy initialisation.

Criterion means (first run): REST 50 messages 0.11 ms (defaults) / 0.43 ms (all_on); burst of 200 MESSAGE_CREATE 5.4 ms / 10.3 ms, i.e. 27 µs / 52 µs per frame.

READY split (best of 5, all_on): decode 64 ms, chain 8 ms (the number printed is 35 ms because the bench clones the 5 MB tree first, 27 ms), encode 57 ms.

## Verdict

- **MESSAGE_CREATE meets the goal**: p99 0.04–0.07 ms with every built-in plugin on, about 15–25 times under 1 ms.
- **REST** response with 50 messages: 0.4 ms with every plugin on, under the goal.
- **READY does not meet 1 ms, and cannot with this design**: 130–300 ms, but 95 % of it is the codec (decode into `serde_json::Value`, then `Val::from_json`; `Val::to_json`, then `serde_json::to_vec`), not plugins: the chain itself is about 8 ms. It happens once per connection, before the client shows the UI.

No plugin was changed: nothing in the chain showed up as a cost for single frames, so there was nothing cheap to fix in plugins.

## Gap and plan (READY)

1. Decode straight into `Val` (a `serde::Deserialize` for `Val`) and encode straight from `Val` (a `Serialize`), skipping the two intermediate `serde_json::Value` trees. Expected to cut the codec cost roughly in half.
2. Only decode guilds the plugins ask for, or hand the chain a borrowed view, so a 5 MB READY is not rebuilt as owned data. Needs a decision on the plugin API, so not done here.
3. Re-run this bench after each step; add a WASM chain once `message_logger` and `fake_nitro` have packages.
