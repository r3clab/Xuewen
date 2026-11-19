{
  flake,
  perSystem,
  pkgs,
  ...
}:
let
  inherit (pkgs) lib;
  inherit (perSystem.nix2container) nix2container;
  inherit (perSystem.self) xuewen;
  # Baked default config: single /data volume, key from the environment.
  # A mounted ConfigMap shadows this file.
  configFile = pkgs.writeTextDir "etc/xuewen/xuewen.toml" ''
    data_dir = "/data"

    [ai]
    api_key_env = "OPENAI_API_KEY"
    model       = "gpt-4o-mini"   # default for chat/summary/daily when enabled

    [ai.embedding]
    model = "text-embedding-3-small"
    dims  = 1536
  '';
  # Docker's named-volume copy-up mirrors the mountpoint's ownership, so an
  # empty /data owned by the container user keeps the volume writable; /tmp
  # is where the daily TL;DR stages PDFs.
  skeleton = pkgs.runCommand "xuewen-image-skeleton" { } "mkdir -p $out/data $out/tmp";
in
nix2container.buildImage {
  name = "ghcr.io/r3clab/xuewen";
  tag = flake.shortRev or flake.dirtyShortRev;
  # Runtime deps that rarely change get their own layer (cheap re-pulls on
  # app updates); the app closure lands in the final layer via Entrypoint.
  layers = [
    (nix2container.buildLayer { deps = xuewen.runtimeDeps ++ [ pkgs.cacert ]; })
  ];
  copyToRoot = [
    configFile
    skeleton
  ];
  perms = [
    {
      path = skeleton;
      regex = "/data";
      mode = "0755";
      uid = 1000;
      gid = 1000;
    }
    {
      path = skeleton;
      regex = "/tmp";
      mode = "1777";
    }
  ];
  config = {
    Entrypoint = [ (lib.getExe xuewen) ];
    Cmd = [
      "serve"
      "--host"
      "0.0.0.0"
      "--port"
      "8080"
      "--allow-remote"
    ];
    Env = [
      "PATH=${xuewen}/bin"
      "SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
      "XUEWEN_CONFIG=/etc/xuewen/xuewen.toml"
      "RUST_LOG=info"
    ];
    User = "1000:1000";
    WorkingDirectory = "/data";
    ExposedPorts = {
      "8080/tcp" = { };
    };
  };
  meta.platforms = lib.platforms.linux;
}
