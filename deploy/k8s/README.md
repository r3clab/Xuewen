# Deploying Xuewen to Kubernetes

## Build and push the image

```sh
skopeo login ghcr.io          # GitHub username + a token with write:packages
nix run .#push                # pushes ghcr.io/r3clab/xuewen:<rev> and :latest
```

Run `nix run .#push` from the repo root (it resolves the flake in the
current directory).

## Deploy

```sh
kubectl apply -f deploy/k8s/
```

Optional semantic search — create the `xuewen-secrets` Secret with the
embedding API key:

```sh
kubectl create secret generic xuewen-secrets --from-literal=OPENAI_API_KEY=sk-...
```

then restart the pod.
Alternative, file-based flow: `cp deploy/k8s/secret.yaml.example deploy/k8s/secret.yaml`, fill in `OPENAI_API_KEY`, and `kubectl apply -f deploy/k8s/secret.yaml` (`secret.yaml` is gitignored, so it's never picked up by `kubectl apply -f deploy/k8s/` and never accidentally committed).
Any OpenAI-compatible endpoint works — edit the ConfigMap's `[ai.embedding]` block and `kubectl rollout restart deployment/xuewen`.

## Expose it — read this first

The pod has NO authentication: anyone who can reach the Service can import and delete papers.
Keep the Service ClusterIP-only and put your own authenticating ingress (oauth2-proxy, Authelia, Tailscale, …) in front.
No Ingress manifest ships here because it is cluster-specific.

## Daily arXiv papers on Glance

`xuewen serve` exposes daily arXiv recommendations at `GET /api/daily`
when the ConfigMap's `xuewen.toml` has a `[daily]` section (requires `[ai.embedding]` and the
`OPENAI_API_KEY` secret, which the summary generation shares by default).

Add a `custom-api` widget to your Glance dashboard's `glance.yml`:

```yaml
- type: custom-api
  title: Daily arXiv
  cache: 1h
  url: http://xuewen.<namespace>.svc.cluster.local:8080/api/daily
  template: |
    {{ if .JSON.Array "papers" }}
    <p class="size-h6 color-subdue">{{ .JSON.String "date" }}</p>
    <ul class="list list-gap-14">
      {{ range .JSON.Array "papers" }}
      <li>
        <a class="size-h4 color-primary" href="{{ .String "abs_url" }}">{{ .String "title" }}</a>
        <div class="size-h6 color-subdue">
          {{ printf "%.2f" (.Float "score") }} · {{ .String "arxiv_id" }} ·
          <a href="{{ .String "pdf_url" }}">PDF</a>
        </div>
        <p>{{ if .String "tldr" }}{{ .String "tldr" }}{{ else }}{{ .String "abstract" }}{{ end }}{{ if .String "code_url" }} · <a href="{{ .String "code_url" }}">Code</a>{{ end }}</p>
        {{ if .String "summary.problem" }}
        <details>
          <summary class="size-h6 color-subdue">details</summary>
          <p><strong>Problem:</strong> {{ .String "summary.problem" }}</p>
          <p><strong>Approach:</strong> {{ .String "summary.approach" }}</p>
          <p><strong>Results:</strong> {{ .String "summary.results" }}</p>
          <p><strong>Limitations:</strong> {{ .String "summary.limitations" }}</p>
        </details>
        {{ end }}
      </li>
      {{ end }}
    </ul>
    {{ else }}
    <p>No papers yet — the first batch appears after the daily run.</p>
    {{ end }}
```

The template targets Glance's `custom-api` widget — check it against your
installed Glance version's documentation and adjust if the template API
differs.

Trigger a run without waiting for the schedule:

```sh
kubectl port-forward svc/xuewen 8080:8080
curl -X POST localhost:8080/api/daily/run
```

## Notes

- `replicas` must stay 1 (SQLite and Tantivy are single-writer); the
  Recreate strategy makes rollouts safe on the shared PVC.
- All state lives in the `xuewen-data` PVC (`/data`): the SQLite DB, PDFs, inbox, and the Tantivy index.
  Qdrant state is separate and rebuildable (`kubectl exec deploy/xuewen -- xuewen --config /etc/xuewen/xuewen.toml index rebuild --vectors-only` — the explicit `--config` is required because the default looks for `./xuewen.toml`).
- Validate manifests offline: `kubectl apply --dry-run=client --validate=false -f deploy/k8s/`
- Local smoke test without a cluster:

  ```sh
  nix run .#load                      # prints the loaded image:tag
  docker images ghcr.io/r3clab/xuewen   # find the tag (git short rev, or "dev")
  docker run --rm -p 8080:8080 -v xuewen-data:/data ghcr.io/r3clab/xuewen:<tag>
  curl http://localhost:8080/api/stats
  ```

- Inbox watching (`xuewen watch`) is deliberately not run in the pod (one process per container).
  Import through the web UI, or for one-off files without the UI:

  ```sh
  kubectl port-forward svc/xuewen 8080:8080
  curl -F file=@paper.pdf http://localhost:8080/api/papers
  ```

  `kubectl cp` does NOT work here — the image has no `tar`.
- Vector rebuild in-pod:

  ```sh
  kubectl exec deploy/xuewen -- xuewen --config /etc/xuewen/xuewen.toml index rebuild --vectors-only
  ```

  The explicit `--config` is required — without it, `xuewen` looks for
  `./xuewen.toml` and won't find the mounted ConfigMap.
