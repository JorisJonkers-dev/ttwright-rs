# Versioning and release

Nothing resolves or releases from a moving branch: `main` is an integration branch.

1. Changes land through squash-merged pull requests with Conventional Commit titles (`feat:`,
   `fix:`, `chore:`, `feat!:` or `BREAKING CHANGE:` for majors).
2. `release.yml` (release-please) maintains a release pull request. Merging it tags `vX.Y.Z`,
   writes `CHANGELOG.md` and bumps `.release-please-manifest.json`.
3. The published release carries the platform packages, checksums, SBOM and build provenance
   (see the toolbox plan, Phase 5).

SemVer: before 1.0 a minor bump is the breaking lever (`bump-minor-pre-major`).

Reusable workflows and actions are pinned by full commit SHA with the tag in a trailing comment;
Renovate bumps both together. Upstream tags (`0.1.x`, `0.2.0`, `v0.1.1`) are not pushed here, so
release-please owns every `vX.Y.Z` tag in this repository.
