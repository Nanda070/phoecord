#!/usr/bin/env python3
"""Rewrite a decoded Discord Gateway event into a fixture that carries no real data (task 2.1.10).

"Rewritten, not redacted" (docs/research/phoenix-recon.md, "Fixtures"): every key, the nesting,
and each value's JSON type are kept exactly; every string and every snowflake-shaped value
(string or integer, keys included) is replaced with an invented one. Nothing is deleted from the
shape, so a fixture still has the same fields a real event would, just with no real content.

Snowflakes get a *consistent* invented replacement: the same real id maps to the same fake id
everywhere it appears in one run (a message's author id and the same id inside a mention stay
recognizably "the same person"), and across files too with `--map`. Every other string is just
replaced once, in place, built from its own key so the fixture still reads as "a username", "a
URL", "an email" without being one; two different real values may end up with the same fake one,
and that is fine; nothing here needs to be reversible except through `--map`.

Usage:
    python -I scripts/scrub_fixture.py INPUT.json -o OUTPUT.json [--map snowflakes.json]
    python -I scripts/scrub_fixture.py --self-test

INPUT is one decoded Gateway event as JSON (an object, commonly with "op"/"t"/"s"/"d"), or a JSON
array of several. --map reads and updates a local id-mapping file (real snowflake -> fake
snowflake): pass the same file across every fixture from one capture so repeated ids stay
consistent, and never commit that file — it is the key that would undo the scrub.
"""

import argparse
import json
import re
import sys
from pathlib import Path

SNOWFLAKE_RE = re.compile(r"^\d{15,20}$")
# A snowflake's millisecond timestamp alone is in this range for 2015-2040; a bare integer outside
# it (counts, permission bitfields that fit a JS-safe int, flags) is not treated as an id.
SNOWFLAKE_INT_MIN = 4 * 10**16

ISO_RE = re.compile(
    r"^(\d{4}-\d{2}-\d{2})T(\d{2}:\d{2}:\d{2})(\.\d+)?(Z|[+-]\d{2}:?\d{2})?$"
)

NAME_POOL = ["Alex", "Sam", "Jordan", "Taylor", "Riley", "Morgan", "Casey", "Drew"]

# Structural/enum fields, never personal data by themselves: the Gateway envelope ("op", "t",
# "s") and Discord's own short, fixed-vocabulary "type" discriminators (channel type, embed type,
# activity type, ...). A fixture needs "t" intact to be loadable by event name at all.
PASSTHROUGH_KEYS = ("op", "t", "s", "type")

EMAIL_KEYS = ("email",)
PHONE_KEYS = ("phone",)
SECRET_KEYS = (
    "token",
    "secret",
    "resume_gateway_url",
    "analytics_token",
    "session_id",
    "cookie",
    "password",
    "backup_codes",
)
URL_KEYS_SUFFIX = ("_url", "url")
HASH_KEYS_SUFFIX = ("_hash", "avatar", "banner", "splash", "discovery_splash", "icon")
NAME_KEYS = ("username", "global_name", "nick", "display_name", "tag")
TEXT_KEYS = ("content", "description", "topic", "bio", "about_me", "custom_status", "reason")


class Scrubber:
    """One run (or, with a loaded `--map`, one coordinated set of files) worth of state: the
    snowflake map and a counter so every other placeholder is unique."""

    def __init__(self, id_map=None):
        self.id_map = dict(id_map or {})
        self.counter = 0

    def next_n(self):
        self.counter += 1
        return self.counter

    def fake_snowflake(self, real):
        if real not in self.id_map:
            # Keep the same digit count: a 17-digit real id gets a 17-digit fake one, so a fixture
            # keeps looking like a snowflake to code that validates the shape.
            digits = len(real)
            fake_n = len(self.id_map) + 1
            fake = str(10 ** (digits - 1) + fake_n).rjust(digits, "0")
            self.id_map[real] = fake[:digits]
        return self.id_map[real]

    def fake_string(self, key, value):
        key = (key or "").lower()
        n = self.next_n()
        if any(k in key for k in EMAIL_KEYS):
            return f"user{n}@example.invalid"
        if any(k in key for k in PHONE_KEYS):
            return f"+1-555-{n:04d}"
        if key in SECRET_KEYS or any(key.endswith(s) for s in ("_token", "_secret")):
            return f"scrubbed-{key}-{n}"
        if key in HASH_KEYS_SUFFIX or any(key.endswith(s) for s in ("_hash",)):
            return f"{n:032x}"
        if key in URL_KEYS_SUFFIX or any(key.endswith(s) for s in URL_KEYS_SUFFIX):
            return f"https://example.invalid/{key}/{n}"
        if key in NAME_KEYS:
            return f"{NAME_POOL[n % len(NAME_POOL)]}{n}"
        if key in TEXT_KEYS:
            return f"lorem ipsum fixture text {n}"
        m = ISO_RE.match(value)
        if m:
            date, time, frac, tz = m.groups()
            return f"2020-01-01T00:00:{n % 60:02d}{frac or ''}{tz or 'Z'}"
        return f"redacted-{key or 'value'}-{n}"

    def scrub_string(self, key, value):
        if SNOWFLAKE_RE.match(value):
            return self.fake_snowflake(value)
        return self.fake_string(key, value)

    def scrub_key(self, key):
        if SNOWFLAKE_RE.match(key):
            return self.fake_snowflake(key)
        return key

    def scrub_value(self, key, value):
        if key in PASSTHROUGH_KEYS and not isinstance(value, (dict, list)):
            return value
        if isinstance(value, str):
            return self.scrub_string(key, value)
        if isinstance(value, bool):
            return value  # bool is an int subclass in Python; keep it before the int check below.
        if isinstance(value, int) and value >= SNOWFLAKE_INT_MIN:
            return int(self.fake_snowflake(str(value)))
        if isinstance(value, dict):
            return {self.scrub_key(k): self.scrub_value(k, v) for k, v in value.items()}
        if isinstance(value, list):
            return [self.scrub_value(key, v) for v in value]
        return value

    def scrub(self, event):
        return self.scrub_value(None, event)


