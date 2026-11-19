# Running Xuewen with Home Manager

The flake exposes `homeModules.default`, a per-user `systemd --user` service for Xuewen.
Settings, secrets, Agent Ask and the shared options are described in [`../README.md`](../README.md); this page covers what is Home Manager-specific.

**Linux only.**
The module relies on systemd user units, which Home Manager does not provide on macOS.
On a Mac, use the native desktop app (`cargo run -p xuewen-desktop`, or the bundled `.dmg`) instead.

## Quick start (flakes)

```nix
# flake.nix
{
  inputs = {
    home-manager.url = "github:nix-community/home-manager";
    xuewen.url = "github:r3clab/xuewen";
  };

  outputs = { nixpkgs, home-manager, xuewen, ... }: {
    homeConfigurations."me@myhost" = home-manager.lib.homeManagerConfiguration {
      pkgs = nixpkgs.legacyPackages.x86_64-linux;
      modules = [
        xuewen.homeModules.default
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
            environmentFile = "/run/user/1000/secrets/xuewen.env";
          };
        })
      ];
    };
  };
}
```

The service auto-starts on login (`WantedBy = default.target`).
Manage it with `systemctl --user {start,stop,status} xuewen` and read logs with `journalctl --user -u xuewen`.

State lives in `dataDir`, `$XDG_DATA_HOME/xuewen` (i.e. `~/.local/share/xuewen`) by default, created before the service starts.

## Agent Ask authentication

The unit runs as you with `$HOME` intact (`ProtectHome` is deliberately unset), so an existing `claude` / `codex` CLI login works as-is.
`ANTHROPIC_API_KEY` / `OPENAI_API_KEY` via `environmentFile` is the alternative.

For a system-wide (multi-user, `/var/lib`) install, use the NixOS module instead — see [`../nixos/README.md`](../nixos/README.md).
