# Xuewen's NixOS and Home Manager modules

Both modules share one option set and one generated `xuewen.toml`; [`nixos/`](nixos/README.md) runs a hardened system service, [`home/`](home/README.md) a per-user `systemd --user` service.
This page covers what they have in common.

## Data and settings

`services.xuewen.settings` is `xuewen.toml` as a Nix attrset.
The module sets `data_dir` to `services.xuewen.dataDir`, and the inbox, library, `library.db`, search index, vector store and preview cache all default under it; set the individual keys in `settings` to move any of them.

The package carries its own runtime tools: `pdftotext` (poppler) and `git` are on its wrapper's `PATH`, so the unit needs nothing extra for ingest or attaching code repositories.

## Secrets

The generated `xuewen.toml` lands in the world-readable Nix store, so never put API keys in `settings`.
Instead reference an env var with `api_key_env` and provide it through `environmentFile`, a file outside the store that only the service can read (e.g. via sops-nix or agenix):

```text
OPENAI_API_KEY=sk-...
```

## Semantic search (optional)

Keyword search works out of the box.
Semantic search needs only `settings.ai.embedding` and its API key in `environmentFile`; vectors are stored in-process under the data directory, with no separate server to run.

## Agent Ask (optional)

The reader's Ask tab runs a tool-using agent through the Claude Code / Codex SDKs.
Enabling a backend is all that's needed — the module puts Node and ripgrep on the unit's `PATH` and points `[ai.agent].runner` at the packaged runner in the store, so nothing has to be installed under `dataDir`:

```nix
services.xuewen.settings.ai.agent.claude_code = { };   # and/or .codex = { };
```

The runner package vendors each SDK's prebuilt CLI binary, so it adds roughly 250 MB to the closure.
Enabling it also relaxes one hardening knob — `MemoryDenyWriteExecute` has to go off for Node's JIT — which is why the section is opt-in rather than on by default.

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
| `dataDir` | per module | Library / DB / index state |
| `environmentFile` | `null` | systemd `EnvironmentFile` for secrets |
| `settings` | `{}` | `xuewen.toml` as a Nix attrset |

`nixosModules.default` / `homeModules.default` fill in `package` and `agentRunnerPackage` from this flake; the bare `nixosModules.xuewen` / `homeModules.xuewen` leave them to you.
