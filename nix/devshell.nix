{
  inputs,
  perSystem,
  pkgs,
  ...
}:
let
  inherit (pkgs) lib;
  inherit (pkgs.stdenv.hostPlatform) isDarwin;
  toolchain = perSystem.fenix.stable.withComponents [
    "cargo"
    "clippy"
    "rust-analyzer"
    "rust-src"
    "rustc"
    "rustfmt"
  ];
in
perSystem.devshell.mkShell {
  imports = [
    "${inputs.devshell}/extra/git/hooks.nix"
    "${inputs.devshell}/extra/language/c.nix"
  ];

  packages = [
    toolchain
    pkgs.poppler-utils
    pkgs.sqlite
    pkgs.nodejs
    pkgs.pkg-config
  ]
  ++ lib.optionals isDarwin [ pkgs.libiconv ];

  # Rust's darwin std links -liconv; devshell's language.c.libraries would set
  # LIBRARY_PATH to "-L<dir>", which the linker does not read as a path.
  env = lib.optionals isDarwin [
    {
      name = "LIBRARY_PATH";
      eval = "$DEVSHELL_DIR/lib";
    }
  ];

  devshell.motd = "";

  git.hooks.enable = true;
  # The hook body runs more than one command, so a treefmt failure would
  # otherwise be masked by clippy succeeding after it.
  git.hooks.pre-commit.text = ''
    set -e
    ${perSystem.self.formatter}/bin/treefmt --fail-on-change
    ${toolchain}/bin/cargo clippy --all-targets -- -D warnings
  '';
}
