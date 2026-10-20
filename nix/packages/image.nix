{
  flake,
  perSystem,
  pkgs,
  ...
}:
let
  inherit (perSystem.nix2container) nix2container;
  inherit (perSystem.self) xuewen;
  # Baked default config: single /data volume, in-cluster Qdrant,
  # key from the environment. A mounted ConfigMap shadows this file.
  configFile = pkgs.writeTextFile {
    name = "xuewen-default-config";
    destination = "/etc/xuewen/xuewen.toml";
    text = ''
      inbox_dir     = "/data/inbox"
      library_root  = "/data/library"
      database_url  = "sqlite:/data/library.db"

      [search]
      index_dir         = "/data/search-index"
      qdrant_url        = "http://xuewen-qdrant:6333"
      qdrant_collection = "xuewen"

      [ai]
      api_key_env = "OPENAI_API_KEY"
      model       = "gpt-4o-mini"   # default for chat/summary/daily when enabled

      [ai.embedding]
      model = "text-embedding-3-small"
      dims  = 1536
    '';
  };
  # Empty /data owned 1000:1000 baked into the image so Docker's
  # named-volume copy-up (which mirrors the mountpoint's ownership)
  # doesn't leave the volume root-owned and unwritable by the
  # container's non-root user.
  dataDir = pkgs.runCommand "xuewen-data-dir" { } "mkdir -p $out/data";
  image = nix2container.buildImage {
    name = "ghcr.io/r3clab/xuewen";
    tag = flake.shortRev or "dev";
    # Layer 1: runtime deps that rarely change (cheap re-pulls on
    # app updates). The app closure lands in the final layer via
    # the Entrypoint reference.
    layers = [
      (nix2container.buildLayer {
        deps = [
          pkgs.poppler-utils
          pkgs.cacert
        ];
      })
    ];
    copyToRoot = [
      configFile
      dataDir
    ];
    perms = [
      {
        path = dataDir;
        regex = "/data";
        mode = "0755";
        uid = 1000;
        gid = 1000;
      }
    ];
    config = {
      Entrypoint = [
        "${xuewen}/bin/xuewen"
        "--config"
        "/etc/xuewen/xuewen.toml"
        "serve"
        "--host"
        "0.0.0.0"
        "--port"
        "8080"
        "--allow-remote"
      ];
      Env = [
        "PATH=${xuewen}/bin:${pkgs.poppler-utils}/bin"
        "SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
        "RUST_LOG=info"
      ];
      User = "1000:1000";
      WorkingDir = "/data";
      ExposedPorts = {
        "8080/tcp" = { };
      };
    };
  };
in
pkgs.lib.addMetaAttrs { platforms = [ "x86_64-linux" ]; } image
