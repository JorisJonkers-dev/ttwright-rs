# ttwright in more detail

The [README](../README.md) is the short version. This page has where ttwright comes from, why it is its own
project, and how to build it.

## Origin

ttwright-rs is the JorisJonkers-dev rewrite of
[SulfurNitride/TTW_Linux_Installer](https://github.com/SulfurNitride/TTW_Linux_Installer) by Sulfur Nitride and
its contributors, whose history and GPL-3.0-only licence it keeps. See [FORK.md](../FORK.md) for how upstream changes
are ported. Report problems with this repository here, not upstream.

## Why a separate project

ttwright is one of the Components raul is made of, next to [nv-rs](https://github.com/JorisJonkers-dev/nv-rs),
[nvmod-rs](https://github.com/JorisJonkers-dev/nvmod-rs), [loot-rs](https://github.com/JorisJonkers-dev/loot-rs) and
[lodwright-rs](https://github.com/JorisJonkers-dev/lodwright-rs). Each does one job, is tested on its own and
releases on its own schedule; raul links them into one program (ttwright through `ttwright_core::api`) and ships one
tested set. Keeping the installer apart also keeps its upstream history and the porting of upstream changes simple.

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
