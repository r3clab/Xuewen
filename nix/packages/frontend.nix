{ pkgs, ... }:
pkgs.buildNpmPackage {
  pname = "xuewen-frontend";
  inherit ((pkgs.lib.importTOML ../../Cargo.toml).workspace.package) version;
  src = ../../frontend;
  npmDepsHash = "sha256-qnYKZTCGWrzUmaIli00VaZ2ssMB+E9sEcJ0aNOVgLIk=";
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
