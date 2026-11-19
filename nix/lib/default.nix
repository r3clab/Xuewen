{ inputs, ... }:
{
  rustToolchain =
    pkgs:
    (inputs.rust-overlay.lib.mkRustBin { } pkgs).fromRustupToolchainFile ../../rust-toolchain.toml;
}
