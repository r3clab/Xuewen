# Home Manager module for Xuewen. Exposed from the flake as `homeModules.default`
# (which also fills in `services.xuewen.package` from the flake's build) and
# `homeModules.xuewen` (this file; set `services.xuewen.package` yourself).
#
# Runs Xuewen as a per-user `systemd --user` service. Linux only: it relies on
# systemd user units, which Home Manager does not provide on Darwin. macOS
# users have the native desktop app (`xuewen-desktop`) instead.
#
# The option set and everything derived from it are shared with the NixOS
# module via ../shared.nix; this file is the user-unit flavor: state under $HOME
# and the reduced hardening set a user unit supports.
{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.services.xuewen;
  shared = import ../shared.nix {
    inherit lib pkgs;
    modules = "homeModules";
    environmentFileExample = "/run/user/1000/secrets/xuewen.env";
  };
  xw = shared.mkXuewen cfg;
in
{
  options.services.xuewen = shared.options // {
    dataDir = lib.mkOption {
      type = lib.types.str;
      default = "${config.xdg.dataHome}/xuewen";
      defaultText = lib.literalExpression ''"''${config.xdg.dataHome}/xuewen"'';
      description = ''
        State directory holding the inbox, library, SQLite database and search
        index. Created automatically before the service starts.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    inherit (xw) assertions warnings;

    systemd.user.services.xuewen = {
      Unit = {
        Description = "Xuewen reference manager";
        After = [ "network-online.target" ];
        Wants = [ "network-online.target" ];
      };
      Install.WantedBy = [ "default.target" ];
      Service = {
        # The data dir lives under $HOME, so create it before first launch
        # rather than through system tmpfiles.
        ExecStartPre = "${pkgs.coreutils}/bin/mkdir -p ${cfg.dataDir}";
        ExecStart = xw.execStart;
        WorkingDirectory = cfg.dataDir;
        EnvironmentFile = lib.mkIf (cfg.environmentFile != null) cfg.environmentFile;
        # A user unit inherits no useful PATH, so pdftotext, git, and (when
        # Agent Ask is on) node and ripgrep are resolved from an explicit one.
        Environment = lib.mapAttrsToList (k: v: "${k}=${v}") (
          {
            RUST_LOG = "info";
            PATH = lib.makeBinPath xw.runtimePackages;
          }
          // xw.environment
        );
        Restart = "on-failure";
        RestartSec = 5;

        # Hardening appropriate for a user unit whose state lives under $HOME.
        # ProtectHome is deliberately NOT set — it would hide the data dir from
        # the service itself.
        NoNewPrivileges = true;
        PrivateTmp = true;
        ProtectKernelTunables = true;
        ProtectKernelModules = true;
        ProtectControlGroups = true;
        RestrictRealtime = true;
        LockPersonality = true;
        MemoryDenyWriteExecute = !xw.agentConfigured;
        RestrictAddressFamilies = [
          "AF_INET"
          "AF_INET6"
          "AF_UNIX"
        ];
      };
    };
  };
}
