# Stub packages keep this an evaluation test of the generated user unit and
# config: nothing here needs the real builds.
{
  flake,
  inputs,
  pkgs,
  ...
}:
let
  stub = name: pkgs.runCommand name { } "mkdir $out";
  runner = stub "agent-runner-stub";
  home = inputs.home-manager.lib.homeManagerConfiguration {
    inherit pkgs;
    modules = [
      flake.homeModules.xuewen
      {
        home.username = "ada";
        home.homeDirectory = "/home/ada";
        home.stateVersion = "26.11";
        services.xuewen = {
          enable = true;
          package = stub "xuewen-stub";
          agentRunnerPackage = runner;
          dataDir = "/home/ada/My Papers";
          settings.ai.agent.claude_code = { };
        };
      }
    ];
  };
in
pkgs.runCommand "xuewen-home-module"
  {
    unit = "${home.config.home-files}/.config/systemd/user/xuewen.service";
    meta.platforms = pkgs.lib.platforms.linux;
  }
  ''
    grep -Fx "ExecStartPre=${pkgs.coreutils}/bin/mkdir -p '/home/ada/My Papers'" "$unit"
    grep -Fx "WorkingDirectory=/home/ada/My Papers" "$unit"
    grep -F "Environment=PATH=${pkgs.nodejs}/bin:" "$unit"
    config=$(grep -o '/nix/store/[^ ]*-xuewen.toml' "$unit")
    grep -Fx 'data_dir = "/home/ada/My Papers"' "$config"
    grep -Fx 'runner = "${runner}/lib/xuewen/agent-runner/src/runner.mjs"' "$config"
    touch $out
  ''