def run(input_path, output_path, map_path):
    data = json.loads(Path(input_path).read_text(encoding="utf-8"))
    id_map = {}
    if map_path and Path(map_path).exists():
        id_map = json.loads(Path(map_path).read_text(encoding="utf-8"))
    scrubber = Scrubber(id_map)
    scrubbed = scrubber.scrub(data)
    Path(output_path).write_text(
        json.dumps(scrubbed, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )
    if map_path:
        Path(map_path).write_text(
            json.dumps(scrubber.id_map, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )


def self_test():
    s = Scrubber()
    event = {
        "op": 0,
        "t": "MESSAGE_CREATE",
        "s": 42,
        "d": {
            "id": "1234567890123456789",
            "channel_id": "1234567890123456789",
            "content": "hey, my number is 555-1234",
            "author": {
                "id": "9876543210987654321",
                "username": "realname123",
                "email": "realname123@gmail.com",
                "avatar": "abcdef0123456789abcdef0123456789",
            },
            "mentions": [{"id": "1234567890123456789", "username": "realname123"}],
            "timestamp": "2026-11-25T10:00:00.123000+00:00",
            "flags": 0,
            "pinned": False,
        },
    }
    out = s.scrub(event)
    assert out["op"] == 0 and out["s"] == 42, "non-identifying numbers are untouched"
    assert out["t"] == "MESSAGE_CREATE", "the envelope's event name is not personal data"
    assert out["d"]["pinned"] is False, "booleans survive"
    assert out["d"]["id"] == out["d"]["mentions"][0]["id"], "repeated snowflake stays consistent"
    assert out["d"]["id"] == out["d"]["channel_id"], "two different real ids of equal digit count can collide, which is fine"
    assert out["d"]["id"] != "1234567890123456789", "the real id is gone"
    assert len(out["d"]["id"]) == 19, "digit count (shape) is preserved"
    assert "@" in out["d"]["author"]["email"] and "gmail" not in out["d"]["author"]["email"]
    assert out["d"]["content"] != event["d"]["content"] and "555-1234" not in out["d"]["content"]
    assert out["d"]["author"]["username"] != "realname123"
    assert re.match(r"^[0-9a-f]{32}$", out["d"]["author"]["avatar"])
    assert out["d"]["timestamp"].startswith("2020-01-01T00:00:") and out["d"]["timestamp"].endswith("+00:00")
    assert set(out.keys()) == set(event.keys()), "no field is dropped or added"
    assert set(out["d"].keys()) == set(event["d"].keys())

    # --map round trip: the same real id scrubbed in two separate runs with a shared map stays
    # the same fake id, the way two fixtures from one capture need to.
    shared = {}
    a = Scrubber(shared).scrub({"id": "1111111111111111111"})
    shared = Scrubber(shared).id_map
    b = Scrubber(shared).scrub({"id": "1111111111111111111"})
    assert a["id"] == b["id"], "a shared map keeps the same real id consistent across runs"

    print("scrub_fixture self-test: all assertions passed")


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("input", nargs="?", help="decoded Gateway event, as JSON")
    parser.add_argument("-o", "--output", help="where to write the scrubbed fixture")
    parser.add_argument("--map", help="id-mapping file to read and update (never commit it)")
    parser.add_argument("--self-test", action="store_true", help="run the built-in assertions and exit")
    args = parser.parse_args()

    if args.self_test:
        self_test()
        return
    if not args.input or not args.output:
        parser.error("INPUT and -o/--output are required unless --self-test is given")
    run(args.input, args.output, args.map)
    print(f"wrote {args.output}")


if __name__ == "__main__":
    main()
