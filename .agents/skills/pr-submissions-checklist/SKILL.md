---
name: pr-submissions-checklist
description: Must read before creating or updating a pull request to Fedimint Observer.
---

# PR Submissions Checklist

Adapted from the Fedimint repository's `pr-submissions-checklist`; the rules
on PR descriptions are the same.

## Scope and history

- Branch from `upstream/master`. One logical change per PR; unrelated fixes and
  drive-by refactors go in a separate PR.
- Commit titles use conventional prefixes in the imperative mood:
  `feat(frontend): show ...`, `fix(health): monitor ...`, `perf(gateways): ...`,
  `docs: ...`, `chore: ...`.
- Don't undo earlier commits in later ones; rework the history instead.
- Don't mark a PR ready for review while it contains placeholders or
  non-functional code.

## Pre-submit checks

Run these locally so CI isn't the first to find easy problems:

- `just final-check` (pre-commit lint, clippy, doc tests and workspace tests),
  inside `nix develop`.
- For changes under `fmo_frontend_react/`: `npm run lint`, `npm run build`,
  and the `npm run test:*` scripts that `react-frontend-ci.yml` runs.
- For changes to SQL, migrations, projectors or read models: the ignored
  PostgreSQL regressions listed under "Tests" in `OPERATIONS.md`.

## Screenshots (required for frontend changes)

A PR that changes anything users see must include screenshots in its
description. Reviewers will ask for them otherwise.

- Every page or component that changed, before and after.
- Light and dark mode.
- Phone width as well, when the layout is affected.
- Loading, empty and error states when the change touches them.

## PR description

The description must help readers understand all the important aspects of the
change without having to deduce them from the code. Read the change itself; if
information is missing, ask whoever requested the change.

Keep its length proportionate to the change's logical scope and risk, not its
line count. Use this as a guide, skipping parts that don't apply:

```markdown
### Motivation

{Why make this change? Who asked for it, or what issue or earlier work does it follow?}

### Context

{What a reader new to this part of the observer needs to know to follow the change.}

### Summary

{A short TL;DR of the sections below.}

### {Details sections as needed}
```

Useful details sections: key changes (file paths, type names), testing (only
non-obvious steps a reader would need to verify it, not a log of checks you
ran), alternatives, tradeoffs, downsides, benchmark (for queries on large
tables, include `EXPLAIN` output or timings), migration and upgrade notes,
privacy impact, screenshots, non-goals.

Style: concrete, active prose with plain words. Prefer a precise contrast
("Before, X; now Y, so Z") to a list of changes. Leave out conversations, agent
names, review iterations and work logs.
