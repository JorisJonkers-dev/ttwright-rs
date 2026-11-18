# Changelog

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
