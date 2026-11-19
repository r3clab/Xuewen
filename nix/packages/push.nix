{ perSystem, pkgs, ... }:
let
  inherit (perSystem.self) image;
in
pkgs.writeShellApplication {
  name = "xuewen-push";
  runtimeInputs = [ pkgs.kustomize ];
  # Prereq: skopeo login ghcr.io (documented in deploy/k8s/README.md)
  text = ''
    ${pkgs.lib.getExe image.copyToRegistry}
    if [ "$#" -gt 0 ]; then
      cd "$1"
      kustomize edit set image ${image.imageName}:${image.imageTag}
    fi
  '';
  meta.description = "push the xuewen image to ghcr and, given a kustomization directory, pin it there";
  meta.platforms = pkgs.lib.platforms.linux;
}
