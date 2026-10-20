# Running Xuewen with Home Manager

The flake exposes `homeModules.default`, a per-user `systemd --user` service for Xuewen.
It builds the package from this flake, generates `xuewen.toml` from Nix options, puts `pdftotext` (poppler) on the unit's `PATH`, and applies user-service hardening.

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
              # Paper chat / the Ask tab is Agent Ask — see below.
            };

            # Secrets stay OUT of the Nix store (see below).
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

`inbox_dir`, `library_root`, `database_url`, `search.index_dir`, `search.vector_dir` and `preview.cache_dir` default under `services.xuewen.dataDir` (`$XDG_DATA_HOME/xuewen`, i.e. `~/.local/share/xuewen`); override them via `settings` if needed.

## Secrets

The generated `xuewen.toml` lands in the world-readable Nix store, so never put API keys in `settings`.
Instead reference an env var with `api_key_env` and provide it through `environmentFile`:

```text
# a file NOT in the Nix store, readable only by you — e.g. via sops-nix
OPENAI_API_KEY=sk-...
```

## Semantic search (optional)

Keyword search works out of the box.
Semantic search needs only `settings.ai.embedding` (as in the quick start) and its API key in `environmentFile`; vectors are stored in-process under `search.vector_dir`, with no separate server to run.

## Agent Ask (optional)

The reader's Ask tab runs a tool-using agent through the Claude Code / Codex SDKs.
Enabling a backend is all that's needed — the module puts Node on the unit's `PATH` and points `[ai.agent].runner` at the packaged runner in the store, so nothing has to be installed under `dataDir`:

```nix
services.xuewen.settings.ai.agent.claude_code = { };   # and/or .codex = { };
```

The unit runs as you with `$HOME` intact (`ProtectHome` is deliberately unset),
so an existing `claude` / `codex` CLI login works as-is. `ANTHROPIC_API_KEY` /
`OPENAI_API_KEY` via `environmentFile` is the alternative — and the only option
under the NixOS module, whose service user has no such login.

The runner package vendors each SDK's prebuilt CLI binary, so it adds roughly 250 MB to the closure.
Enabling it also relaxes one hardening knob — `MemoryDenyWriteExecute` has to go off for Node's JIT.

## Exposing it

The web UI has **no authentication** and exposes mutating endpoints.
Keep the default loopback bind and front it with an authenticating reverse proxy.
Setting a non-loopback `host` binds publicly and requires the explicit `allowRemote = true` opt-in (an eval-time assertion reminds you); only do that on a trusted network.

## Options

| Option | Default | Purpose |
| --- | --- | --- |
| `enable` | `false` | Enable the service |
| `package` | flake build | Xuewen package to run |
| `agentRunnerPackage` | flake build | Agent Ask runner; defaults `settings.ai.agent.runner` |
| `host` / `port` | `127.0.0.1` / `8080` | Bind address |
| `allowRemote` | `false` | Pass `--allow-remote` (required for a non-loopback `host`) |
| `dataDir` | `$XDG_DATA_HOME/xuewen` | Library / DB / index state |
| `environmentFile` | `null` | systemd `EnvironmentFile` for secrets |
| `settings` | `{}` | `xuewen.toml` as a Nix attrset |

For a system-wide (multi-user, `/var/lib`) install, use the NixOS module
instead — see [`../nixos/README.md`](../nixos/README.md).
