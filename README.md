# ttwright-rs

`ttwright` installs [Tale of Two Wastelands](https://taleoftwowastelands.com/) (TTW), which brings Fallout 3's world
into Fallout: New Vegas, and other MPI packages, from your own copies of both games, natively on Linux, Windows and
macOS. It writes only into the folder you give it; both games stay untouched.

Players use it through [raul](https://github.com/JorisJonkers-dev/rauls-trusty-toolbox), which has it built in and
adds the result to your mods as one Mod. On its own it is a command line program and a library.

ttwright-rs is the JorisJonkers-dev rewrite of
[SulfurNitride/TTW_Linux_Installer](https://github.com/SulfurNitride/TTW_Linux_Installer) by Sulfur Nitride and
its contributors, whose history and GPL-3.0-only licence it keeps. See [FORK.md](FORK.md) for how upstream changes
are ported. Report problems with this repository here, not upstream.

## Used from raul

raul's **Tale of Two Wastelands** page walks through four steps: the sources (your Fallout 3 folder, the New Vegas
copy of raul's Instance and the TTW package you downloaded), the output folder and the space it needs, a review, and
the install with its progress. You can leave the page while it runs, cancel it, and continue a cancelled install
later. When it is done, raul adds TTW to the Instance as one Mod.

<p align="center">
  <img src="https://raw.githubusercontent.com/JorisJonkers-dev/rauls-trusty-toolbox/main/crates/raul-gui/tests/snapshots/ttw/ttw-sources-ready.png" alt="raul's TTW page, step 1 of 4: the Fallout 3 folder, New Vegas and the TTW package are set" width="49%">
  <img src="https://raw.githubusercontent.com/JorisJonkers-dev/rauls-trusty-toolbox/main/crates/raul-gui/tests/snapshots/ttw/ttw-installing.png" alt="raul's TTW page while it installs, at 25 percent" width="49%">
</p>

The same from raul's command line: `raul ttw --instance <dir>`, with the game folders and the package taken from
`raul config` or given as flags.

## Why a separate project

ttwright is one of the Components raul is made of, next to [nv-rs](https://github.com/JorisJonkers-dev/nv-rs),
[nvmod-rs](https://github.com/JorisJonkers-dev/nvmod-rs), [loot-rs](https://github.com/JorisJonkers-dev/loot-rs) and
[lodwright-rs](https://github.com/JorisJonkers-dev/lodwright-rs). Each does one job, is tested on its own and
releases on its own schedule; raul links them into one program (ttwright through `ttwright_core::api`) and ships one
tested set. Keeping the installer apart also keeps its upstream history and the porting of upstream changes simple.

## Supported TTW versions

| Package | Versions | Written against |
| --- | --- | --- |
| Tale of Two Wastelands | 3.3 and 3.4 | 3.4.0, the current public release |
| Other MPI packages | any version | |

A TTW package of another version is refused (`unsupported_version`, exit code 3) unless you pass
`--allow-unsupported-version`. Any package whose manifest uses something this build does not implement (an asset
operation, a location, variable or check type, an audio parameter or format, a post-installation command, a
manifest field) is refused before the package's files are written, with `unsupported_feature` naming it, for
example `asset_op:3`. `ttwright contracts --json` lists the versions and every supported feature; `ttwright inspect`
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

# Bind a source location declared by the package (repeat for more locations)
ttwright install --mpi /path/to/other.mpi --dest /path/to/output \
  --location "TEXTURE SOURCE=/path/to/downloaded/textures"

# Continue an install that was cancelled (Ctrl+C) or interrupted
ttwright install --resume --mpi ... --dest /path/to/output

# Check a package and the game copies without writing anything
ttwright inspect --mpi /path/to/TTW.mpi
ttwright install --dry-run --mpi ... --dest ...

# Other commands: detect, verify, extract, logs, contracts, schema
ttwright --help
```

Without `--json`, game copies you leave out are looked for in Steam libraries (`--no-detect` turns that off).

An MPI manifest declares its source folders in `Locations` (an array per profile, each entry with a `Name`, a `Type`
and a `Value`). Most point at a game copy through a variable such as `%FO3DATA%`. A package may also declare a
location with a value ttwright cannot fill itself (an unrecognised variable token, or no value at all): that
location is bindable. `ttwright inspect --mpi ...` lists those declared source locations, marks each required or
optional and says whether it is bound; `--location NAME=PATH` on `inspect` shows the effect without writing
anything. On install, repeat `--location NAME=PATH` for every required location. Names are matched exactly, apart
from ASCII case, and must name a source location the package declares: a game location cannot be bound. An unbound
required location stops the install before any of the package's files are written with `location_path_missing`,
naming the location. Bound folders (and archives) are read only, and a folder inside `--dest` or containing it is
refused. ttwright writes only inside `--dest`, redirecting anything the manifest aimed outside it.

### Driven by another program

`ttwright install --json` prints one `ttwright-result` document on stdout when it ends, also when it fails, and
`ttwright-events` progress lines (JSON, one per line) on stderr, or into the file named by `--events`. With
`--json` it never looks for game copies: pass every one the package needs, or it stops with `game_path_missing`.

| Exit code | Status | Meaning |
| --- | --- | --- |
| 0 | `success` | installed |
| 1 | `failed` | see `error.code` |
| 2 | `failed` | wrong arguments (`usage`) |
| 3 | `unsupported` | `unsupported_feature` or `unsupported_version`; no files were installed |
| 4 | `cancelled` | cancelled; `--resume` continues |

The error codes are stable: `usage`, `mpi_not_found`, `mpi_invalid`, `not_a_package`, `manifest_missing`, `manifest_invalid`,
`unsupported_feature`, `unsupported_version`, `game_path_missing`, `location_path_missing`, `game_not_found`, `check_failed`,
`asset_failed`, `archive_failed`, `post_command_failed`, `resume_mismatch`, `cancelled`, `io`, `internal`.

ttwright never writes outside the destination. A file the manifest meant for a place outside it (the real TTW
package's first assets go to `FNV Root`) is put under `<destination>/_ttwright_game_root/` with its relative path
instead. The result lists them in `redirected` (up to 100: `step`, `location`, `target`, `written_to`) and counts them
in `redirected_total`, also on a dry run; the plain output prints a summary and the list. A check on the final path
(`..` and symbolic links resolved) guards every write, delete, rename and file time.

The schemas are in [`contracts/`](contracts) and attached to every release; `ttwright schema result` prints them.
Every document carries an integer `schema_version`; `ttwright contracts --json` lists the versions this build
speaks.

### Resume

An install keeps a journal, `.ttwright-journal.json`, in the destination. A cancelled or interrupted install leaves
it; `--resume` skips every loose file already written and every archive already written, and refuses a journal of
another package or other game copies (`resume_mismatch`). A finished install removes it.

### Packages

The `ttwright` program is published as OCI packages on ghcr.io:

`ghcr.io/jorisjonkers-dev/ttwright-rs:<version>-<target>`

where `<version>` is the release tag without the leading `v` and `<target>` is one of
`x86_64-unknown-linux-gnu`, `x86_64-pc-windows-msvc` or `universal-apple-darwin`. Each package is a `tar.gz`
(Linux and macOS) or `zip` (Windows) archive containing the binary and `LICENSE`.

Verify a package with:

```sh
gh attestation verify oci://ghcr.io/jorisjonkers-dev/ttwright-rs:<version>-<target> --repo JorisJonkers-dev/ttwright-rs
```

To republish an existing tag, for example after the package was renamed, run the release workflow by hand on `main` with the tag and `publish` set: `gh workflow run release.yml -R JorisJonkers-dev/ttwright-rs --ref main -f tag=vX.Y.Z -f publish=true`. The run builds the binaries of that tag, uploads the assets to its GitHub release and pushes the packages again; it tells no one. Without `publish=true` it is a dry run that pushes nothing.

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
