# Using ttwright

The [README](../README.md) is the short version. This page has how raul uses ttwright, the supported TTW
versions, where to get the package, the command line and the downloads.

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

The schemas are in [`contracts/`](../contracts) and attached to every release; `ttwright schema result` prints them.
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

## Feature catalogue

`catalogue/` lists the MPI manifest features an install supports (`install.features`), read by
[tally](https://github.com/JorisJonkers-dev/tally) (format: its `docs/FORMAT.md`). The implemented items must be
exactly the features `ttwright contracts --json` lists (`crates/ttwright-contract/src/supports.rs`): a pull request
that adds a feature adds its item in the same pull request, or the Pipeline's "Feature catalogue (tally)" step fails.

Tests say which features they prove, with the typed ids `ttwright-testkit` builds from the catalogue (its build.rs
runs tally's generator, `catalogue/tally_ids_gen.rs`):

```rust
use ttwright_testkit::{covers, ids};

#[test]
fn a_patch_applies_in_memory_and_on_disk() {
    covers(&[ids::install::features::ASSET_OP_2]);
    // ...
}
```

An id the catalogue does not have does not compile. Tag only what the test runs and checks; more than ten ids in one
test needs a `// covers-table: <reason>` comment. Locally:
`cargo run -p ttwright-cli -- contracts --json >target/contracts.json && tally check`.

An item is **covered** when a test tagged with it passed on Linux, Windows and macOS. CI runs the tests through
cargo-nextest (`.config/nextest.toml`, profile `ci`: no retries, skipped tests listed) on all three, and
`Pipeline Complete` reads the three JUnit files with `tally ingest`. The covered and tagged counts may not drop
below main's last report (`docs/coverage/tally-report.json`), or the step fails. On main the job "Catalogue reports" commits the generated reports (`docs/coverage/FEATURES.md`,
`tally-report.json`, `tally.svg`) as the raul-progress-bot App; pull requests never edit `docs/coverage/`. Locally:
`cargo nextest run --workspace --profile ci`, then
`tally ingest --junit target/nextest/ci/junit.xml --os linux --out target/results.json` and
`tally report --results target/results.json` (Linux alone gives passing, not covered).
