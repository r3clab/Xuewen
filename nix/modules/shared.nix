# Shared core of the NixOS and Home Manager modules (nixos/xuewen.nix and
# home/xuewen.nix): the option set both declare, and everything derived from a
# module's `cfg`, live here once so the two cannot drift. Platform-only options
# (user/group, openFirewall) and the systemd wiring stay in the modules.
#
# `modules` names the flake attribute whose `default` wrapper fills in the
# package options ("nixosModules" or "homeModules"); it appears in doc
# strings and the package assertion. `dataDirDefault` holds the platform's
# `default` (and `defaultText`) for `dataDir`.
{
  lib,
  pkgs,
  modules,
  environmentFileExample,
  dataDirDefault,
}:
rec {
  tomlFormat = pkgs.formats.toml { };

  options = {
    enable = lib.mkEnableOption "Xuewen, a self-hosted reference manager";

    package = lib.mkOption {
      # nullOr with a null default rather than no default: evaluating the
      # assertion below forces the option, and without a default the module
      # system would abort first with its generic "used but not defined"
      # error instead of the assertion's guidance.
      type = lib.types.nullOr lib.types.package;
      default = null;
      defaultText = lib.literalMD "the flake's `xuewen` package (via `${modules}.default`)";
      description = ''
        The xuewen package to run. `${modules}.default` sets this to the
        flake's build; with the bare `${modules}.xuewen` you must set it.
      '';
    };

    agentRunnerPackage = lib.mkOption {
      type = lib.types.nullOr lib.types.package;
      default = null;
      defaultText = lib.literalMD "the flake's `agent-runner` package (via `${modules}.default`)";
      description = ''
        The Node runner behind Agent Ask (`[ai.agent.*]`), used to default
        `settings.ai.agent.runner`. `${modules}.default` sets this to the
        flake's build; with the bare `${modules}.xuewen`, leaving it `null`
        means you must set `settings.ai.agent.runner` yourself. Ignored when
        `[ai.agent.*]` is absent from {option}`services.xuewen.settings`.
      '';
    };

    host = lib.mkOption {
      type = lib.types.str;
      default = "127.0.0.1";
      description = ''
        Address to bind. The web UI has no authentication and exposes mutating
        endpoints, so the backend refuses non-loopback binds unless
        {option}`services.xuewen.allowRemote` is set.
      '';
    };

    port = lib.mkOption {
      type = lib.types.port;
      default = 8080;
      description = "TCP port to bind.";
    };

    allowRemote = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Pass `--allow-remote` so the backend accepts binding the non-loopback
        {option}`services.xuewen.host`. The web UI has no authentication and
        exposes mutating endpoints; only enable this behind an authenticating
        reverse proxy or on a trusted network.
      '';
    };

    dataDir = lib.mkOption (
      {
        type = lib.types.path;
        description = ''
          State directory holding the inbox, library, SQLite database, search
          index, vector store and preview cache. Created automatically.
        '';
      }
      // dataDirDefault
    );

    environmentFile = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      example = environmentFileExample;
      description = ''
        A systemd `EnvironmentFile` holding secrets that must stay out of the
        world-readable Nix store — e.g. `OPENAI_API_KEY=sk-…` for the
        `api_key_env` referenced by `[ai.*]`.
      '';
    };

    settings = lib.mkOption {
      inherit (tomlFormat) type;
      default = { };
      example = lib.literalExpression ''
        {
          ai = {
            api_key_env = "OPENAI_API_KEY";
            model = "gpt-4o-mini";
            embedding = { model = "text-embedding-3-small"; dims = 1536; };
            summary = { };
            # Paper chat / the Ask tab is Agent Ask — set [ai.agent.*] separately.
          };
        }
      '';
      description = ''
        `xuewen.toml` as a Nix attrset. `data_dir` is
        {option}`services.xuewen.dataDir`, and every path the backend keeps
        (inbox, library, database, indexes, preview cache) defaults under it;
        set the individual keys here to override.

        Do NOT put API keys here — the generated file lands in the
        world-readable Nix store. Use `api_key_env` together with
        {option}`services.xuewen.environmentFile` instead.
      '';
    };
  };

  mkXuewen = cfg: rec {
    # Agent Ask ([ai.agent.*]) spawns `node` for its runner; node's JIT needs
    # writable-then-executable mappings, so its presence also decides each
    # module's MemoryDenyWriteExecute hardening.
    agentConfigured = lib.hasAttrByPath [ "ai" "agent" ] cfg.settings;

    derivedSettings = {
      data_dir = toString cfg.dataDir;
    }
    # The default `[ai.agent].runner` (`agent-runner/src/runner.mjs`) only
    # exists in a dev checkout. Point it at the store copy so Agent Ask needs
    # no unpackaged files under dataDir.
    // lib.optionalAttrs (agentConfigured && cfg.agentRunnerPackage != null) {
      ai.agent.runner = "${cfg.agentRunnerPackage}/lib/xuewen/agent-runner/src/runner.mjs";
    };

    configFile = tomlFormat.generate "xuewen.toml" (lib.recursiveUpdate derivedSettings cfg.settings);

    # `--allow-remote` comes from the explicit allowRemote option, never from
    # guessing at `host`: a Nix mirror of the backend's
    # `cli::serve::is_loopback_host` diverged at the edges, turning the
    # backend's deliberate refusal of a non-loopback bind into a silent 5s
    # crash loop.
    execStart = lib.escapeShellArgs (
      [
        "${cfg.package}/bin/xuewen"
        "--config"
        "${configFile}"
        "serve"
        "--host"
        cfg.host
        "--port"
        (toString cfg.port)
      ]
      ++ lib.optional cfg.allowRemote "--allow-remote"
    );

    # The package itself brings pdftotext and git; Agent Ask's runner needs
    # node, and Claude Code's Grep tool ripgrep.
    runtimePackages = lib.optionals agentConfigured [
      pkgs.nodejs
      pkgs.ripgrep
    ];

    environment = lib.optionalAttrs agentConfigured {
      # Claude Code's CLI otherwise extracts its own ripgrep out of the Bun
      # binary into a temp dir and execs it — unpatched, so it finds no ELF
      # interpreter on a NixOS host and the Grep tool dies. Same fix nixpkgs'
      # claude-code applies: turn it off and put a real ripgrep on PATH.
      USE_BUILTIN_RIPGREP = "0";
    };

    assertions = [
      {
        assertion = cfg.package != null;
        message = "services.xuewen.package must be set (use ${modules}.default, or set it explicitly).";
      }
      {
        # The loopback literals the backend always accepts. Anything else —
        # including other loopbacks like 127.0.0.2, for which the flag is
        # harmless — needs the explicit opt-in, or the started unit would
        # crash-loop on the backend's own refusal.
        assertion =
          cfg.allowRemote
          || lib.elem cfg.host [
            "127.0.0.1"
            "localhost"
            "::1"
          ];
        message = ''services.xuewen.host = "${cfg.host}" is not a loopback literal and the backend refuses such binds; set services.xuewen.allowRemote = true to opt in (the web UI has no auth — put an authenticating reverse proxy in front).'';
      }
    ];

    warnings = lib.optional cfg.allowRemote "services.xuewen.allowRemote is enabled: the web UI has no auth and serves mutating endpoints to any host that can reach ${cfg.host}. Put an authenticating reverse proxy in front.";
  };
}
