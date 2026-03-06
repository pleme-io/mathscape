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

    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = {
    self,
    nixpkgs,
    fenix,
    substrate,
    forge,
    flake-utils,
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

      registry = "ghcr.io/pleme-io/mathscape";
      linuxSystems = ["x86_64-linux" "aarch64-linux"];

      commonBuildInputs =
        [pkgs.sqlite pkgs.pkg-config]
        ++ darwinBuildInputs;

      # ── Packages ─────────────────────────────────────────────────────
      mkMathscape = {
        pname,
        cargoBuildFlags ? ["--package" pname],
      }:
        rustPlatform.buildRustPackage {
          inherit pname cargoBuildFlags;
          version = "0.1.0";
          src = self;
          cargoLock.lockFile = ./Cargo.lock;
          nativeBuildInputs = [pkgs.pkg-config];
          buildInputs = commonBuildInputs;
        };

      service = mkMathscape {pname = "mathscape-service";};
      cli = mkMathscape {pname = "mathscape-cli";};
      mcp = mkMathscape {pname = "mathscape-mcp";};

      # ── Docker image (Linux only) ───────────────────────────────────
      mkImage = imgSystem: let
        imgPkgs = import nixpkgs {system = imgSystem;};
        imgService = mkMathscape {pname = "mathscape-service";};
      in
        imgPkgs.dockerTools.buildLayeredImage {
          name = registry;
          tag = "latest";
          contents = with imgPkgs; [
            imgService
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
              "SSL_CERT_FILE=${imgPkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
            ];
            ExposedPorts = {
              "8080/tcp" = {};
              "9090/tcp" = {};
            };
            Volumes = {
              "/data" = {};
            };
            Labels = {
              "org.opencontainers.image.source" = "https://github.com/pleme-io/mathscape";
              "org.opencontainers.image.description" = "Mathscape evolutionary symbolic compression engine";
            };
          };
        };

      # ── Helm chart apps ──────────────────────────────────────────────
      helmApps = substrateLib.mkHelmSdlcApps {
        name = "mathscape";
        chartDir = ./deploy/charts/mathscape;
        libChartDir = ./deploy/charts/pleme-lib;
        registry = "oci://ghcr.io/pleme-io/charts";
      };
    in {
      # ── Packages ───────────────────────────────────────────────────
      packages =
        {
          default = service;
          mathscape-service = service;
          mathscape-cli = cli;
          mathscape-mcp = mcp;
        }
        // lib.optionalAttrs (builtins.elem system linuxSystems) {
          image = mkImage system;
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
          release = substrateLib.mkImageReleaseApp {
            name = "mathscape";
            inherit registry mkImage;
          };
        }
        // helmApps;

      # ── Dev shell (using substrate mkRustDevShell) ─────────────────
      devShells.default = substrateLib.mkRustDevShell {
        withSqlite = true;
        withHelm = true;
        withKubernetes = true;
        withDocker = true;
        extraPackages = with pkgs; [
          rust-analyzer
          cargo-watch
        ];
        extraEnv = {
          DATA_DIR = "./data";
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
    };
}
