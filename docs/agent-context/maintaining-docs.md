# Maintaining docs

## When does this apply

You shipped a code change that touched a surface owned by a doc, or you're about to
write a "we added X in phase Y" sentence into an architecture doc — stop and read
this.

## The rules are generic

The doc-authoring rules (the recoverability test, `path → symbol` pointers instead of
line numbers, describe-what-IS, single-owner, ~1–2k-token leaves, the decisions field
spec) are **generic** and live in the global kit:
[`~/agent-docs/v1/rules/authoring-rules.md`](~/agent-docs/v1/rules/authoring-rules.md).
Read that — it is the authority. This stub exists so `See also` links resolve and to
point you at the app bindings.

## The app bindings

- **What to update when you change file X** → the `change-to-doc` table in
  [`../_meta/manifest.md`](../_meta/manifest.md).
- **Who owns a concept** → [`../_meta/ownership.json`](../_meta/ownership.json)
  (explained in [`../ownership.md`](../ownership.md)).
- **Per-commit gates** → the `drift-gates` slot in the manifest.

## See also

- [`~/agent-docs/v1/rules/authoring-rules.md`](~/agent-docs/v1/rules/authoring-rules.md) — the authority.
- [`../_meta/manifest.md`](../_meta/manifest.md) — change-to-doc table, drift gates.
- [`../ownership.md`](../ownership.md) — how to use the ownership data.
