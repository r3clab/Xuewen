{ perSystem, pkgs, ... }:
pkgs.lib.addMetaAttrs {
  description = "load the xuewen image into the local docker daemon";
  platforms = pkgs.lib.platforms.linux;
} perSystem.self.image.copyToDockerDaemon
