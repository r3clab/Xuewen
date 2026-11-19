{ pkgs, ... }:
pkgs.buildNpmPackage {
  pname = "xuewen-frontend";
  inherit ((pkgs.lib.importTOML ../../Cargo.toml).workspace.package) version;
  src = ../../frontend;
  npmDeps = pkgs.importNpmLock { npmRoot = ../../frontend; };
  inherit (pkgs.importNpmLock) npmConfigHook;
  # `npm run build` is the default buildPhase; run vitest before install.
  doCheck = true;
  checkPhase = ''
    runHook preCheck
    npm test
    runHook postCheck
  '';
  installPhase = ''
    runHook preInstall
    cp -r dist $out
    runHook postInstall
  '';
}
