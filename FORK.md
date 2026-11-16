# About this repository

ttwright-rs is the JorisJonkers-dev rewrite of
[SulfurNitride/TTW_Linux_Installer](https://github.com/SulfurNitride/TTW_Linux_Installer), a Rust
installer for Tale of Two Wastelands and other MPI packages, by Sulfur Nitride and its
contributors. Their history is kept here, their [LICENSE](LICENSE) (GPL-3.0-only) applies to
this repository, and code ported from upstream keeps its authors' credit.

It is a component of [Raul's tRusty Toolbox](https://github.com/JorisJonkers-dev/rauls-trusty-toolbox),
alongside nv-rs, nvmod-rs, loot-rs and lodwright-rs.

## Why it exists

- To run TTW installs natively on Linux, Windows and macOS, as a library and a contract-driven
  command line that the Toolbox's `raul` runs as a process through its Contracts (ADR-0002 and
  ADR-0007 in rauls-trusty-toolbox), with progress events, resume and a result document.
- To work the way the other components work: a required check on every merge, Conventional
  Commits, release-please, Renovate with a 7-day cooldown, tests, fuzzing of the MPI and BSA
  parsers, and attested release assets.

## Branches

| Branch | What it is |
| --- | --- |
| `main` | **the default branch**: upstream's history, then the rewrite. Protected: pull requests only, squash merges, `Pipeline Complete` must pass. Never rewritten. |
| `upstream` | an exact copy of upstream `main`, fast-forwarded by `.github/workflows/upstream-sync.yml`. Nothing else writes it. |
| `<type>/<topic>` | work on `main`, for example `feat/resume`. |

Upstream's tags (`0.1.x`, `0.2.0`, `v0.1.1`) are not pushed here; release-please owns every
`vX.Y.Z` tag in this repository.

## Porting from upstream

When upstream moves, the sync workflow opens an issue listing the new commits. Each one is
ported to `main` by pull request, adapted to this repository's structure, or ticked off as not
applicable with a reason. The commit message names the upstream commit it ports.

### Where upstream's files are here

| Upstream | Here |
| --- | --- |
| `src/models/`, `src/services/`, `src/app/install_runner.rs`, `src/lib.rs` | the same paths under `crates/ttwright-core/src/` |
| `src/app/cli.rs` | `crates/ttwright-cli/src/main.rs` (install, contracts, schema) and `commands.rs` (the other commands, as they were) |
| `src/app/gui.rs` | removed: the Toolbox is the only UI. GUI changes are not ported. |
| `examples/` | `crates/ttwright-core/examples/` |
| `tools/xdelta3` | removed: patches are applied by oxidelta, in process |

The layout under `crates/ttwright-core/src/` is upstream's, so most upstream commits apply with
`git format-patch -1 <commit>` and `git am --directory=crates/ttwright-core <patch>`. Differences to expect: the
library prints through `tracing` instead of `println!` (stdout carries the result document), errors that callers
match on are `InstallError` with an `ErrorCode`, and unknown manifest features are refused in
`src/app/preflight.rs` instead of skipped.

## Not shipped

No game files, no MPI packages and no TTW data are in this repository or its releases. Users
provide their own copies of Fallout 3 and Fallout: New Vegas and download the MPI package from
its publisher.
