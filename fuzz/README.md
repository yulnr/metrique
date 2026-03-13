# Fuzzing Guide

This crate uses [`cargo-fuzz`](https://github.com/rust-fuzz/cargo-fuzz) with libFuzzer to stress formatter invariants:

- `fuzz_json`: if formatting returns `Ok(())`, output must be valid JSON.
- `fuzz_emf`: if formatting returns `Ok(())`, each emitted line must be valid JSON object.
- Both targets format two entries through the same formatter instance to exercise state reuse.

## Current Validation Scope

Current assertions are intentionally focused on JSON framing/structural validity:

- parseable JSON for successful formatter calls,
- object-per-line shape for EMF output lines.

This is a baseline safety invariant. Additional semantic invariants may be added in future
(for example, stricter checks on expected EMF metadata structure when formatting succeeds).

## Prerequisites

- Rust nightly toolchain
- `cargo-fuzz` installed (`cargo install cargo-fuzz`)

## Run Locally

From this `fuzz/` directory:

```bash
cargo +nightly fuzz run fuzz_json -- -max_total_time=60
cargo +nightly fuzz run fuzz_emf -- -max_total_time=60
```

Longer local runs:

```bash
cargo +nightly fuzz run fuzz_json -- -max_total_time=1800
cargo +nightly fuzz run fuzz_emf -- -max_total_time=1800
```

## Reproduce A Crash

When libFuzzer finds a crash, it writes an input under `fuzz/artifacts/<target>/...`.

Reproduce with:

```bash
cargo +nightly fuzz run fuzz_json fuzz/artifacts/fuzz_json/<crash-file>
cargo +nightly fuzz run fuzz_emf fuzz/artifacts/fuzz_emf/<crash-file>
```

Then:

1. Fix the bug.
2. Add a deterministic regression test in the normal test suite.
3. Keep/record the reproducer until the regression test exists.

## Corpus Policy

`fuzz/corpus` is ignored in git to avoid accidental large commits.

- Do not commit the entire evolving local corpus.
- It is acceptable to commit a small, minimized seed corpus intentionally (if desired) after running corpus minimization.
- Crash repro inputs can be committed selectively when useful for auditability until covered by deterministic tests.

Minimize corpus (optional):

```bash
cargo +nightly fuzz cmin fuzz_json
cargo +nightly fuzz cmin fuzz_emf
```

## CI Behavior

Nightly GitHub Actions fuzzing:

- restores corpus from cache (`fuzz/corpus`),
- runs both fuzz targets,
- prunes corpus to a size cap,
- saves corpus back to cache.

This allows corpus quality to improve across CI runs without committing all corpus files into the repository.

### Corpus Cache Policy

CI cache keying uses a branch-scoped lineage with daily buckets:

- primary key: `${os}-${branch}-${day}-${run_attempt}`,
- restore priority: same day -> same week -> same branch -> same OS fallback.

This keeps corpus learning persistent while limiting unbounded long-term cache churn.
