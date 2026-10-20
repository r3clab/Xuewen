{
  flake,
  perSystem,
  pkgs,
  ...
}:
let
  inherit (pkgs) lib;
in
pkgs.rustPlatform.buildRustPackage {
  pname = "xuewen";
  inherit ((lib.importTOML ../../Cargo.toml).workspace.package) version;
  # Exclude everything the backend build never reads — the frontend
  # sources (dist comes from the `frontend` package), agent-runner
  # (its own package, consumed only at runtime; the agent tests use
  # tests/fixtures/stub_runner.mjs), Nix and deploy files, and CI —
  # so editing them never rebuilds the backend. One carve-out:
  # agent-runner/test/fixtures holds the wire-contract fixture that a
  # Rust unit test asserts against (via CARGO_MANIFEST_DIR), so that
  # subtree must stay visible to this build.
  src = lib.cleanSourceWith {
    src = flake;
    filter =
      path: _type:
      let
        rel = lib.removePrefix (toString flake + "/") (toString path);
        under = dir: rel == dir || lib.hasPrefix (dir + "/") rel;
        fixturePath =
          builtins.elem rel [
            "agent-runner"
            "agent-runner/test"
            "agent-runner/test/fixtures"
          ]
          || lib.hasPrefix "agent-runner/test/fixtures/" rel;
      in
      !(
        under "frontend"
        || under "deploy"
        || under "nix"
        || (under "agent-runner" && !fixturePath)
        || under ".github"
        || builtins.elem rel [
          "flake.nix"
          "flake.lock"
          ".gitignore"
          ".envrc"
          "README.md"
        ]
      );
  };
  cargoLock.lockFile = ../../Cargo.lock;
  # rust-embed reads frontend/dist at compile time; build.rs would
  # write a placeholder if this were missing.
  preBuild = ''
    mkdir -p frontend/dist
    cp -r ${perSystem.self.frontend}/. frontend/dist/
  '';
  # The full test suite runs in the sandbox: wiremock binds loopback
  # (available in Nix builds), pdftotext comes from poppler, the
  # agent::code clone tests shell out to git, and the tests/it/agent.rs
  # suite runs its stub runner with node.
  nativeCheckInputs = [
    pkgs.poppler-utils
    pkgs.git
    pkgs.nodejs
  ];
}
