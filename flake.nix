{
  description = "Mathscape — evolutionary symbolic compression engine";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11";

    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    substrate = {
      url = "github:pleme-io/substrate";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.fenix.follows = "fenix";
    };

    forge = {
      url = "github:pleme-io/forge";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.fenix.follows = "fenix";
      inputs.substrate.follows = "substrate";
    };

    hanabi = {
      url = "github:pleme-io/hanabi";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.fenix.follows = "fenix";
      inputs.substrate.follows = "substrate";
    };

    helmworks = {
      url = "github:pleme-io/helmworks";
      flake = false;
    };

    pleme-linker = {
      url = "github:pleme-io/pleme-linker";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    flake-utils.url = "github:numtide/flake-utils";
    devenv = {
      url = "github:cachix/devenv";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = {
    self,
    nixpkgs,
    fenix,
    substrate,
    forge,
    hanabi,
    helmworks,
    pleme-linker,
    flake-utils,
    ...
  }:
    flake-utils.lib.eachDefaultSystem (system: let
      # ── Pkgs with substrate rust overlay ──────────────────────────────
      rustOverlay = import "${substrate}/lib/rust-overlay.nix";
      pkgs = import nixpkgs {
        inherit system;
        overlays = [(rustOverlay.mkRustOverlay {inherit fenix system;})];
      };

      lib = pkgs.lib;
      darwinBuildInputs = (import "${substrate}/lib/darwin.nix").mkDarwinBuildInputs pkgs;

      substrateLib = substrate.libFor {
        inherit pkgs system;
        fenix = fenix.packages.${system};
        forge = forge.packages.${system}.forge-cli or null;
      };

      # ── Build configuration ──────────────────────────────────────────
      rustToolchain = pkgs.fenixRustToolchain;
      rustPlatform = pkgs.makeRustPlatform {
        rustc = rustToolchain;
        cargo = rustToolchain;
      };

      registryEngine = "ghcr.io/pleme-io/mathscape";
      registryWeb = "ghcr.io/pleme-io/mathscape-web";
      linuxSystems = ["x86_64-linux" "aarch64-linux"];

      commonBuildInputs =
        [pkgs.openssl pkgs.pkg-config pkgs.postgresql]
        ++ darwinBuildInputs;

      # ── Target-system Rust build helper ────────────────────────────
      # Builds Rust binaries targeting the given Linux system.
      # Remote builders handle cross-compilation transparently.
      mkTargetRustPlatform = targetSystem: let
        targetPkgs = import nixpkgs {
          system = targetSystem;
          overlays = [(rustOverlay.mkRustOverlay {inherit fenix; system = targetSystem;})];
        };
        tc = targetPkgs.fenixRustToolchain;
      in {
        inherit targetPkgs;
        platform = targetPkgs.makeRustPlatform {rustc = tc; cargo = tc;};
        darwinInputs = (import "${substrate}/lib/darwin.nix").mkDarwinBuildInputs targetPkgs;
      };

      mkTargetBin = targetSystem: pname: let
        t = mkTargetRustPlatform targetSystem;
      in
        t.platform.buildRustPackage {
          inherit pname;
          version = "0.1.0";
          src = self;
          cargoLock = {
            lockFile = ./Cargo.lock;
            allowBuiltinFetchGit = true;
          };
          cargoBuildFlags = ["--package" pname];
          nativeBuildInputs = [t.targetPkgs.pkg-config t.targetPkgs.protobuf];
          buildInputs = [t.targetPkgs.openssl t.targetPkgs.pkg-config t.targetPkgs.postgresql]
            ++ t.darwinInputs;
        };

      # ── Host Rust packages (for dev shell, CLI usage) ──────────────
      mkMathscape = {
        pname,
        cargoBuildFlags ? ["--package" pname],
      }:
        rustPlatform.buildRustPackage {
          inherit pname cargoBuildFlags;
          version = "0.1.0";
          src = self;
          cargoLock = {
            lockFile = ./Cargo.lock;
            allowBuiltinFetchGit = true;
          };
          nativeBuildInputs = [pkgs.pkg-config pkgs.protobuf];
          buildInputs = commonBuildInputs;
        };

      service = mkMathscape {pname = "mathscape-service";};
      cli = mkMathscape {pname = "mathscape-cli";};
      mcp = mkMathscape {pname = "mathscape-mcp";};
      db = mkMathscape {pname = "mathscape-db";};

      # ── Frontend build (substrate mkViteBuild pattern) ──────────────
      webBuild = import "${substrate}/lib/web-build.nix" {inherit pkgs lib;};
      frontend = webBuild.mkViteBuild {
        appName = "mathscape-frontend";
        src = self + "/frontend";
        npmDepsHash = lib.fakeHash;
        buildScript = "build";
      };

      # ── Engine Docker image (Linux only) ────────────────────────────
      # Pure Rust service: REST + GraphQL + gRPC APIs.
      # No frontend — that goes to hanabi via mathscape-web image.
      mkEngineImage = imgSystem: let
        imgPkgs = import nixpkgs {system = imgSystem;};
        imgService = mkTargetBin imgSystem "mathscape-service";
        imgDb = mkTargetBin imgSystem "mathscape-db";
        dockerHelpers = import "${substrate}/lib/docker-helpers.nix";
      in
        imgPkgs.dockerTools.buildLayeredImage {
          name = registryEngine;
          tag = "latest";
          architecture =
            if imgSystem == "x86_64-linux"
            then "amd64"
            else "arm64";
          contents = with imgPkgs; [
            imgService
            imgDb
            busybox
            cacert
            coreutils
          ];
          extraCommands = ''
            mkdir -p data tmp
            chmod 1777 tmp
          '';
          config = {
            Cmd = ["${imgService}/bin/mathscape-service"];
            Env = [
              "RUST_LOG=info,mathscape=debug"
              "DATA_DIR=/data"
              "LOG_FORMAT=json"
              (dockerHelpers.mkSslEnv imgPkgs)
            ];
            ExposedPorts = {
              "8080/tcp" = {};
              "8081/tcp" = {};
              "9090/tcp" = {};
            };
            Volumes = {
              "/data" = {};
            };
            User = "1000";
            Labels = {
              "org.opencontainers.image.source" = "https://github.com/pleme-io/mathscape";
              "org.opencontainers.image.description" = "Mathscape engine — evolutionary symbolic compression";
            };
          };
        };

      # ── Web Docker image (Linux only) ───────────────────────────────
      # Frontend dist served by hanabi BFF (same pattern as lilitu-web).
      # Uses target-system pkgs for proper Linux images from any host.
      mkWebImage = imgSystem: let
        imgPkgs = import nixpkgs {system = imgSystem;};
        imgWebDocker = import "${substrate}/lib/web-docker.nix" {
          pkgs = imgPkgs;
          defaultAtticToken = "";
          defaultGhcrToken = "";
          forgeCmd = "forge";
        };
        hanabiPkg = hanabi.packages.${imgSystem}.default;
      in
        imgWebDocker.mkNodeDockerImage {
          appName = "mathscape-web";
          builtApp = frontend;
          webServer = hanabiPkg;
          architecture =
            if imgSystem == "x86_64-linux"
            then "amd64"
            else "arm64";
        };

      # ── Helm chart apps ──────────────────────────────────────────────
      helmApps = substrateLib.mkHelmAllApps {
        charts = [
          {
            name = "mathscape";
            chartDir = ./deploy/charts/mathscape;
          }
          {
            name = "mathscape-web";
            chartDir = ./deploy/charts/mathscape-web;
          }
        ];
        libChartDir = "${helmworks}/charts/pleme-lib";
        registry = "oci://ghcr.io/pleme-io/charts";
      };

      # ── Image release apps (substrate multi-image pattern) ──────────
      releaseApps = substrateLib.mkImageReleaseApps {
        engine = {
          registry = registryEngine;
          mkImage = mkEngineImage;
        };
        web = {
          registry = registryWeb;
          mkImage = mkWebImage;
        };
      };
    in {
      # ── Packages ───────────────────────────────────────────────────
      packages =
        {
          default = service;
          mathscape-service = service;
          mathscape-cli = cli;
          mathscape-mcp = mcp;
          mathscape-db = db;
          mathscape-frontend = frontend;
        }
        // lib.optionalAttrs (builtins.elem system linuxSystems) {
          image = mkEngineImage system;
          image-web = mkWebImage system;
        };

      # ── Apps ───────────────────────────────────────────────────────
      apps =
        {
          default = {
            type = "app";
            program = "${service}/bin/mathscape-service";
          };
          cli = {
            type = "app";
            program = "${cli}/bin/mathscape-cli";
          };
          mcp = {
            type = "app";
            program = "${mcp}/bin/mathscape-mcp";
          };
          db = {
            type = "app";
            program = "${db}/bin/mathscape-db";
          };

          # ── Local development ──────────────────────────────────────────
          dev = {
            type = "app";
            program = toString (pkgs.writeShellScript "mathscape-dev" ''
              set -euo pipefail
              REPO_ROOT=$(${pkgs.git}/bin/git rev-parse --show-toplevel)
              cd "$REPO_ROOT"

              echo "Starting postgres..."
              ${pkgs.docker-compose}/bin/docker-compose up -d postgres

              echo "Waiting for postgres..."
              timeout=30
              elapsed=0
              until ${pkgs.docker}/bin/docker exec "$(${pkgs.docker-compose}/bin/docker-compose ps -q postgres)" \
                pg_isready -U mathscape -d mathscape > /dev/null 2>&1; do
                if [ $elapsed -ge $timeout ]; then
                  echo "Timeout waiting for postgres"
                  exit 1
                fi
                sleep 1
                elapsed=$((elapsed + 1))
              done
              echo "Postgres ready."

              echo "Running migrations..."
              DATABASE_URL="postgres://mathscape:mathscape@localhost/mathscape" \
                ${db}/bin/mathscape-db migrate

              echo "Starting engine (cargo run)..."
              export DATABASE_URL="postgres://mathscape:mathscape@localhost/mathscape"
              export DATA_DIR="./data"
              export STATIC_DIR="./frontend/dist"
              export RUST_LOG="debug"
              exec cargo run --package mathscape-service
            '');
          };

          dev-down = {
            type = "app";
            program = toString (pkgs.writeShellScript "mathscape-dev-down" ''
              set -euo pipefail
              REPO_ROOT=$(${pkgs.git}/bin/git rev-parse --show-toplevel)
              cd "$REPO_ROOT"
              ${pkgs.docker-compose}/bin/docker-compose down
            '');
          };

          migrate = {
            type = "app";
            program = toString (pkgs.writeShellScript "mathscape-migrate" ''
              set -euo pipefail
              DATABASE_URL="''${DATABASE_URL:-postgres://mathscape:mathscape@localhost/mathscape}" \
                exec ${db}/bin/mathscape-db migrate
            '');
          };

          "db:reset" = {
            type = "app";
            program = toString (pkgs.writeShellScript "mathscape-db-reset" ''
              set -euo pipefail
              DATABASE_URL="''${DATABASE_URL:-postgres://mathscape:mathscape@localhost/mathscape}" \
                exec ${db}/bin/mathscape-db reset
            '');
          };

          "db:status" = {
            type = "app";
            program = toString (pkgs.writeShellScript "mathscape-db-status" ''
              set -euo pipefail
              DATABASE_URL="''${DATABASE_URL:-postgres://mathscape:mathscape@localhost/mathscape}" \
                exec ${db}/bin/mathscape-db status
            '');
          };
        }
        // releaseApps
        // helmApps;

      # ── Dev shell (using substrate mkRustDevShell) ─────────────────
      devShells.default = substrateLib.mkRustDevShell {
        withHelm = true;
        withKubernetes = true;
        withDocker = true;
        extraPackages = with pkgs; [
          rust-analyzer
          cargo-watch
          postgresql
          sea-orm-cli
          nodejs_22
          protobuf
        ];
        extraEnv = {
          DATA_DIR = "./data";
          STATIC_DIR = "./frontend/dist";
          DATABASE_URL = "postgres://localhost/mathscape";
          RUST_LOG = "debug";
        };
      };
    })
    // {
      # ── Overlay ──────────────────────────────────────────────────────
      overlays.default = final: prev: {
        mathscape-service = self.packages.${final.system}.mathscape-service;
        mathscape-cli = self.packages.${final.system}.mathscape-cli;
        mathscape-mcp = self.packages.${final.system}.mathscape-mcp;
      };

      # ── Home Manager module ─────────────────────────────────────────
      #
      # blackmatter.components.mathscape = {
      #   enable = true;
      #   mcp.enable = true;
      # };
      #
      # Deploys:
      #   - ~/.config/mathscape-mcp/mathscape-mcp.yaml  (shikumi-style)
      #   - anvil.mcp.servers.mathscape  (Claude Code sees the tools)
      homeManagerModules.default = import ./module {
        mathscapePackages = self.packages;
      };
    };
}
