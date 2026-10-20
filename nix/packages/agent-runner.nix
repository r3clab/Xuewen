{ pkgs, ... }:
let
  inherit (pkgs) lib stdenv;
in
pkgs.buildNpmPackage {
  pname = "xuewen-agent-runner";
  inherit ((lib.importTOML ../../Cargo.toml).workspace.package) version;
  src = ../../agent-runner;
  npmDepsHash = "sha256-+ERsdA8rx5Nf9IeYd98huAWPixBsOZWEBC4dZttfHnI=";
  # No build script — this is plain ESM shipped as-is with its deps.
  dontNpmBuild = true;
  doCheck = true;
  checkPhase = ''
    runHook preCheck
    npm test
    runHook postCheck
  '';
  # Unlike the frontend (baked into the binary at compile time), the
  # runner stays a Node program and needs its node_modules at runtime,
  # so it is its own store path that the deploy modules point
  # `[ai.agent].runner` at.
  #
  # Both SDKs pull their CLI in as a prebuilt native binary (an npm
  # optional dep picked by os/cpu), which makes this a binary
  # distribution as much as a JS one. Codex's are static-pie musl and
  # run anywhere; Claude Code's is glibc-dynamic and needs its ELF
  # interpreter rewritten to run on a NixOS host that lacks nix-ld.
  # `dontStrip` for the same reason nixpkgs' own claude-code sets it:
  # the binary is a Bun single-file executable, and stripping it leaves
  # the Bun runtime to execute instead of the program embedded in it.
  nativeBuildInputs = lib.optional stdenv.hostPlatform.isElf pkgs.autoPatchelfHook;
  # Only for autoPatchelfHook to resolve against: libstdc++ for Claude
  # Code's binary, ncurses (libtinfo) for the zsh Codex vendors to run
  # shell commands in. Darwin's binaries need no patching.
  buildInputs = lib.optionals stdenv.hostPlatform.isElf [
    stdenv.cc.cc.lib
    pkgs.ncurses
  ];
  dontStrip = true;
  installPhase = ''
    runHook preInstall
    mkdir -p $out/lib/xuewen/agent-runner
    cp -r src node_modules package.json $out/lib/xuewen/agent-runner/
    runHook postInstall
  '';
}
