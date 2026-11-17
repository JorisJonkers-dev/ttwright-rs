# Versioning and release

Nothing resolves or releases from a moving branch: `main` is an integration branch.

1. Changes land through squash-merged pull requests with Conventional Commit titles (`feat:`,
   `fix:`, `chore:`, `feat!:` or `BREAKING CHANGE:` for majors).
2. `release.yml` (release-please) maintains a release pull request. Merging it tags `vX.Y.Z`,
   writes `CHANGELOG.md` and bumps `.release-please-manifest.json`.
3. The published release carries the `ttwright` binaries for Linux x86_64, Windows x86_64 and macOS
   (universal), the Contract schemas (`ttwright-result.v1.json`, `ttwright-events.v1.json`,
   `ttwright-contracts.v1.json`) and `SHA256SUMS`, with a build provenance attestation over all of
   them (`gh attestation verify <file> --repo JorisJonkers-dev/ttwright-rs`).
4. The release workflow then sends `component-release` to the Toolbox, whose Component bump workflow
   opens the pull request that pins the new tag in `known-good.toml`.

A dry run (Actions, Release, Run workflow) builds and attests the same assets from any branch and
keeps them as a workflow artifact, without a release.

The library API (`ttwright_core::api`: `install(job, &mut dyn Progress, &CancelToken)`) carries its own
version, `ttwright_contract::API_VERSION`, which `ttwright contracts --json` lists as the `ttwright-api`
contract. A caller links a tagged release; a change to a call, a type or an error code that an existing caller
would notice raises that version in the same release.

| Release | Result | Events | Library API |
| --- | --- | --- | --- |
| first release with `ttwright_core::api` (after 0.3.0) | 1 | 1 | 1 |

SemVer: before 1.0 a minor bump is the breaking lever (`bump-minor-pre-major`).

Reusable workflows and actions are pinned by full commit SHA with the tag in a trailing comment;
Renovate bumps both together. Upstream tags (`0.1.x`, `0.2.0`, `v0.1.1`) are not pushed here, so
release-please owns every `vX.Y.Z` tag in this repository.
