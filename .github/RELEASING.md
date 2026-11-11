# Releases

The repository follows the sibling projects: Conventional Commits, release-please, one shared workspace version, binary archives, and crates.io publication of the library before the CLI.

`release-please.yml` creates the version/changelog PR. After that PR merges, it creates a `vX.Y.Z` release and explicitly dispatches the binary and crate publishing workflows, because releases created with `GITHUB_TOKEN` do not trigger downstream release events. The bootstrap manifest starts at 0.0.0; the first feature release is 0.1.0.

Before using release-please, enable the repository setting that allows GitHub Actions to create pull requests.

`release.yml` builds `quran-detect` natively for Linux (x86_64 and arm64), macOS (arm64 and x86_64) and Windows (x86_64, with a static C runtime), packages it with the README, license, Tanzil notices, generated dependency licenses, CLI reference and man page (`.zip` on Windows, `.tar.gz` elsewhere, each with a SHA-256 file), and smoke-tests the binary extracted from each archive. Manual runs without `publish` keep CI artifacts only. Publishing requires a tag matching the root package version.

`publish.yml` verifies the workspace packages, then publishes `quran-detect` before `quran-detect-cli`, skipping versions already on crates.io. First-time publication requires a repository `CARGO_REGISTRY_TOKEN`. After initial publication, configure trusted publishing for both crates with this repository, workflow `publish.yml` and environment `release`, then remove the token. Creating this repository or a pull request does not publish anything.

Before a release that changes matching code or the embedded Tanzil text, run `scripts/parity.py` and the benchmark gate described in [validation](../docs/validation.md).
