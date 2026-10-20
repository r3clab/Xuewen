{ pkgs, ... }:
pkgs.writeShellApplication {
  name = "xuewen-load";
  text = ''
    nix run .#image.copyToDockerDaemon
  '';
  meta.description = "load the xuewen image into the local docker daemon";
  meta.platforms = [ "x86_64-linux" ];
}
