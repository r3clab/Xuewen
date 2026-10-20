# Blueprint calls the outer function with the publishing flake, so `flake`
# here is Xuewen's even when a consumer imports this module.
{ flake, ... }:
{ lib, pkgs, ... }:
let
  inherit (flake.packages.${pkgs.stdenv.hostPlatform.system}) xuewen agent-runner;
in
{
  imports = [ flake.homeModules.xuewen ];
  services.xuewen.package = lib.mkDefault xuewen;
  services.xuewen.agentRunnerPackage = lib.mkDefault agent-runner;
}
