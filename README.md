<div align="center">

<img src="assets/logo.svg" width="112" height="112" alt="Xuewen — the 學 seal" />

# 學問 · Xuewen

</div>

A self-hosted reference manager for research papers (currently for computer science only).

## The name

**學問** (xuéwèn, simplified 学问) — "learning / scholarship."
The two characters double as the interface marks: **學** (xué, *learning*) is the cinnabar seal wordmark, and **問** (wèn, *to ask*) is the amber chat launcher — the assistant that answers questions about a paper.

## Features

- **Automatic metadata** — resolved from arXiv, Crossref, DBLP and [OpenReview](https://openreview.net) (which carries ICLR and recent ICML/NeurIPS proceedings months before DBLP indexes them, and that Crossref never carries at all), with optional [GROBID](https://github.com/kermitt2/grobid) header extraction as a fallback.
  Papers are filed under a deterministic cite key (e.g. `vaswani2017attention`).
- **Manual identify** — when auto-resolution is unsure, match a paper to a DOI,
  arXiv id, or a title search from the UI or CLI.
- **Search** — BM25 keyword search (always on) plus optional semantic search over title/abstract/body chunks, fused into one ranked list.
  Queries accept GitHub-style qualifiers in the web UI and the CLI alike:

  | Qualifier | Meaning |
  |---|---|
  | `tag:nlp`, `tag:"deep learning"` | filter by tag (nested prefix: also matches `nlp/eval`) |
  | `project:thesis`, `project:"my thesis"` | filter by project name |
  | `is:starred` | starred papers only |
  | `status:resolved` / `status:needs-review` | resolution status |
  | `in:title` `in:authors` `in:abstract` `in:body` `in:notes` | restrict search fields (union of `in:` tokens; `notes` = your annotation notes) |
  | `author:smith`, `author:"ada lovelace"` | author-scoped search term (repeatable, ANDed) |
  | `"exact phrase"` | phrase query |

  Anything else — including malformed qualifiers — is plain free text; parsing never errors.
- **Annotations** — highlight, underline, strike through or squiggle a passage, drop a sticky note, and attach a note to any mark, with undo/redo and a page-ordered list in the reader's sidebar.
  Marks live in SQLite beside the paper, never in the PDF: **the library's file stays byte-identical**, so its content hash (which drives dedupe and filing) keeps telling the truth.
  Note text is searchable (`in:notes`), and "Export annotated PDF" writes a separate copy with the marks burned in for sharing.
- **Agent Ask** — optional tool-using agent (Claude Code / Codex SDKs) in the reader's Ask thread or `xuewen ask`, grounded in the paper's extracted text and, when attached, its code repository, inside a read-only sandbox.
- **Daily arXiv recommendations** — a ranked, LLM-summarized feed of new papers
  scored against your library's interests (optional).
- **Helix-style keymap** — `Space` is a leader key, as in [Helix](https://helix-editor.com): `Space f` opens a file picker that fuzzy-matches a paper's name and title as you type and previews its rendered pages beside the list. `?` lists every binding.
- **Citation export** — BibTeX / BibLaTeX for a single paper, a project, or the
  whole library.
- **Organization** — projects (named groups of papers), free-form tags
  (`/`-nested, e.g. `nlp/eval`), and a star flag; filter, rename, and delete
  from the UI's pill bar or the CLI.
- **Paywall helper** — optional institutional (EZproxy) support for fetching
  PDFs you have access to.

## Architecture

| Layer | Tech |
| --- | --- |
| Backend | Rust — [Axum](https://github.com/tokio-rs/axum), [SQLx](https://github.com/launchbadge/sqlx)/SQLite, [Tantivy](https://github.com/quickwit-oss/tantivy) (keyword), [Qdrant Edge](https://crates.io/crates/qdrant-edge) (vectors, embedded in-process — no server), reqwest |
| Frontend | [Svelte 5](https://svelte.dev/) + Vite + Tailwind, embedded via `rust-embed` |
| Text extraction | `pdftotext` (poppler-utils) |

## Quick start

With [Nix](https://nixos.org/) (recommended — pulls in cargo, node, poppler,
sqlite):

```sh
nix develop            # or: direnv allow

# Build the frontend (rust-embed bundles frontend/dist into the binary).
npm --prefix frontend install
npm --prefix frontend run build

# Write xuewen.toml (see Configuration), then run the web UI.
cargo run -- serve                   # http://127.0.0.1:8080
```

Without Nix, install Rust, Node.js and poppler-utils yourself, then run the same
commands.

### Frontend hot-reload (development)

Run the backend and the Vite dev server side by side — Vite proxies `/api` and
`/papers` to the backend:

```sh
cargo run -- serve                   # backend on :8080
npm --prefix frontend run dev        # UI on :5173, hot-reloads
```

### Prebuilt binary

```sh
nix build            # ./result/bin/xuewen  (frontend already embedded)
```

## Desktop app (macOS)

The same server and UI, packaged as a native window — a [Tauri](https://tauri.app/) app boots the Axum backend in-process and opens a webview at it, no terminal required.
Apple Silicon only; download the `.dmg` from
[GitHub Releases](../../releases).

**First launch:** the dmg is unsigned, so Gatekeeper blocks a plain double-click.
Either right-click the app → **Open** (once), or:

```sh
xattr -d com.apple.quarantine /Applications/Xuewen.app
```

**Where data lives:** `~/Library/Application Support/Xuewen/` — `xuewen.toml`, an `env` file for API keys, `xuewen.db`, `library/`, `inbox/`, `search-index/`, `search-vectors/`, and `preview-cache/`.
Logs go to `~/Library/Logs/Xuewen/`.
First launch creates all of this; existing files are never overwritten.

**Enabling AI features:** edit `xuewen.toml` (same `[ai.*]` sections as the
server — see [Configuration](#configuration)) and put keys in the `env` file, one
`KEY=value` per line (e.g. `OPENAI_API_KEY=sk-...`), then relaunch.

**What's bundled:** `pdftotext` and Node.js ship inside the app, so PDF ingest and Agent Ask work out of the box.
Semantic search is built in as well: vectors are stored in `search-vectors/`, so it needs only `[ai.embedding]` and an API key, with nothing else to install.

**Building locally:**

```sh
npm --prefix frontend install && npm --prefix frontend run build   # embedded UI (else the dmg ships a placeholder page)
npm --prefix agent-runner install --omit=dev                       # bundled as a resource, needed for Agent Ask
bash desktop/scripts/prepare-sidecars.sh   # needs Homebrew poppler + dylibbundler
npm --prefix desktop install               # pinned Tauri CLI (does the bundling/signing)
cd desktop && npx tauri build
```

## Configuration

`xuewen.toml` is read from the working directory (`--config` to override).
Only three keys are required:

```toml
inbox_dir    = "./inbox"
library_root = "./library"
database_url = "sqlite:./library.db"
```

Optional sections enable the richer features:

- `[ai.embedding]` — semantic search; needs only an API key.
  Vectors are stored in-process (no separate server) under `[search].vector_dir` (default `./search-vectors`, `~` expanded like `index_dir`); neither directory may contain the other.
  After changing `[ai.embedding].dims`, recreate the store with `xuewen index rebuild --vectors-only`.
- `[ai.agent.*]` — the reader's Ask thread / paper chat (a tool-using agent; see
  [Agent Ask setup](#agent-ask-setup)).
- `[daily]` — daily arXiv recommendations.
- `[proxy]` — institutional paywall access.
- `[preview]` — where the file picker's rendered page images are cached (defaults to `./preview-cache`).

API keys are read from environment variables via `api_key_env` (e.g. `OPENAI_API_KEY`), so they never need to live in the config file.
`src/config.rs` defines the full set of options.

**Upgrading from an external Qdrant server:** `qdrant_url` and `qdrant_collection` are now ignored.
The embedded store starts empty, so the first start re-embeds the whole library through the embedding API, which costs API calls and takes a while.
Once semantic search works again, the old Qdrant server and its data can be removed.

### Agent Ask setup

The reader's Ask thread can run a tool-using agent — via the [Claude
Code](https://github.com/anthropics/claude-code) or [Codex](https://github.com/openai/codex)
SDKs — that reads a paper's extracted text and, once attached, its GitHub
repository, inside a read-only per-paper workspace.

1. **Node ≥ 20** on the machine running `xuewen serve` or `xuewen ask`.
2. Install the runner's own dependencies once: `npm --prefix agent-runner install` (separate from the frontend's `npm --prefix frontend install`).
   The NixOS and Home Manager modules do this for you — see [Deployment](#deployment).
   The runner is looked up at `agent-runner/src/runner.mjs` relative to the working directory; run `xuewen ask` from the checkout or set `[ai.agent].runner` to its absolute path.
3. Enable one or both backends in `xuewen.toml`:

   ```toml
   [ai.agent]
   [ai.agent.claude_code]
   [ai.agent.codex]
   ```

4. Authenticate whichever backend(s) you enabled — either your existing `claude` / `codex` CLI login, or `ANTHROPIC_API_KEY` / `OPENAI_API_KEY` in the environment `xuewen serve` or `xuewen ask` runs in.
   Neither key is ever written to `xuewen.toml`.

Attaching a repository (Details → Code, `PUT /api/papers/{id}/code`, or `xuewen code set`) does a **local, read-only shallow clone** into `<library_root>/agent/<paper_id>/repo/` for the agent to read from — it is never pushed, modified, or redistributed; it exists only on the machine that holds the library, for as long as it stays attached.

**Sandbox boundary, honestly.**
"Read-only" describes what the agent can't do — it can't write files, run shell commands (Claude Code backend), or reach the network — not what it can read.
The Claude Code backend's tool pre-approval (`allowedTools: ['Read', 'Grep', 'Glob']`) is not scoped to the paper's workspace, and Codex's read-only sandbox likewise blocks writes and network access but not reads; either backend can read any file on disk that the user running `xuewen serve` or `xuewen ask` can read, not just `<library_root>/agent/<paper_id>/`.
Don't attach repositories you don't trust, and treat Agent Ask like the rest of the web UI: single-user and loopback-only unless you understand the exposure of `--allow-remote`.

## CLI

The same binary drives everything from the terminal:

| Command | Purpose |
| --- | --- |
| `serve` | Run the web UI (loopback by default; `--allow-remote` to bind publicly) |
| `ingest <pdf>` | Ingest a single local PDF |
| `import <input>` | Import by arXiv id, DOI, or URL |
| `watch` | Auto-ingest new PDFs dropped in the inbox |
| `identify <id>` | Manually match a paper (`--doi` / `--arxiv` / `--title`) |
| `refresh` | Re-resolve failed records and re-file to cite-key paths |
| `list` | List papers (`--project` / `--tag` / `--starred` / `--query` / `--status`, `--sort`) |
| `show <id>` | One paper's metadata, abstract, AI summary, tags, projects, and code repo |
| `stats` | Library counts |
| `search <query>` | Search from the terminal (`--keyword-only` / `--semantic-only`); accepts the same `tag:`/`project:`/`is:starred`/`status:`/`in:`/`author:` qualifiers as the web UI |
| `ask <id> [question]` | Ask the paper agent (`--model`); with no question, read one per line from stdin until Ctrl-D |
| `chat` | Saved Ask conversations (`history` / `clear` / `models`) |
| `citations <id>` | The reference list the web reader parsed for a paper |
| `translate [text]` | Translate text, or stdin, with the configured provider (`--to` / `--provider`) |
| `daily` | Daily arXiv recommendations (`show` / `run [--force]`) |
| `summarize` | Generate LLM summaries for library papers |
| `export` | Emit BibTeX / BibLaTeX (single, filtered, or whole library) |
| `project` | Manage projects (`list` / `new` / `rename` / `rm` / `add` / `remove` / `show`) |
| `tag` | Manage tags (`list` / `add` / `remove` / `rename` / `rm` / `show`) |
| `star` / `unstar` | Star or un-star a paper |
| `name <id> [name]` | Show, set, or `--clear` the name a paper is known by |
| `annotation` | A paper's reader annotations (`list` / `add` / `rm` / `clear`); `add` attaches a page note |
| `index` | Inspect or rebuild the search indexes |
| `code` | Attach, inspect, or detach a paper's code repo for Agent Ask (`set` / `status` / `rm`) |
| `delete` / `restore` / `purge` | Trash lifecycle |
| `proxy-cookie` | Manage the stored EZproxy session cookie |

Only one process can hold the vector store at a time.
While `xuewen serve` (or the desktop app) is running, `xuewen search`, `xuewen index status`, and `xuewen daily run` forward to it over HTTP, so search keeps returning semantic results and the day's run happens in the server.
`xuewen index rebuild` refuses to run until that server is stopped, and `xuewen watch` started beside it warns and skips indexing.

Commands that add, change, or remove papers or notes (`ingest`, `import`, `identify`, `refresh`, `delete`, `restore`, `purge`, and annotation edits) bring the full-text index up to date before they return, and `xuewen search` first indexes anything new.
When another process holds the index, they leave it to that process and say so.

Run `xuewen --help` (or `xuewen <command> --help`) for the full flags.

## Deployment

- **NixOS module** — `nixosModules.default` provides a hardened systemd service.
  See [`nix/modules/nixos/README.md`](nix/modules/nixos/README.md).
- **Home Manager module** — `homeModules.default` runs a per-user `systemd --user` service (Linux only).
  See [`nix/modules/home/README.md`](nix/modules/home/README.md).
- **Container image** — a minimal OCI image is built with `nix2container`; a
  Kubernetes example lives in [`deploy/k8s/`](deploy/k8s/).

The NixOS and Home Manager modules handle Agent Ask themselves: put an `[ai.agent.*]` section in `settings` and the unit gets Node ≥ 20 on its `PATH`, with `[ai.agent].runner` defaulted to a packaged `agent-runner` (npm dependencies and the CLI binaries those ship, all built into the store).
Nothing needs installing under the data directory by hand.

The OCI image still leaves it out — it bundles neither Node.js nor `agent-runner/`.
To use Agent Ask there, mount both into the container and point `[ai.agent].runner` at the runner script.

## Development

```sh
cargo test                       # backend unit + integration tests
npm --prefix frontend test       # frontend unit tests (Vitest)
npm --prefix frontend run check  # svelte-check / TypeScript
npm --prefix agent-runner test   # agent runner protocol tests (Agent Ask)
nix fmt                          # format the whole tree (treefmt)
```

`nix flake check` builds the packages and runs the checks (including a NixOS VM
test on Linux).

## License

[MIT](LICENSE) © Jie Zhu
