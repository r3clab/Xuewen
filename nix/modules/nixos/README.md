# Running Xuewen on NixOS

The flake exposes `nixosModules.default`, a hardened systemd service for Xuewen.
Settings, secrets, Agent Ask and the shared options are described in [`../README.md`](../README.md); this page covers what is NixOS-specific.

## Quick start (flakes)

```nix
# flake.nix
{
  inputs.xuewen.url = "github:r3clab/xuewen";

  outputs = { nixpkgs, xuewen, ... }: {
    nixosConfigurations.myhost = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        xuewen.nixosModules.default
        ({ ... }: {
          services.xuewen = {
            enable = true;
            # host = "127.0.0.1";  # default; non-loopback also needs allowRemote = true
            # port = 8080;

            settings.ai = {
              api_key_env = "OPENAI_API_KEY";
              embedding = { model = "text-embedding-3-small"; dims = 1536; };
              summary = { };  # per-paper LLM summaries
              # Paper chat / the Ask tab is Agent Ask — see ../README.md.
            };

            # Secrets stay OUT of the Nix store.
            environmentFile = "/run/secrets/xuewen.env";  # 0600, root-owned
          };
        })
      ];
    };
  };
}
```

State lives in `dataDir`, `/var/lib/xuewen` by default, created with the service user's ownership.

## Agent Ask authentication

The service user has no `claude`/`codex` CLI login, so authentication comes from `ANTHROPIC_API_KEY` / `OPENAI_API_KEY` in the `environmentFile`.

## Reverse proxy

```nix
services.nginx.virtualHosts."papers.example.com" = {
  enableACME = true; forceSSL = true;
  locations."/".proxyPass = "http://127.0.0.1:8080";
  # add basic auth / oauth2-proxy here
};
```

## NixOS-only options

| Option | Default | Purpose |
| --- | --- | --- |
| `dataDir` | `/var/lib/xuewen` | Library / DB / index state |
| `user` / `group` | `xuewen` | Service identity (auto-created) |
| `openFirewall` | `false` | Open the port |

## Test

A VM test boots a machine, enables the module, and hits the API (Linux + KVM):

```text
nix build .#checks.x86_64-linux.nixos-module -L
```
