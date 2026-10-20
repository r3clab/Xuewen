# Shared core of the NixOS and Home Manager modules (nixos/xuewen.nix and
# home/xuewen.nix): the option set both declare verbatim, and everything
# derived from a module's `cfg`, live here once so the two cannot drift.
# Platform-only options (dataDir, user/group, openFirewall) and the systemd
# wiring stay in the modules.
#
# `modules` names the flake attribute whose `default` wrapper fills in the
# package options ("nixosModules" or "homeModules"); it appears in doc
# strings and the package assertion.
{
  lib,
  pkgs,
  modules,
  environmentFileExample,
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
        `xuewen.toml` as a Nix attrset. `inbox_dir`, `library_root`,
        `database_url`, `search.index_dir` and `search.vector_dir` default
        under {option}`services.xuewen.dataDir`; set them here to override.

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

    # Paths the backend requires. They default under `dataDir`; anything the
    # user puts in `settings` wins (recursiveUpdate is deep, so setting
    # `settings.search.index_dir` keeps the `vector_dir` default below).
    derivedSettings = {
      inbox_dir = "${cfg.dataDir}/inbox";
      library_root = "${cfg.dataDir}/library";
      database_url = "sqlite:${cfg.dataDir}/library.db";
      search.index_dir = "${cfg.dataDir}/search-index";
      search.vector_dir = "${cfg.dataDir}/search-vectors";
      preview.cache_dir = "${cfg.dataDir}/preview-cache";
    }
    # The backend resolves `[ai.agent].runner` against its working directory,
    # whose default (`agent-runner/src/runner.mjs`) only exists in a dev
    # checkout. Point it at the store copy so Agent Ask needs no unpackaged
    # files under dataDir.
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

    # pdftotext (poppler-utils) is required for PDF text extraction, which the
    # ingest pipeline and paper chat both depend on. git backs the repo-attach
    # endpoint (PUT /api/papers/{id}/code shallow-clones into the agent
    # workspace). node and ripgrep are only needed when [ai.agent.*] is
    # configured.
    runtimePackages = [
      pkgs.poppler-utils
      pkgs.git
    ]
    ++ lib.optionals agentConfigured [
      pkgs.nodejs
      pkgs.ripgrep
    ];

    environment = {
      # reqwest talks HTTPS to arXiv/Crossref/OpenAI; give it a CA bundle
      # explicitly (the hardened NixOS unit's ProtectSystem=strict sandbox
      # exposes none).
      SSL_CERT_FILE = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";
    }
    // lib.optionalAttrs agentConfigured {
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
