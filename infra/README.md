# API container releases

`.github/workflows/api-image.yml` checks the API and builds its Docker image on
pull requests to `main`. Every push to `main` (including merges from `dev`)
publishes `ghcr.io/parazeeknova/lennette-api` after API lint, unit tests and real
PostgreSQL/Redis integration tests pass. Manual runs publish only from `main`.
PR builds never log in or publish. Publishing uses the repository's
`GITHUB_TOKEN` with `packages: write`; no registry password is needed.

## Versions

The API package version in `apps/api/Cargo.toml` is independent of desktop
releases. Each publication receives a SemVer prerelease tag such as
`0.1.0-main.42.abcdef123456`, the full `sha-<commit>` tag, and the moving `latest`
tag. Bump the Cargo API version when changing the API release line and update
Cargo.lock with Cargo. Reruns retain the same version for the same workflow run.
Build summaries include the digest. Deploy by version or digest for repeatable
releases; `latest` follows successful main builds.

## Dokploy

The image targets Linux amd64, runs as UID 10001, includes CA certificates and
sets `API_BIND=0.0.0.0:8090`. Configure runtime secrets in Dokploy; env files are
excluded from the Docker context. Run the image with the `migrate` argument as
the deployment migration step before starting the server. The image does not
include PostgreSQL, Redis, ClamAV or OpenObserve. BuildKit attaches SBOM and
build provenance attestations.

After the first publication, choose the package's visibility in GitHub Packages.
Private packages require Dokploy registry credentials with `read:packages`;
public packages support anonymous pulls. If a package already exists outside
this repository, connect it to this repository and grant it Actions write access.
