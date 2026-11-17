# ttwright-rs

Installs Tale of Two Wastelands (TTW) and other MPI packages from your own copies of the games, natively on
Linux, Windows and macOS. It is a Component of
[Raul's tRusty Toolbox](https://github.com/JorisJonkers-dev/rauls-trusty-toolbox): the Toolbox's `raul` runs it as
a process, passes it the game copies it detected and reads its result and event Contracts. The command line also
works on its own. There is no window: the Toolbox is the only UI.

ttwright-rs is the JorisJonkers-dev rewrite of
[SulfurNitride/TTW_Linux_Installer](https://github.com/SulfurNitride/TTW_Linux_Installer) by Sulfur Nitride and
its contributors, whose history and GPL-3.0-only licence it keeps. See [FORK.md](FORK.md) for how upstream changes
are ported. Report problems with this repository here, not upstream.

## Supported TTW versions

| Package | Versions | Written against |
| --- | --- | --- |
| Tale of Two Wastelands | 3.3 and 3.4 | 3.4.0, the current public release |
| Other MPI packages | any version | |

A TTW package of another version is refused (`unsupported_version`, exit code 3) unless you pass
`--allow-unsupported-version`. Any package whose manifest uses something this build does not implement (an asset
operation, a location, variable or check type, an audio parameter or format, a post-installation command, a
manifest field) is refused before a file is written, with `unsupported_feature` naming it, for example
`asset_op:3`. `ttwright contracts --json` lists the versions and every supported feature; `ttwright inspect`
checks a package without installing it.

Implemented: copy from a game folder or archive, new files from the package, VCDIFF (xdelta3) patches, Ogg Vorbis
resampling, audio encoding to Ogg Vorbis, WAV and MP3, archive (BSA) writing, MD5, SHA-1 and CRC32 file checks,
free-space and path checks, the rename and delete post-installation commands, and the file times the manifest sets.

## Getting the package

You need your own Fallout 3 and Fallout: New Vegas, and the TTW package from its publisher:
[mod.pub](https://mod.pub/ttw/133/files) (an account is needed). Extract the download; the `.mpi` file is inside.
No game files and no package are in this repository or its releases.

## Command line

```sh
# Install
ttwright install --mpi /path/to/TTW.mpi --fo3 /path/to/Fallout3 --fnv /path/to/FalloutNV --dest /path/to/output

# Continue an install that was cancelled (Ctrl+C) or interrupted
ttwright install --resume --mpi ... --dest /path/to/output

# Check a package and the game copies without writing anything
ttwright inspect --mpi /path/to/TTW.mpi
ttwright install --dry-run --mpi ... --dest ...

# Other commands: detect, verify, extract, logs, contracts, schema
ttwright --help
```

Without `--json`, game copies you leave out are looked for in Steam libraries (`--no-detect` turns that off).

### Driven by the Toolbox

`ttwright install --json` prints one `ttwright-result` document on stdout when it ends, also when it fails, and
`ttwright-events` progress lines (JSON, one per line) on stderr, or into the file named by `--events`. With
`--json` it never looks for game copies: pass every one the package needs, or it stops with `game_path_missing`.

| Exit code | Status | Meaning |
| --- | --- | --- |
| 0 | `success` | installed |
| 1 | `failed` | see `error.code` |
| 2 | `failed` | wrong arguments (`usage`) |
| 3 | `unsupported` | `unsupported_feature` or `unsupported_version`; nothing was written |
| 4 | `cancelled` | cancelled; `--resume` continues |

The error codes are stable: `usage`, `mpi_not_found`, `mpi_invalid`, `manifest_missing`, `manifest_invalid`,
`unsupported_feature`, `unsupported_version`, `game_path_missing`, `game_not_found`, `check_failed`,
`asset_failed`, `archive_failed`, `post_command_failed`, `resume_mismatch`, `cancelled`, `io`, `internal`.

The schemas are in [`contracts/`](contracts) and attached to every release; `ttwright schema result` prints them.
Every document carries an integer `schema_version`; `ttwright contracts --json` lists the versions this build
speaks.

### Resume

An install keeps a journal, `.ttwright-journal.json`, in the destination. A cancelled or interrupted install leaves
it; `--resume` skips every loose file already written and every archive already written, and refuses a journal of
another package or other game copies (`resume_mismatch`). A finished install removes it.

## Building

Rust 1.100 (`rust-toolchain.toml`). The MP3 and Vorbis encoders need `make`, a C compiler and `pkg-config`.

```sh
cargo build --release -p ttwright-cli   # target/release/ttwright
cargo test --workspace
```

| Crate | What it is |
| --- | --- |
| `ttwright-core` | the installer library: MPI packages, manifests, checks, asset operations, archives, resume; `ttwright_core::api` is the entry point a program links (typed job and result, progress events, cancel and resume, errors with `copy_text()`), and the `ttwright` command is a wrapper over it |
| `ttwright-cli` | the `ttwright` command |
| `ttwright-contract` | the result and event Contracts, error codes, the handshake and their JSON Schemas |
| `ttwright-testkit` | synthetic fixtures for the tests; no game data |

With Nix: `nix run github:JorisJonkers-dev/ttwright-rs -- --help`, or `nix develop` for a shell.

Upstream's author has a [Ko-Fi](https://ko-fi.com/sulfurnitride) and a [Discord](https://discord.gg/9JWQzSeUWt).
