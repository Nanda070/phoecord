# Gateway fixtures (task 2.1.10)

Decoded Discord Gateway events, scrubbed so none of them carry real data, for tests that need a
realistic event shape without touching the network or a real account.

**Status: the scrubbing tool (`scripts/scrub_fixture.py`) and this README are done. The fixtures
themselves are not recorded yet** — that step needs a human with a throwaway Discord account and
a running `mitmproxy`/`phoecord`, which cannot be done from here. See "Recording" below for the
steps whoever does that should follow.

## Layout

```
tests/fixtures/
  electron/   today's client: `encoding=json` and `encoding=etf`, both with `zlib-stream`
  phoenix/    Desktop Phoenix: `encoding=json` with `zstd-stream` (confirmed in
              docs/research/phoenix-recon.md, "Gateway wire format")
```

Each file is one decoded event as JSON (`{"op": 0, "t": "MESSAGE_CREATE", "s": ..., "d": {...}}`),
named `<EVENT_NAME>.json` (`HELLO.json`, `READY.json`, `MESSAGE_CREATE.json`, ...). At least
`HELLO`, `READY`, `MESSAGE_CREATE` (task's "done when"); `GUILD_CREATE`, `MESSAGE_UPDATE`,
`MESSAGE_DELETE`, `PRESENCE_UPDATE` and `TYPING_START` are the plan's fuller list.

## Recording

**Use a throwaway account** — one you can afford to have suspended or lose, never a real one.
`docs/research/phoenix-recon.md`, "Before you start" has the general risk notes (a captured
session has the account token; `TERMS.md` on client modification and traffic interception) and
"Capture" has the `mitmproxy` addon (`gateway_capture.py`) that saves only server→client frames,
one JSON line per connection. Coordinate with ken (task 2.1.4): the same capture, read once,
covers both tasks.

1. Capture with that addon against the Electron client, and separately against Phoenix, each with
   `--proxy-server` pointed at `phoecord run` or plain `mitmdump`.
2. Decode each captured frame into one JSON object per event (`phoecord-proxy/src/codec.rs`'s
   codec reads the same `encoding=`/`compress=` gateway URL parameters this needs).
3. Scrub every decoded event before it goes anywhere near this repository:

   ```
   python -I scripts/scrub_fixture.py decoded/READY.json -o tests/fixtures/phoenix/READY.json --map /tmp/snowflakes.json
   python -I scripts/scrub_fixture.py decoded/MESSAGE_CREATE.json -o tests/fixtures/phoenix/MESSAGE_CREATE.json --map /tmp/snowflakes.json
   ```

   The same `--map` file across every event of one capture keeps a repeated id (a user id that is
   both a message's author and a mention) consistent in the fixtures. **Never commit the map
   file** — it is the real-id-to-fake-id key, kept only on the recording machine, deleted once the
   fixtures are in.
4. Open the scrubbed JSON and read it yourself before committing: the scrubber replaces every
   string and every snowflake-shaped value, but it is a heuristic tool, not a guarantee — look
   especially at `d.user` / `d.relationships` / `d.user_settings` inside `READY`, which carry the
   most personal data of any event (email, phone, session ids, resume URL, analytics token,
   settings, relationships, server names).
5. Delete the raw, unscrubbed capture and the decoded-but-unscrubbed intermediates once the
   fixtures are committed; they never belong in the repository (`docs/AGENTS.md`, "Never commit
   … traffic dumps").

## Done when

Fixtures load in tests, and a human (Nanda) has reviewed them for personal data before merging —
the scrubber does not replace that review.
