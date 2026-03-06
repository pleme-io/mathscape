# Deployment Architecture

## Overview

Mathscape is deployed as a StatefulSet in Kubernetes with persistent
storage for the expression database (redb) and metadata (SQLite).
The Helm chart follows helmworks conventions with pleme-lib dependency.

## Build Pipeline

```
Source (Rust workspace)
    │
    ▼
nix build .#image          Build Docker image via buildLayeredImage
    │
    ▼
nix run .#release          Push multi-arch image to GHCR via skopeo
    │                      Tags: amd64-<sha>, amd64-latest
    ▼
nix run .#helm:release     Lint, package, push Helm chart to OCI registry
    │                      Registry: oci://ghcr.io/pleme-io/charts
    ▼
FluxCD HelmRelease         Reconciles chart + values from k8s repo
    │
    ▼
StatefulSet + PVC          Running in Kubernetes with persistent /data
```

## Docker Image

Built with `pkgs.dockerTools.buildLayeredImage` (Nix-native, no Dockerfile):

- **Base**: busybox + cacert + coreutils (minimal, ~15 MB)
- **Binary**: `mathscape-service` (statically linked where possible)
- **Ports**: 8080 (HTTP), 9090 (metrics)
- **Volume**: /data (expression store + metadata)
- **User**: 1000:1000 (non-root)
- **Read-only rootfs**: yes (writes only to /data and /tmp)

## Helm Chart: `mathscape`

### Chart Structure

```
deploy/charts/mathscape/
  Chart.yaml                 depends on pleme-lib ~0.4.0
  values.yaml                StatefulSet defaults
  templates/
    _helpers.tpl             delegates to pleme-lib
    statefulset.yaml         StatefulSet with volumeClaimTemplates
    service.yaml             ClusterIP service (HTTP + metrics)
    serviceaccount.yaml      pleme-lib.serviceaccount
    servicemonitor.yaml      Prometheus ServiceMonitor
    networkpolicy.yaml       pleme-lib.networkpolicy
    pdb.yaml                 PodDisruptionBudget (disabled by default)
    configmap.yaml           Optional engine configuration
  tests/
    statefulset_test.yaml    helm-unittest: StatefulSet assertions
    service_test.yaml        helm-unittest: Service assertions
    pvc_test.yaml            helm-unittest: PVC assertions
```

### Key Design Decisions

**StatefulSet (not Deployment)**: Mathscape needs stable persistent
storage. A Deployment with PVC would work for single-replica, but
StatefulSet provides stable network identity and ordered
scaling/termination — important for database integrity.

**Single replica**: The engine is deterministic and single-threaded
by design. Running multiple replicas would produce divergent
traversals. `replicaCount: 1` is the default and recommended setting.

**Large memory limit**: Expression evaluation and e-graph saturation
are memory-intensive. Default: 256Mi request, 2Gi limit. Adjust based
on population size and library complexity.

**Persistent storage**: Default 10Gi PVC. Estimated growth:
~50 KB/epoch, ~500 MB after 10k epochs, ~5 GB after 100k epochs.
Resize as needed.

## Kubernetes Manifest (FluxCD)

```yaml
# k8s/clusters/plo/infrastructure/mathscape/kustomization.yaml
apiVersion: kustomize.config.k8s.io/v1beta1
kind: Kustomization
namespace: mathscape
resources:
  - namespace.yaml
  - helmrelease.yaml

# k8s/clusters/plo/infrastructure/mathscape/helmrelease.yaml
apiVersion: helm.toolkit.fluxcd.io/v2
kind: HelmRelease
metadata:
  name: mathscape
  namespace: mathscape
spec:
  interval: 5m
  chart:
    spec:
      chart: mathscape
      version: "0.1.0"
      sourceRef:
        kind: HelmRepository
        name: pleme-charts
        namespace: flux-system
  values:
    image:
      repository: ghcr.io/pleme-io/mathscape
      tag: amd64-<sha>
    persistence:
      size: 20Gi
    resources:
      requests:
        cpu: 500m
        memory: 1Gi
      limits:
        cpu: 2000m
        memory: 4Gi
```

## Nix Build Commands

```bash
# Development
nix develop             # Enter devShell (Rust + SQLite + Helm + kubectl)
cargo build             # Build all crates
cargo test              # Run all tests
cargo run -p mathscape-cli   # Interactive REPL

# Docker image
nix build .#image       # Build Docker image (Linux only)

# Release
nix run .#release       # Push multi-arch image to GHCR

# Helm
nix run .#lint:mathscape     # Lint chart
nix run .#release:mathscape  # Lint + package + push chart to OCI
nix run .#template -- mathscape deploy/charts/values-production.yaml
```

## Monitoring

The ServiceMonitor scrapes `/metrics` on port 9090. Prometheus
recording rules and alerts can be added via the chart's
`monitoring` values.

Key alerts to configure:
- `MathscapeEngineStalled` — no new epoch in 10 minutes
- `MathscapeHighMemory` — memory usage > 80% of limit
- `MathscapeStorageNearFull` — PVC usage > 80%
- `MathscapeCompressionPlateau` — CR unchanged for 100+ epochs

## Security

- Non-root container (UID 1000)
- Read-only root filesystem
- No privilege escalation
- Capabilities dropped (ALL)
- NetworkPolicy: deny-all base + allow DNS + allow Prometheus scrape
- No inbound mutations — the HTTP API is entirely read-only

## Substrate Patterns Used

| Pattern | Source | Purpose |
|---|---|---|
| `mkRustOverlay` | `substrate/lib/rust-overlay.nix` | Fenix Rust toolchain |
| `mkRustDevShell` | `substrate/lib/rust-devenv.nix` | Development environment |
| `mkDarwinBuildInputs` | `substrate/lib/darwin.nix` | macOS SDK compatibility |
| `mkImageReleaseApp` | `substrate/lib/image-release.nix` | Multi-arch Docker push |
| `mkHelmSdlcApps` | `substrate/lib/helm-build.nix` | Helm lint/package/push/release |
| `buildLayeredImage` | `nixpkgs/dockerTools` | Nix-native Docker image build |
