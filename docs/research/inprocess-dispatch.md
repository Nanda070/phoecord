# In-process delivery of `Host::emit_dispatch` (2.2.3, 2.2.7)

Status (2026-11-27): **blocked on RE.** The in-process transport queues the server → client payloads
plugins make, but the hooks we have cannot hand them to the client. This note says why, which
workarounds were weighed and dropped, and what one function would unblock it. Addresses and byte
patterns stay in the private recon notes; this file describes behaviour only.

## What has to happen

A plugin calls `Host::emit_dispatch(frame)` with a whole gateway frame (`{"op":0,"t":…,"s":null,"d":…}`)
that the client should handle as if Discord had sent it. Today every caller is a reaction to
something other than a gateway event:

| Plugin | Emits after |
|:--|:--|
| `commands` (slash command answers) | a REST request (the command message the client sends) |
| `server_info`, `view_raw`, `unsuppress_embeds` | a message context menu click, a command |
| `silent_delete` (purge report) | a plugin timer |

So the client needs a **new** frame at a moment of the plugin's choosing, not a change to an
event that is already on its way.

Through the proxy this is simple: a local payload is written to the client's WebSocket as one
more message (`phoecord-proxy/src/gateway.rs`, the `locals.recv()` arm). Locals carry `"s": null`,
and Phoenix handles them (seen live with 2.3.5's menu answers through the proxy).

## What the in-process hook has

The incoming hook (`hooks/gateway_recv.rs`) is a detour on the frame inflater. The inflater is
called once per WebSocket message, takes the decompressor's output buffer and returns it to its
caller as one `Vec<u8>` holding one JSON frame; the caller then parses that frame and dispatches
it. The detour runs after the original returns and before the caller parses, and all it can do
is rewrite that one buffer in place (a changed event, or `PHOECORD_DROPPED` with the same `s`).

It cannot add a frame: there is no second call to answer, and nothing in the buffer format holds
more than one frame. `InProcessTransport::dispatch` already sorts the queue into `before` (queued
since the last event) and `after` (queued while handling it); the inflate hook counts them, logs
the first drop and throws them away.

## Workarounds weighed

| Idea | Why it is dropped |
|:--|:--|
| Two frames in one buffer (`{event}{local}`) | the caller parses one JSON value; a typical `serde_json::from_slice` fails on trailing characters, and then the real event is lost too |
| Put a local in the slot of an event plugins dropped | `s` is fine (the dropped event's), but no current emitter reacts to a gateway event, so such a slot almost never meets a waiting local |
| Shift the stream: hand out the local now, hold the real frame for the next call | every later frame arrives one message late, for good; heartbeat ACKs (op 11) arrive late as well, and a missing ACK makes the client close the connection as dead and reconnect |
| Make Discord send a spare frame (op 8 with our own `nonce`, then swap the `GUILD_MEMBERS_CHUNK` that answers it) | needs a way to send a frame to Discord, which is also missing (the encoder hook of 2.2.4 can only rewrite the client's own frames; `Delivery`'s `FrameSink` is `None`) |
| Replace a frequent low-value event (`PRESENCE_UPDATE`, `TYPING_START`) | the client loses real data, unpredictably |

None of them is safe enough to ship.

## What would unblock it (request to Nanda)

**The client's per-frame handler**: the function the inflater's caller hands the inflated frame
to (as text or after parsing it), with its calling convention and where its `self` / session
context comes from. With it:

1. **Detour it instead of (or next to) the inflater.** After the original handles the real event,
   the detour calls it once more for each queued local: `before`, then the event, then `after`, the
   same order as the proxy. Locals keep `"s": null`, as through the proxy, so the session's
   sequence number is untouched.
2. **Payloads queued outside a frame** (REST, menu clicks, timers) wait until the next frame and go
   in as its `before`. On a quiet account the next frame can be a heartbeat ACK, about 41 s away. If
   that is too slow, the next step is to call the handler from the client's own executor (needs
   to know how it is woken), not from our timer thread: the handler is not ours to call from
   another thread.

If the handler takes typed actions rather than JSON (as the registry handlers do, see
`hooks/gateway_recv.rs`), the useful find is one level up: whatever parses the text into those
actions.

A paired find helps 2.2.4 the same way: the function that writes one frame to the live gateway
connection, for `Delivery`'s `FrameSink` (plugin `emit_send` in-process).

A third find, for the full `READY` in-process (`full_ready`, 2.2.7): **where `IDENTIFY` is built
or sent.** Live (2026-11-27) the encoder hook of 2.2.4 never sees `op 1` or `op 2`, although
heartbeats are acknowledged, and `IDENTIFY` leaves about one second after the client starts. With
the faster signature scan the hooks are in about 1.1 s after start and `READY` reaches the plugins
(9 guilds, all `partial`), so emptying `client_state.guild_versions` needs a hook on that path
which is in before the first connection.

Question on event order: is "locals queued outside a frame go in before the next real event"
acceptable as the in-process rule? Through the proxy they go out at once, so the order between a
local and the next real event can differ by one frame.

## Code that is ready

- `transport::Delivery`: the queue (`MAX_QUEUED`, oldest dropped first), drop counters, the
  `before` / `after` split in `InProcessTransport::dispatch`.
- `hooks::gateway::split_dispatch` / `rebuild`: frame text in and out.
- What is left once the handler is known: its locator in `signatures/phoenix.toml`, a detour or a
  call in `hooks/gateway_recv.rs`, and a live check (a ViewRaw menu click in an in-process launch
  shows its local message).
