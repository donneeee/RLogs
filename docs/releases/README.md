# rLogs release checkpoints

rLogs releases are deliberate major-feature checkpoints. Ordinary fixes and
incremental feature slices remain on `main` without a version tag.

A release is ready only when:

1. The major feature is complete at its truthful capability boundary.
2. The exact commit has a successful `CI` push run on `main`.
3. `apps/desktop-tauri/tauri.conf.json` has the next semantic version.
4. `docs/releases/v<version>.md` is created from `TEMPLATE.md` and states the
   supported game builds, available capabilities, locked capabilities, and
   known limitations without implying that a guarded feature is usable.
5. The version commit is on `main`, then an annotated `v<version>` tag is
   created for that exact commit and pushed.

The tag starts `.github/workflows/release.yml`. The workflow rechecks the tag,
version, release-note contract, and successful main CI run; builds a versioned
Windows x64 NSIS installer; installs and launches it in a smoke test; records
its Authenticode status; verifies the release contains exactly that one
versioned installer; publishes its SHA-256 checksum file; and only then makes
the draft GitHub release public. The website resolves that asset from GitHub's
latest-release metadata because the validated filename changes with each version.

If any check fails, leave the release draft unpublished, fix the problem in a
new commit and version, and use a new tag. Do not move or reuse a published
release tag.
