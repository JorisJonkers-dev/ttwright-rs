# Changelog

## [0.6.0](https://github.com/JorisJonkers-dev/ttwright-rs/compare/v0.5.0...v0.6.0) (2026-11-25)


### Features

* **catalogue:** feature catalogue of the supported manifest features, checked against the Contracts ([#72](https://github.com/JorisJonkers-dev/ttwright-rs/issues/72)) ([afa0021](https://github.com/JorisJonkers-dev/ttwright-rs/commit/afa0021efd9af7fbecf670e32b044207ed895f38))
* **install:** bind custom source locations with --location ([#67](https://github.com/JorisJonkers-dev/ttwright-rs/issues/67)) ([c51c5c0](https://github.com/JorisJonkers-dev/ttwright-rs/commit/c51c5c0e0c4137728b8bb076a730abf8d9762ce1))


### Bug Fixes

* **deps:** update minor and patch dependencies ([#63](https://github.com/JorisJonkers-dev/ttwright-rs/issues/63)) ([ef7e475](https://github.com/JorisJonkers-dev/ttwright-rs/commit/ef7e475631765620c5d688e7094dd10a23571401))

## [0.5.0](https://github.com/JorisJonkers-dev/ttwright-rs/compare/v0.4.4...v0.5.0) (2026-11-20)


### Features

* **deps:** port audio decoding to symphonia 0.6 ([#47](https://github.com/JorisJonkers-dev/ttwright-rs/issues/47)) ([10529cc](https://github.com/JorisJonkers-dev/ttwright-rs/commit/10529cce72fbbd7bb0d46ccbcda34ae2572f06a7))
* **deps:** port the memory probe to sysinfo 0.40 ([#49](https://github.com/JorisJonkers-dev/ttwright-rs/issues/49)) ([294d1d4](https://github.com/JorisJonkers-dev/ttwright-rs/commit/294d1d46b240505fe03782e5c1c6ecd2ff90672d))


### Bug Fixes

* **audio:** encode mono MP3 from mono samples ([#61](https://github.com/JorisJonkers-dev/ttwright-rs/issues/61)) ([8164eff](https://github.com/JorisJonkers-dev/ttwright-rs/commit/8164eff80a82867c40cec9505c12622aaac64fac))
* **audio:** honour -c, -fmt and -b, refuse unknown audio parameters, name non-package files ([#54](https://github.com/JorisJonkers-dev/ttwright-rs/issues/54)) ([8fe04cf](https://github.com/JorisJonkers-dev/ttwright-rs/commit/8fe04cffd49fa044285c189310044b2fa5ff002d))
* **install:** let a resume pass the must-not-exist check for files it wrote itself ([#57](https://github.com/JorisJonkers-dev/ttwright-rs/issues/57)) ([6227954](https://github.com/JorisJonkers-dev/ttwright-rs/commit/6227954a4dfa3df850cd2b2209d8e90139688838))
* **install:** never write outside the destination ([#60](https://github.com/JorisJonkers-dev/ttwright-rs/issues/60)) ([c752a9a](https://github.com/JorisJonkers-dev/ttwright-rs/commit/c752a9a11072b87c22a809e8daa487bae1495ab8)), closes [#55](https://github.com/JorisJonkers-dev/ttwright-rs/issues/55)

## [0.4.4](https://github.com/JorisJonkers-dev/ttwright-rs/compare/v0.4.3...v0.4.4) (2026-11-20)


### Bug Fixes

* **deps:** update minor and patch dependencies ([#43](https://github.com/JorisJonkers-dev/ttwright-rs/issues/43)) ([8e4dc33](https://github.com/JorisJonkers-dev/ttwright-rs/commit/8e4dc33b0d71ae99f50dcba62f6ccb197c413482))

## [0.4.3](https://github.com/JorisJonkers-dev/ttwright-rs/compare/v0.4.2...v0.4.3) (2026-11-18)


### Bug Fixes

* **deps:** hold cc below 1.9 for Windows builds ([#36](https://github.com/JorisJonkers-dev/ttwright-rs/issues/36)) ([62d7853](https://github.com/JorisJonkers-dev/ttwright-rs/commit/62d78531c4ac7676e252f47b30927516a206718a))

## [0.4.2](https://github.com/JorisJonkers-dev/ttwright-rs/compare/v0.4.1...v0.4.2) (2026-11-18)


### Bug Fixes

* **deps:** use caret requirements so the Components link together ([#37](https://github.com/JorisJonkers-dev/ttwright-rs/issues/37)) ([7602461](https://github.com/JorisJonkers-dev/ttwright-rs/commit/76024612a6b7c720ec78994cecda08588c2383d3))

## [0.4.1](https://github.com/JorisJonkers-dev/ttwright-rs/compare/v0.4.0...v0.4.1) (2026-11-18)


### Bug Fixes

* **deps:** pin dependencies ([#26](https://github.com/JorisJonkers-dev/ttwright-rs/issues/26)) ([6de3019](https://github.com/JorisJonkers-dev/ttwright-rs/commit/6de30190081668c9e1bdae002775a6e53487f811))
* **release:** find the release pull request from release-please's output ([#33](https://github.com/JorisJonkers-dev/ttwright-rs/issues/33)) ([cd52ae9](https://github.com/JorisJonkers-dev/ttwright-rs/commit/cd52ae9c0b00be0370821de4ca23a38cbfb21e3b))

## [0.4.0](https://github.com/JorisJonkers-dev/ttwright-rs/compare/v0.3.0...v0.4.0) (2026-11-17)


### Features

* **api:** library API for in-process installs with progress, cancel, resume and copyable errors ([#20](https://github.com/JorisJonkers-dev/ttwright-rs/issues/20)) ([ceb08cc](https://github.com/JorisJonkers-dev/ttwright-rs/commit/ceb08cc023c411220f9679c7dbb0b09decef66ec))


### Bug Fixes

* **deps:** update libz-sys for RUSTSEC-2026-0544 and note chrono's unmaintained advisory ([#22](https://github.com/JorisJonkers-dev/ttwright-rs/issues/22)) ([b632952](https://github.com/JorisJonkers-dev/ttwright-rs/commit/b6329528ade38a09ccb99643b9039103aceea68d))

## [0.3.0](https://github.com/JorisJonkers-dev/ttwright-rs/compare/v0.2.0...v0.3.0) (2026-11-16)


### ⚠ BREAKING CHANGES

* the binary is ttwright, not mpi_installer; the library crate is ttwright-core.
* remove the eframe GUI; the Toolbox is the only UI ([#10](https://github.com/JorisJonkers-dev/ttwright-rs/issues/10))

### Features

* restructure into a Component with result and event Contracts ([#13](https://github.com/JorisJonkers-dev/ttwright-rs/issues/13)) ([3247192](https://github.com/JorisJonkers-dev/ttwright-rs/commit/3247192f0138c50ef778056ffe779823e95fb584))


### Bug Fixes

* build on macOS, where statvfs block counts are 32-bit ([#4](https://github.com/JorisJonkers-dev/ttwright-rs/issues/4)) ([4b31438](https://github.com/JorisJonkers-dev/ttwright-rs/commit/4b3143838f492c3b76339dc8f5728f334ad86824))
* **release:** start release notes at the estate bootstrap commit ([#3](https://github.com/JorisJonkers-dev/ttwright-rs/issues/3)) ([731c909](https://github.com/JorisJonkers-dev/ttwright-rs/commit/731c90925551e8b1cec597888ea05a87e1e9b44d))


### Code Refactoring

* remove the eframe GUI; the Toolbox is the only UI ([#10](https://github.com/JorisJonkers-dev/ttwright-rs/issues/10)) ([f57d269](https://github.com/JorisJonkers-dev/ttwright-rs/commit/f57d2690d84178605cadfe8c118bb3e1691c7760))

## [0.1.6] - 2026-01-29

### Major Performance Improvements

#### Streaming BSA Builder - 10x RAM Reduction
- **Before:** All BSA file data accumulated in RAM (10-15GB peak)
- **After:** Files stream to disk staging, only ~1.8GB peak RAM
- Enables installation on systems with 8GB RAM or less

#### Parallel BSA Building
- BSA archives now built concurrently using Rayon
- Multiple BSAs process simultaneously instead of sequentially
- Better CPU utilization during finalization phase

#### Parallel BSA Pre-extraction
- BSA files extracted in parallel instead of one at a time
- Eliminates CPU idle time between processing phases

#### Optimized Chunk Processing
- Reduced chunk boundaries (fewer pauses)
- Systems with 6GB+ RAM now process in single chunk
- Continuous CPU utilization throughout installation

#### Installation Timer
- Added timer to both CLI and GUI
- Shows total installation time in minutes and seconds

### Technical Details

**StreamingBsaBuilder** (`bsa_handler.rs`):
- Files written to temporary staging file on disk as they're added
- Only lightweight metadata (~100 bytes/file) kept in RAM during processing
- Each BSA's staging file is independent
- Thread-safe via interior mutability (Mutex)

**Memory Usage Comparison:**
| Stage | Before | After |
|-------|--------|-------|
| During processing | ALL BSA files in RAM | Metadata only (~5MB for 50k files) |
| During BSA write | ALL BSAs combined (10-15GB) | ONE BSA at a time (~1.8GB peak) |

### Performance Results
- **RAM:** Reduced from 10-15GB to ~1.8GB peak (10x improvement)
- **Time:** ~2 minutes on Ryzen 7 9800X3D
- **CPU:** Consistent utilization with minimal idle gaps

---

## [0.1.5] - 2026-01-28

### RAM-Based Chunking
- Added RAM-aware chunked processing to control memory usage
- Automatically adjusts chunk count based on available system memory

---

## [0.1.4] - 2026-01-27

### SQLite BSA Cache
- Replaced in-memory BSA cache with SQLite-backed storage
- Reduced RAM usage from 10-38GB to ~100MB for cache
- Stream BSA files to SQLite instead of collecting in memory

---

## [0.1.3] and earlier

- Initial release with GUI and CLI installers
- Support for TTW, FNV BSA Decompressor, Oblivion BSA Decompressor
- Audio transcoding (MP3, OGG, WAV)
- xdelta3 patch support
- File verification
