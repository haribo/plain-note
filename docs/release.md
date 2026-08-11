# Releases

Two distinct things, deliberately kept apart:

| | Snapshot | Release (later) |
|---|---|---|
| Source branch | `develop` | `main` |
| Identity | build stamp `develop-<date>-<sha7>` | SemVer tag `vX.Y.Z` |
| Stability | unstable, mutable | stable, immutable |
| GitHub | **pre-release**, rolling `snapshot` tag | normal release |
| Changelog | none | yes |

A snapshot is **not a version**. Its only job is to produce installable
artifacts so the software can actually be tested. Versioned SemVer releases on
`main` (with a changelog) come once we are ready to promise stability.

## Snapshot pipeline

Triggered **manually** (`workflow_dispatch`) via the `release-snapshot` workflow.
It builds from `develop` and:

- stamps the build (`PN_BUILD_INFO=develop-<date>-<sha7>`), visible in
  `pn --version` and the relay startup log;
- publishes Linux x86_64 binaries — `pn`, `pn-relay`, `plain-note-gui` — as a
  GitHub **pre-release** under the rolling `snapshot` tag (overwritten each run).

Notes:

- The `plain-note-gui` binary needs GTK 4 + libadwaita installed at runtime.
- **Android is not built** (no CI toolchain).

## Deployment

The project ships **binaries only**. Deployment — running the `pn-relay` binary,
containerizing it, TLS termination, service supervision — is the responsibility
of whoever self-hosts. See the relay's environment configuration in the top-level
`README.md`.
