<!--
`Ruling:` decision template.
An owner's decision is written as one line at the bottom of the plan's ledger, `ai/ledger.md`.
Then: python scripts/gen_decisions.py (rebuilds docs/DECISIONS.md; same commit).
If the decision changes the plan's README (for example "Not doing"), edit the README in the same commit.
One format for every plan; the generator looks for lines that start with `Ruling`.
-->

# Decision format

```
Ruling: <what was decided> (owner Nanda, YYYY-MM-DD, "<quote, verbatim or translated to English>") — why: <reason> — risk: <what can go wrong, or none>
```

## Example

```
Ruling: the main plugin runtime is WASM (wasmtime + WIT), not Lua or QuickJS (owner Nanda, 2026-11-24, "WASM") — why: strong sandbox, typed contract, TypeScript via componentize-js — risk: wasmtime adds 10–15 MB to the binary
```

## Rules

1. The quote is the person's words, not a retelling. A choice between options is the option and its gist. Quotes in another language are translated to English and marked "(translated)".
2. A decision without a quote is not recorded: ask first.
3. The date is the day of the decision, not the day of writing.
4. In the same turn: `python scripts/gen_decisions.py`.
5. Someone else's decision changes only through a new `Ruling:` line; the old one is never edited.
