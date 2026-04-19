# mathscape — Home Manager module.
#
# Two responsibilities:
#
#   1. Register the mathscape-mcp binary with blackmatter-anvil
#      so Claude Code / Cursor / any MCP client can invoke the
#      running model's tools (step, eval_expr, curriculum_score,
#      list_rules, identify_rule, etc.).
#
#   2. Deploy a shikumi-style YAML config at
#      ~/.config/mathscape-mcp/mathscape-mcp.yaml holding the
#      runtime knobs (engine ticks, step limit, default learning
#      rate). Nix → YAML → app follows the pleme-io convention
#      ("Prefer shikumi-style Nix→YAML→app config patterns").
#
# Usage (in a home-manager profile):
#
#   blackmatter.components.mathscape = {
#     enable = true;
#     mcp.enable = true;     # register MCP server with anvil
#     mcp.scopes = ["pleme"]; # which claude contexts get this
#     settings = {
#       step_limit = 2000;
#       epochs_per_tick = 1;
#       default_learning_rate = 0.1;
#     };
#   };
{ mathscapePackages }:
{ lib, config, pkgs, ... }:
with lib;
let
  cfg = config.blackmatter.components.mathscape;
  homeDir = config.home.homeDirectory;

  # Resolve the mathscape-mcp binary for the host system. The
  # caller supplies mathscapePackages keyed by system.
  systemPackages =
    mathscapePackages.${pkgs.stdenv.hostPlatform.system} or null;

  mcpBin =
    if systemPackages != null
    then "${systemPackages.mathscape-mcp}/bin/mathscape-mcp"
    else "mathscape-mcp"; # fallback to PATH

  # shikumi-style YAML config. Nix options → YAML file the
  # running mathscape-mcp reads at
  # ~/.config/mathscape-mcp/mathscape-mcp.yaml.
  yamlFormat = pkgs.formats.yaml {};

  configFile = yamlFormat.generate "mathscape-mcp.yaml" cfg.settings;
in {
  options.blackmatter.components.mathscape = {
    enable = mkEnableOption "Mathscape live model + MCP integration";

    mcp = {
      enable = mkOption {
        type = types.bool;
        default = true;
        description = ''
          Register the mathscape-mcp binary with blackmatter-anvil
          so Claude Code (and other MCP clients) can drive the
          running model: step the motor, eval expressions, score
          the curriculum, inspect the library live.
        '';
      };

      scopes = mkOption {
        type = types.listOf types.str;
        default = []; # empty = all scopes
        example = [ "pleme" ];
        description = ''
          Anvil scope restriction. Empty list = available in every
          claude context. Use e.g. ["pleme"] to restrict to the
          pleme context wrapper.
        '';
      };

      agents = mkOption {
        type = types.listOf types.str;
        default = []; # empty = all agents
        example = [ "claude" "cursor" ];
        description = ''
          Anvil agent restriction. Empty list = every agent.
        '';
      };
    };

    settings = mkOption {
      type = yamlFormat.type;
      default = {
        # Engine loop
        epochs_per_tick = 1;
        step_limit = 1000;

        # Training
        default_learning_rate = 0.1;
        ewc_lambda = 0.0;            # disabled by default
        learning_progress_window = 5;

        # Neuroplasticity (coach-controlled at runtime; these
        # are seed values)
        prune_magnitude_threshold = 1e-6;
        prune_min_activations = 1;
        rejuvenate_phantom_threshold = 0.001;
        rejuvenate_initial_value = 0.01;

        # Curriculum
        curriculum_tick_every = 5;   # run curriculum every N epochs
      };
      description = ''
        shikumi-style YAML config for mathscape-mcp. Written to
        ~/.config/mathscape-mcp/mathscape-mcp.yaml on activation.
        The running mathscape-mcp reads this file at startup.

        These defaults mirror the knob surface exposed through
        the Coach's TuningAction catalog. Operators can override
        them per-host; the Coach retains runtime authority.
      '';
    };
  };

  config = mkIf cfg.enable (mkMerge [
    # Always deploy the shikumi YAML.
    {
      home.file.".config/mathscape-mcp/mathscape-mcp.yaml".source =
        configFile;
    }

    # Register with anvil when MCP is enabled.
    (mkIf cfg.mcp.enable {
      blackmatter.components.anvil.mcp.servers.mathscape = {
        command = mcpBin;
        args = [];
        env = {
          MATHSCAPE_CONFIG =
            "${homeDir}/.config/mathscape-mcp/mathscape-mcp.yaml";
        };
        description = ''
          Mathscape — live mathematical model. Tools: step (run
          one epoch), eval_expr (inference on live library),
          curriculum_score (per-subdomain competency),
          list_rules (discovered library), identify_rule
          (match against known-math catalog). Talk to the model
          while it trains.
        '';
        scopes = cfg.mcp.scopes;
        agents = cfg.mcp.agents;
      };
    })
  ]);
}
