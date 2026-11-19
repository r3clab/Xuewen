# NixOS module for Xuewen. Exposed from the flake as `nixosModules.default`
# (which also fills in `services.xuewen.package` from the flake's build) and
# `nixosModules.xuewen` (this file; set `services.xuewen.package` yourself).
#
# The option set and everything derived from it are shared with the Home
# Manager module via ../shared.nix; this file is the system-unit flavor: service
# user/group, tmpfiles, firewall, and the full hardening set.
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
    modules = "nixosModules";
    environmentFileExample = "/run/secrets/xuewen.env";
    dataDirDefault.default = "/var/lib/xuewen";
  };
  xw = shared.mkXuewen cfg;
in
{
  options.services.xuewen = shared.options // {
    openFirewall = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Open {option}`services.xuewen.port` in the firewall.";
    };

    user = lib.mkOption {
      type = lib.types.str;
      default = "xuewen";
      description = "User the service runs as (created when left at the default).";
    };

    group = lib.mkOption {
      type = lib.types.str;
      default = "xuewen";
      description = "Group the service runs as (created when left at the default).";
    };
  };

  config = lib.mkIf cfg.enable {
    inherit (xw) assertions warnings;

    users.users = lib.mkIf (cfg.user == "xuewen") {
      xuewen = {
        isSystemUser = true;
        inherit (cfg) group;
        home = cfg.dataDir;
        description = "Xuewen service user";
      };
    };
    users.groups = lib.mkIf (cfg.group == "xuewen") { xuewen = { }; };

    networking.firewall.allowedTCPPorts = lib.mkIf cfg.openFirewall [ cfg.port ];

    systemd.tmpfiles.rules = [
      "d ${cfg.dataDir} 0750 ${cfg.user} ${cfg.group} - -"
    ];

    systemd.services.xuewen = {
      description = "Xuewen reference manager";
      wantedBy = [ "multi-user.target" ];
      after = [ "network-online.target" ];
      wants = [ "network-online.target" ];
      path = xw.runtimePackages;
      environment = {
        RUST_LOG = lib.mkDefault "info";
      }
      // xw.environment;
      serviceConfig = {
        ExecStart = xw.execStart;
        User = cfg.user;
        Group = cfg.group;
        WorkingDirectory = cfg.dataDir;
        EnvironmentFile = lib.mkIf (cfg.environmentFile != null) [ cfg.environmentFile ];
        Restart = "on-failure";
        RestartSec = 5;

        # Hardening: the service only ever writes under dataDir.
        ReadWritePaths = [ cfg.dataDir ];
        NoNewPrivileges = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        PrivateTmp = true;
        PrivateDevices = true;
        ProtectControlGroups = true;
        ProtectKernelModules = true;
        ProtectKernelTunables = true;
        ProtectKernelLogs = true;
        ProtectClock = true;
        ProtectHostname = true;
        ProtectProc = "invisible";
        ProcSubset = "pid";
        PrivateUsers = true;
        CapabilityBoundingSet = "";
        RestrictNamespaces = true;
        RestrictRealtime = true;
        RemoveIPC = true;
        LockPersonality = true;
        MemoryDenyWriteExecute = !xw.agentConfigured;
        UMask = "0077";
        SystemCallArchitectures = "native";
        # Codex sandboxes the commands it runs with Landlock and seccomp, the
        # calls in @sandbox.
        SystemCallFilter = [
          "@system-service"
        ]
        ++ lib.optional xw.agentConfigured "@sandbox"
        ++ [ "~@privileged" ];
        SystemCallErrorNumber = "EPERM";
        RestrictAddressFamilies = [
          "AF_INET"
          "AF_INET6"
          "AF_UNIX"
        ];
      };
    };
  };
}
