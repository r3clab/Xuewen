{
  flake,
  perSystem,
  pkgs,
  ...
}:
let
  inherit (pkgs) lib;
  toolchain = (flake.lib.rustToolchain pkgs).override { extensions = [ ]; };
  rustPlatform = pkgs.makeRustPlatform {
    cargo = toolchain;
    rustc = toolchain;
  };
  # pdftotext backs text extraction (ingest, chat, citations); git backs the
  # repo-attach shallow clone.
  runtimeDeps = [
    pkgs.poppler-utils
    pkgs.git
  ];
in
rustPlatform.buildRustPackage {
  pname = "xuewen";
  inherit ((lib.importTOML ../../Cargo.toml).workspace.package) version;
  src = lib.fileset.toSource {
    root = ../..;
    fileset = lib.fileset.unions [
      ../../Cargo.toml
      ../../Cargo.lock
      ../../.cargo
      ../../build.rs
      ../../src
      ../../tests
      ../../migrations
      # Shared specs Rust unit tests assert against: the agent-runner wire
      # contract and the frontend search-query cases.
      ../../agent-runner/test/fixtures
      ../../frontend/src/features/library/search/query_cases.json
      # Cargo loads every workspace member's manifest, even unbuilt ones.
      ../../desktop/Cargo.toml
    ];
  };
  # Loading a member also needs a target to exist; a stub keeps the desktop
  # sources out of this build's inputs.
  postPatch = ''
    mkdir -p desktop/src
    touch desktop/src/main.rs
  '';
  cargoLock.lockFile = ../../Cargo.lock;
  # nixpkgs' cargoSetupHook makes aws-lc-sys, zstd-sys and libsqlite3-sys
  # link the system libraries instead of their vendored copies.
  nativeBuildInputs = [
    pkgs.pkg-config
    pkgs.makeBinaryWrapper
  ];
  buildInputs = [
    pkgs.aws-lc
    pkgs.sqlite
    pkgs.zstd
  ];
  # rust-embed reads frontend/dist at compile time; without it the binary
  # serves a placeholder page instead of the UI.
  preBuild = ''
    mkdir -p frontend/dist
    cp -r ${perSystem.self.frontend}/. frontend/dist/
  '';
  # reqwest builds its clients against the system CA store, which the sandbox
  # lacks; the agent tests run their stub runner with node.
  nativeCheckInputs = runtimeDeps ++ [
    pkgs.cacert
    pkgs.nodejs
  ];
  # wiremock and the server tests bind loopback, which the darwin sandbox
  # denies by default.
  __darwinAllowLocalNetworking = true;
  postInstall = ''
    wrapProgram $out/bin/xuewen --prefix PATH : ${lib.makeBinPath runtimeDeps}
  '';
  passthru = { inherit runtimeDeps; };
  meta.mainProgram = "xuewen";
}
