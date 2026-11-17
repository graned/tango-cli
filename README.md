# tango

Registry-agnostic artifact orchestration CLI for Docker/OCI images.

Tango standardizes the container image lifecycle across repositories and monorepos with a strict philosophy:

- **Build once per commit** — never rebuild the same SHA twice
- **Promote by digest** — no rebuilds on merge, just retag
- **Digests are truth, tags are pointers** — deploy by `sha256:...`, use tags as aliases
- **Registry is configuration** — works with GHCR, GitLab, Docker Hub, or any OCI registry
- **Reusable across projects** — one CLI, zero CI script duplication

---

## Table of Contents

- [Installation](#installation)
- [Core Concepts](#core-concepts)
- [Configuration](#configuration)
  - [Root Config](#root-config)
  - [Monorepo Packages](#monorepo-packages)
  - [Environment Variables](#environment-variables)
- [Commands](#commands)
  - [tango publish](#tango-publish)
  - [tango promote](#tango-promote)
  - [tango resolve](#tango-resolve)
  - [tango print-ref](#tango-print-ref)
  - [tango affected](#tango-affected)
  - [tango dance](#tango-dance)
- [Global Flags](#global-flags)
- [Workflow Model](#workflow-model)
- [GitHub Actions Integration](#github-actions-integration)
- [Architecture](#architecture)
- [Development](#development)

---

## Installation

### From source

```bash
git clone https://github.com/your-org/tango-cli
cd tango-cli
cargo build --release
sudo cp target/release/tango /usr/local/bin/
```

### Prerequisites

- Rust 1.75+ (for building)
- Docker with Buildx plugin (for all registry operations)
- `docker login` credentials configured for your registry

---

## Core Concepts

### The Tagging Model

Tango generates tags deterministically from CI context:

| Event | Tags Applied |
|---|---|
| `commit` | `sha-<short_sha>` |
| `pull-request` | `sha-<short_sha>`, `pr-<number>` |
| `merge` | `sha-<short_sha>`, `main`, `latest` (if enabled) |

The `sha-` tag is the **immutable identity**. All other tags are mutable pointers that get repointed without rebuilding.

### Promotion Without Rebuild

On merge, tango uses `docker buildx imagetools create --tag` to repoint the manifest. No layers are re-pushed. The image that was built on the PR commit is the exact same binary that goes to production.

```
PR build:    sha-abc123def456   ──►  ghcr.io/org/api:sha-abc123def456
                                     ghcr.io/org/api:pr-42

Merge:       (no rebuild)       ──►  ghcr.io/org/api:main
                                     ghcr.io/org/api:latest
                                     (all pointing to same digest)
```

---

## Configuration

Tango reads configuration from `tango.yaml` in the current directory by default. Override with `--config` or `TANGO_CONFIG`.

### Root Config

```yaml
# tango.yaml
registry:
  host: ghcr.io
  namespace: my-org
  # username: my-bot    # prefer TANGO_REGISTRY_USER env var

defaults:
  tags:
    sha_prefix: "sha-"              # prefix for commit-sha tags
    pr_prefix: "pr-"                # prefix for PR tags
    main_tag: "main"                # tag applied on merge
    latest_tag: "latest"            # tag applied on merge (if enabled)
    publish_latest_on_merge: true   # whether to apply latest_tag on merge

packages:
  api:
    image:
      name: api
      context: ./services/api
      dockerfile: ./services/api/Dockerfile
      cache_from:
        - type=registry,ref=ghcr.io/my-org/api:buildcache
      cache_to:
        - type=registry,ref=ghcr.io/my-org/api:buildcache,mode=max
      platforms:
        - linux/amd64
        - linux/arm64
      build_args:
        NODE_ENV: production
      labels:
        org.opencontainers.image.source: https://github.com/my-org/my-repo

  worker:
    image:
      name: worker
      context: ./services/worker
      platforms:
        - linux/amd64
```

### Monorepo Packages

Each package can override `registry` and `defaults` independently:

```yaml
packages:
  frontend:
    image:
      name: frontend
      context: ./frontend
    registry:
      host: registry.hub.docker.com
      namespace: myorg
    defaults:
      tags:
        publish_latest_on_merge: false
        main_tag: "stable"
```

### Config Merging Priority

Configuration is resolved in this order (highest wins):

```
CLI flags  >  env vars  >  package config  >  root config  >  defaults
```

### Environment Variables

| Variable | Description |
|---|---|
| `TANGO_CONFIG` | Path to config file (default: `tango.yaml`) |
| `TANGO_PACKAGE` | Active package name |
| `TANGO_REGISTRY_HOST` | Override `registry.host` |
| `TANGO_REGISTRY_NAMESPACE` | Override `registry.namespace` |
| `TANGO_REGISTRY_USER` | Registry username |
| `TANGO_REGISTRY_PASSWORD` | Registry password / token (piped to `docker login --password-stdin`) |
| `TANGO_DRY_RUN` | Enable dry-run mode (`1` or `true`) |
| `TANGO_JSON` | Enable JSON output |
| `TANGO_EVENT` | CI event type (`commit`, `pr`, `merge`) |
| `TANGO_SHA` | Git commit SHA |
| `TANGO_PR_NUMBER` | Pull request number |

---

## Commands

### tango publish

Build an image and push it with commit/PR tags.

```
tango publish --event <EVENT> --sha <SHA> [OPTIONS]
```

**Flags:**

| Flag | Env | Description |
|---|---|---|
| `--event <EVENT>` | `TANGO_EVENT` | `commit`, `pr`, or `merge` |
| `--sha <SHA>` | `TANGO_SHA` | Git commit SHA |
| `--pr <NUMBER>` | `TANGO_PR_NUMBER` | PR number (required for `--event pr`) |
| `--build-arg KEY=VAL` | | Extra Docker build arg (repeatable) |
| `--label KEY=VAL` | | Extra Docker label (repeatable) |
| `--platform <PLATFORM>` | | Target platform, e.g. `linux/amd64` (repeatable) |
| `--cache-from <CACHE>` | | Buildx cache source (repeatable; replaces `image.cache_from`) |
| `--cache-to <CACHE>` | | Buildx cache destination (repeatable; replaces `image.cache_to`) |

Cache options also apply to builds performed by `tango dance` (including
`--force-build`), not promotion-only runs. Each CLI list independently replaces
its configured list; when omitted, the configured list is used. With neither
configured nor CLI cache options, no cache flags are passed. Values are forwarded
unchanged as separate Docker arguments; quote them in your shell. Buildx and the
selected builder must support the chosen cache backend. Tango redacts cache
values from command logs and hides build output when cache options are present
because Buildx may echo credentials. Avoid embedding credentials in cache values.

**Examples:**

```bash
# PR commit
tango publish --package api --event pr --sha abc123def456 --pr 42

# Standalone commit (no PR)
tango publish --package api --event commit --sha abc123def456

# With extra build args and multi-platform
tango publish --package api --event pr --sha abc123 --pr 42 \
  --build-arg VERSION=1.2.3 \
  --platform linux/amd64 \
  --platform linux/arm64

# JSON output
tango publish --package api --event pr --sha abc123 --pr 42 --json

# Override configured cache sources and destination
tango dance --package api --event commit --sha abc123 \
  --cache-from 'type=registry,ref=ghcr.io/my-org/api:branch-cache' \
  --cache-from 'type=registry,ref=ghcr.io/my-org/api:buildcache' \
  --cache-to 'type=registry,ref=ghcr.io/my-org/api:branch-cache,mode=max'
```

**JSON output:**

```json
{
  "image": "ghcr.io/my-org/api",
  "sha_tag": "sha-abc123def456",
  "pr_tag": "pr-42",
  "main_tag": null,
  "latest_tag": null,
  "tags": ["sha-abc123def456", "pr-42"],
  "digest": "sha256:a1b2c3d4...",
  "reference": "ghcr.io/my-org/api@sha256:a1b2c3d4..."
}
```

---

### tango promote

Promote an existing image to new tags without rebuilding. Uses `docker buildx imagetools create` — no layers are re-pushed.

```
tango promote --target-tag <TAG> [--target-tag <TAG>...] [--source <REF> | --sha <SHA>]
```

**Flags:**

| Flag | Env | Description |
|---|---|---|
| `--source <REF>` | | Full source reference (tag or `@digest`). Mutually exclusive with `--sha` |
| `--sha <SHA>` | `TANGO_SHA` | Derive source from `sha-<sha>` tag |
| `--target-tag <TAG>` | | Target tag (repeatable, required) |
| `--registry <HOST>` | | Override registry host |
| `--namespace <NS>` | | Override namespace |
| `--image <NAME>` | | Override image name |

**Examples:**

```bash
# Promote by SHA (most common on merge)
tango promote --package api --sha abc123def456 \
  --target-tag main \
  --target-tag latest

# Promote from an explicit reference
tango promote --package api \
  --source ghcr.io/my-org/api:sha-abc123def456 \
  --target-tag main

# Promote from digest (most precise)
tango promote --package api \
  --source "ghcr.io/my-org/api@sha256:a1b2c3..." \
  --target-tag staging

# JSON output
tango promote --package api --sha abc123 --target-tag main --json
```

**JSON output:**

```json
{
  "source": "ghcr.io/my-org/api:sha-abc123def456",
  "targets": [
    "ghcr.io/my-org/api:main",
    "ghcr.io/my-org/api:latest"
  ],
  "dry_run": false
}
```

---

### tango resolve

Resolve a tag to its canonical digest. Use this to pin image references for deployment.

```
tango resolve [--tag <TAG> | --ref <REF>]
```

**Flags:**

| Flag | Description |
|---|---|
| `--tag <TAG>` | Tag to resolve (uses package/config for registry/namespace/name) |
| `--ref <REF>` | Full image reference to resolve |
| `--registry <HOST>` | Override registry host |
| `--namespace <NS>` | Override namespace |
| `--image <NAME>` | Override image name |

**Examples:**

```bash
# Resolve the current main tag
tango resolve --package api --tag main

# Resolve a full reference
tango resolve --ref ghcr.io/my-org/api:sha-abc123def456

# Use in scripts to pin the digest
DIGEST=$(tango resolve --package api --tag main)
kubectl set image deployment/api app=ghcr.io/my-org/api@$DIGEST

# JSON output
tango resolve --package api --tag main --json
```

**JSON output:**

```json
{
  "reference": "ghcr.io/my-org/api:main",
  "digest": "sha256:a1b2c3d4...",
  "pinned": "ghcr.io/my-org/api@sha256:a1b2c3d4..."
}
```

---

### tango print-ref

Print the full image reference for a given tag or event context. Useful for injecting references into deployment manifests or CI job outputs.

```
tango print-ref [--tag <TAG> | --digest <DIGEST> | --event <EVENT> --sha <SHA>]
```

**Flags:**

| Flag | Env | Description |
|---|---|---|
| `--tag <TAG>` | | Explicit tag |
| `--digest <DIGEST>` | | Use digest instead of tag |
| `--event <EVENT>` | `TANGO_EVENT` | Derive tag from event + sha |
| `--sha <SHA>` | `TANGO_SHA` | Git commit SHA for tag derivation |
| `--pr <NUMBER>` | `TANGO_PR_NUMBER` | PR number (for `--event pr`) |
| `--select <sha\|pr\|main\|latest>` | | Which derived tag to print (default: `sha`) |
| `--registry <HOST>` | | Override registry host |
| `--namespace <NS>` | | Override namespace |
| `--image <NAME>` | | Override image name |

**Examples:**

```bash
# Print the sha-tag reference for the current commit
tango print-ref --package api --event pr --sha abc123def456 --pr 42
# → ghcr.io/my-org/api:sha-abc123def456

# Print the PR tag
tango print-ref --package api --event pr --sha abc123 --pr 42 --select pr
# → ghcr.io/my-org/api:pr-42

# Print a digest-pinned reference
tango print-ref --package api --digest sha256:a1b2c3...
# → ghcr.io/my-org/api@sha256:a1b2c3...

# Use in a Helm values file
IMAGE=$(tango print-ref --package api --event merge --sha $GITHUB_SHA)
helm upgrade myapp ./chart --set image.ref=$IMAGE
```

**JSON output:**

```json
{
  "reference": "ghcr.io/my-org/api:sha-abc123def456",
  "registry": "ghcr.io",
  "namespace": "my-org",
  "name": "api",
  "tag": "sha-abc123def456",
  "digest": null
}
```

---

### tango affected

Detect which packages have changed between two git refs. Compares changed file paths against each package's configured `context` directory.

```
tango affected [--base <REF>] [--head <REF>]
```

**Flags:**

| Flag | Default | Description |
|---|---|---|
| `--base <REF>` | `origin/main` | Base ref for the diff |
| `--head <REF>` | `HEAD` | Head ref for the diff |
| `--with-dependents` | | Include transitive dependents (future) |

**Examples:**

```bash
# Detect packages changed in current PR
tango affected --base origin/main --head HEAD

# Detect packages changed in a specific range
tango affected --base v1.2.0 --head v1.3.0

# JSON output (ideal for CI matrix)
tango affected --base origin/main --json
```

**JSON output:**

```json
{
  "base": "origin/main",
  "head": "HEAD",
  "changed_files": [
    "services/api/src/main.rs",
    "services/api/Dockerfile"
  ],
  "affected_packages": ["api"]
}
```

**Using as a CI matrix:**

```yaml
- name: Detect affected packages
  id: affected
  run: |
    PACKAGES=$(tango affected --base origin/${{ github.base_ref }} --json \
      | jq -c '.affected_packages')
    echo "packages=$PACKAGES" >> $GITHUB_OUTPUT

- name: Publish affected packages
  if: steps.affected.outputs.packages != '[]'
  strategy:
    matrix:
      package: ${{ fromJson(steps.affected.outputs.packages) }}
  run: tango publish --package ${{ matrix.package }} --event pr --sha ${{ github.sha }} --pr ${{ github.event.number }}
```

---

### tango dance

Orchestrate the full pipeline in a single command. On `pr`/`commit` events it builds and tags; on `merge` it either promotes an existing sha-tag or builds then promotes.

```
tango dance --event <EVENT> --sha <SHA> [OPTIONS]
```

**Flags:**

Combines all flags from `publish` plus:

| Flag | Description |
|---|---|
| `--promote-from-sha <REF>` | For merge events: full reference of already-built image to promote from. If omitted, builds fresh then promotes. |

**Examples:**

```bash
# Full pipeline for a PR (build + tag)
tango dance --package api --event pr --sha abc123def456 --pr 42

# Full pipeline for a merge (build + promote to main/latest)
tango dance --package api --event merge --sha abc123def456

# Merge with promotion from existing sha-tag (no rebuild)
tango dance --package api --event merge --sha abc123def456 \
  --promote-from-sha ghcr.io/my-org/api:sha-abc123def456

# Dry run to preview actions
tango dance --package api --event pr --sha abc123 --pr 42 --dry-run

# Full pipeline with JSON output
tango dance --package api --event merge --sha abc123 --json
```

**JSON output:**

```json
{
  "event": "merge",
  "image": "ghcr.io/my-org/api",
  "sha_tag": "sha-abc123def456",
  "pr_tag": null,
  "main_tag": "main",
  "latest_tag": "latest",
  "tags": ["sha-abc123def456", "main", "latest"],
  "digest": "sha256:a1b2c3d4...",
  "reference": "ghcr.io/my-org/api@sha256:a1b2c3d4...",
  "dry_run": false
}
```

---

## Global Flags

These flags apply to every command:

| Flag | Short | Env | Description |
|---|---|---|---|
| `--config <PATH>` | `-c` | `TANGO_CONFIG` | Config file path (default: `tango.yaml`) |
| `--package <NAME>` | `-p` | `TANGO_PACKAGE` | Target package (monorepo) |
| `--dry-run` | | `TANGO_DRY_RUN` | Print actions without executing |
| `--json` | | `TANGO_JSON` | Output results as JSON |
| `--verbose` | `-v` | | Enable debug logging |

---

## Workflow Model

### PR / Commit Flow

```
git push / PR opened
        │
        ▼
tango publish --event pr --sha $SHA --pr $PR_NUMBER
        │
        ├── docker buildx build --push -t ghcr.io/org/api:sha-abc123def456
        │
        └── docker buildx imagetools create --tag ghcr.io/org/api:pr-42 sha-abc123def456
```

### Merge Flow

```
PR merged to main
        │
        ▼
tango promote --sha $SHA --target-tag main --target-tag latest
        │
        └── docker buildx imagetools create --tag ghcr.io/org/api:main  sha-abc123def456
        └── docker buildx imagetools create --tag ghcr.io/org/api:latest sha-abc123def456
            (no docker build, no layer re-push)
```

### Deploy by Digest

```bash
# Always resolve to digest before deploying
DIGEST=$(tango resolve --package api --tag main)
# → sha256:a1b2c3d4...

# Use the pinned reference in your deployment
kubectl set image deployment/api app=ghcr.io/my-org/api@$DIGEST
```

---

## GitHub Actions Integration

A complete workflow is included at [`.github/workflows/ci.yaml`](.github/workflows/ci.yaml).

### Minimal PR workflow

```yaml
name: CI
on:
  pull_request:
    branches: [main]

jobs:
  publish:
    runs-on: ubuntu-latest
    permissions:
      contents: read
      packages: write
    steps:
      - uses: actions/checkout@v4

      - name: Set up Docker Buildx
        uses: docker/setup-buildx-action@v3

      - name: Log in to GHCR
        uses: docker/login-action@v3
        with:
          registry: ghcr.io
          username: ${{ github.actor }}
          password: ${{ secrets.GITHUB_TOKEN }}

      - name: Install tango
        run: cargo install --path .

      - name: Publish image
        env:
          TANGO_REGISTRY_HOST: ghcr.io
          TANGO_REGISTRY_NAMESPACE: ${{ github.repository_owner }}
        run: |
          tango publish \
            --package api \
            --event pr \
            --sha ${{ github.sha }} \
            --pr ${{ github.event.number }} \
            --json
```

### Minimal merge/promote workflow

```yaml
name: Promote on merge
on:
  push:
    branches: [main]

jobs:
  promote:
    runs-on: ubuntu-latest
    permissions:
      contents: read
      packages: write
    steps:
      - uses: actions/checkout@v4

      - name: Set up Docker Buildx
        uses: docker/setup-buildx-action@v3

      - name: Log in to GHCR
        uses: docker/login-action@v3
        with:
          registry: ghcr.io
          username: ${{ github.actor }}
          password: ${{ secrets.GITHUB_TOKEN }}

      - name: Install tango
        run: cargo install --path .

      - name: Promote to main
        env:
          TANGO_REGISTRY_HOST: ghcr.io
          TANGO_REGISTRY_NAMESPACE: ${{ github.repository_owner }}
        run: |
          tango promote \
            --package api \
            --sha ${{ github.sha }} \
            --target-tag main \
            --target-tag latest \
            --json
```

### Monorepo: affected packages matrix

```yaml
jobs:
  detect:
    runs-on: ubuntu-latest
    outputs:
      packages: ${{ steps.affected.outputs.packages }}
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0
      - name: Detect affected
        id: affected
        run: |
          PKGS=$(tango affected \
            --base "origin/${{ github.base_ref }}" \
            --json | jq -c '.affected_packages')
          echo "packages=$PKGS" >> $GITHUB_OUTPUT

  publish:
    needs: detect
    if: needs.detect.outputs.packages != '[]'
    strategy:
      matrix:
        package: ${{ fromJson(needs.detect.outputs.packages) }}
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Publish ${{ matrix.package }}
        run: |
          tango publish \
            --package ${{ matrix.package }} \
            --event pr \
            --sha ${{ github.sha }} \
            --pr ${{ github.event.number }}
```

---

## Architecture

```
src/
├── main.rs              # Tokio entry point, tracing setup
├── cli/
│   ├── mod.rs           # Cli struct, Commands enum, dispatch()
│   ├── publish.rs       # tango publish
│   ├── promote.rs       # tango promote
│   ├── resolve.rs       # tango resolve
│   ├── print_ref.rs     # tango print-ref
│   ├── affected.rs      # tango affected
│   └── dance.rs         # tango dance
├── config/
│   ├── mod.rs           # TangoConfig, PackageConfig, ResolvedConfig structs
│   └── loader.rs        # File loading + env var merge
├── core/
│   ├── tags.rs          # Event enum, TagSet, generate_tags()
│   └── image_ref.rs     # ImageRef (builder + Display + FromStr)
└── registry/
    ├── mod.rs           # RegistryClient trait
    └── client.rs        # DockerClient (shells to docker buildx)
```

### Registry abstraction

The `RegistryClient` trait decouples commands from the Docker CLI:

```rust
#[async_trait]
pub trait RegistryClient: Send + Sync {
    async fn push_image(&self, image_ref: &ImageRef, ...) -> Result<String>;
    async fn tag_image(&self, source: &ImageRef, targets: &[ImageRef], dry_run: bool) -> Result<()>;
    async fn get_digest(&self, image_ref: &ImageRef) -> Result<String>;
    async fn check_exists(&self, image_ref: &ImageRef) -> Result<bool>;
}
```

`DockerClient` is the production implementation. Alternative backends (native OCI HTTP, AWS ECR SDK, etc.) can be added without changing any command logic.

### Image references

All references are strongly typed:

```rust
// Build a reference
let image = ImageRef::new("ghcr.io", "my-org", "api")
    .with_tag("sha-abc123def456");
// → ghcr.io/my-org/api:sha-abc123def456

// Parse a reference
let image: ImageRef = "ghcr.io/my-org/api@sha256:a1b2c3...".parse()?;

// Switch to digest
let pinned = image.with_digest("sha256:a1b2c3...");
// → ghcr.io/my-org/api@sha256:a1b2c3...
```

---

## Development

```bash
# Build
cargo build

# Run with verbose output
cargo run -- --verbose publish --package api --event pr --sha abc123 --pr 1 --dry-run

# Check formatting
cargo fmt --check

# Lint
cargo clippy -- -D warnings

# Run tests
cargo test
```

### Adding a new registry backend

1. Implement `RegistryClient` for your type in `src/registry/`
2. Select it in `CommandContext` based on config or a `--backend` flag
3. No changes needed in any command handler

### Adding a new command

1. Create `src/cli/mycommand.rs` with `MyCommandArgs` (derives `clap::Args`) and `pub async fn run(ctx: CommandContext, args: MyCommandArgs) -> Result<()>`
2. Add `pub mod mycommand;` to `src/cli/mod.rs`
3. Add a variant to `Commands` and a match arm in `dispatch()`

---

## License

MIT
