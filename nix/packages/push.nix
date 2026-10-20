{ flake, pkgs, ... }:
let
  tag = flake.shortRev or "dev";
in
pkgs.writeShellApplication {
  name = "xuewen-push";
  # Prereq: skopeo login ghcr.io (documented in deploy/k8s/README.md)
  text = ''
    nix run .#image.copyTo -- docker://ghcr.io/r3clab/xuewen:${tag}
    if [ "${tag}" = "dev" ]; then
      echo "dirty tree — pushed :dev only, not :latest"
    else
      nix run .#image.copyTo -- docker://ghcr.io/r3clab/xuewen:latest
    fi
  '';
  meta.description = "push the xuewen image to ghcr";
  meta.platforms = [ "x86_64-linux" ];
}
