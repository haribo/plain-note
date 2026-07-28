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
  GitHub **pre-release** under the rolling `snapshot` tag (overwritten each run);
- builds and pushes the relay image to `ghcr.io/<owner>/pn-relay:snapshot`.

Notes:

- The `plain-note-gui` binary needs GTK 4 + libadwaita installed at runtime.
- **Android is not built** (no CI toolchain).
- The GHCR package starts private; make it public in the repo's package settings
  if self-hosters should pull it anonymously.

## Relay image

The image is a **release artifact**, not a hosted service. Run it yourself:

```sh
docker run -d --name pn-relay \
  -e PN_RELAY_ADMIN_TOKEN=<secret> \
  -v pn-relay-data:/data \
  -p 8787:8787 \
  ghcr.io/<owner>/pn-relay:snapshot
```

The relay serves plain HTTP; terminate TLS with a reverse proxy for internet use.
