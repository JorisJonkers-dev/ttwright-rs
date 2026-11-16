# Fuzzing

Three targets, each built on every pull request (`cargo check --manifest-path fuzz/Cargo.toml`) and run for ten
minutes each week by `.github/workflows/fuzz.yml`:

| Target | Reads |
| --- | --- |
| `mpi_package` | an MPI package (a TES4 archive) from memory, `MpiStore::from_bytes` |
| `bsa_archive` | a file from a game archive (BSA) in memory, `BsaHandler::extract_from_bytes` |
| `manifest` | an MPI manifest: assets, the profile choice, locations, variables and archive targets |

Run one locally with nightly Rust and cargo-fuzz:

```sh
cargo +nightly fuzz run mpi_package -- -max_total_time=60
```

The corpus starts empty; no game data or MPI package is used as a seed.
