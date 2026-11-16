# Changed-line coverage gate

Every pull request must run at least **70%** of the executable lines it adds or edits in a test. The step
"Changed-line coverage gate (70%)" of `Pipeline Complete` enforces it, so a pull request below the line cannot merge.
The rule and the number are the estate's (nv-rs `docs/fork/COVERAGE.md`).

## How it works

1. `cargo llvm-cov --workspace --lcov --output-path lcov.info` runs the tests once, instrumented.
2. `tools/coverage-gate` (Rust, no dependencies) reads the lines the change adds or edits from
   `git diff -U0 -M <base>...HEAD -- '*.rs'` and keeps those the report counts as executable.
3. Fewer than 70% of them ran: the step fails. It writes a table to the job summary with each file and the line
   numbers no test ran.

Details:

- The base is the pull request's base commit; on `main` it is the commit before the push. Without a base (a manual
  run) the gate has nothing to compare with and passes.
- Renames are detected, so a moved file counts only the lines that changed in it.
- A change with no executable line (documentation, workflows, comments, `use` lines) passes.
- The fuzz harnesses and `tools/` are their own workspaces and are not measured.

## Locally

```sh
mise exec -- cargo llvm-cov --workspace --locked --lcov --output-path lcov.info
cargo run --manifest-path tools/coverage-gate/Cargo.toml -- --lcov lcov.info --base origin/main --min 70
```

Building needs `make`, a C compiler and `pkg-config` (the LAME and Vorbis encoders).
