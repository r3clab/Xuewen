# Deploying Xuewen to Kubernetes

## Build and push the image

```sh
skopeo login ghcr.io          # GitHub username + a token with write:packages
nix run .#push -- deploy/k8s  # pushes ghcr.io/r3clab/xuewen:<rev> and pins <rev> in deploy/k8s/kustomization.yaml
```

`<rev>` is the commit's short hash, or `<rev>-dirty` for a tree with uncommitted changes.
The directory argument is optional and may point anywhere; without it the script only pushes.
Commit the updated `kustomization.yaml` to record what the cluster runs.

## Deploy

```sh
kubectl apply -k deploy/k8s/
```

The Deployment runs the image with a read-only root filesystem, no capabilities and the runtime's default seccomp profile; `/data` (the PVC) and `/tmp` (an `emptyDir`) are its only writable paths.
The startup and liveness probes hit `/api/health`, which never touches the database; readiness hits `/api/stats`.

Optional semantic search — create the `xuewen-secrets` Secret with the
embedding API key:

```sh
kubectl create secret generic xuewen-secrets --from-literal=OPENAI_API_KEY=sk-...
```

then restart the pod.
Alternative, file-based flow: `cp deploy/k8s/secret.yaml.example deploy/k8s/secret.yaml`, fill in `OPENAI_API_KEY`, and `kubectl apply -f deploy/k8s/secret.yaml` (`secret.yaml` is gitignored and not listed in `kustomization.yaml`, so it is never applied by `kubectl apply -k deploy/k8s/` and never accidentally committed).
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

## Upgrading from the Qdrant pod

Vectors now live inside the xuewen pod, in `/data/search-vectors` on the `xuewen-data` PVC; there is no separate Qdrant Deployment any more.
The new store starts empty, so the first start re-embeds the whole library through the embedding API, which costs API calls and takes a while.
Leftover `qdrant_url` / `qdrant_collection` keys in the ConfigMap are ignored.

`kubectl apply` does not prune, so the old Qdrant resources keep running after the upgrade.
Once semantic search works, delete them by hand:

```sh
kubectl delete deployment/xuewen-qdrant service/xuewen-qdrant pvc/xuewen-qdrant-storage
```

## Notes

- `replicas` must stay 1 (SQLite, Tantivy and the vector store are single-writer); the Recreate strategy makes rollouts safe on the shared PVC.
- All state lives in the `xuewen-data` PVC (`/data`): the SQLite DB, PDFs, inbox, the Tantivy index, and the vector store (`/data/search-vectors`).
- `kubectl exec deploy/xuewen -- xuewen search '…'` works while the server runs: the CLI forwards the query to it over HTTP.
  The image's `XUEWEN_CONFIG` points every `xuewen` command at the mounted ConfigMap.
- Validate manifests offline: `kubectl kustomize deploy/k8s/`
- Local smoke test without a cluster:

  ```sh
  nix run .#load                      # prints the loaded image:tag
  docker run --rm -p 8080:8080 -v xuewen-data:/data ghcr.io/r3clab/xuewen:<tag>
  curl http://localhost:8080/api/health
  ```

- Inbox watching (`xuewen watch`) is deliberately not run in the pod (one process per container).
  Import through the web UI, or for one-off files without the UI:

  ```sh
  kubectl port-forward svc/xuewen 8080:8080
  curl -F file=@paper.pdf http://localhost:8080/api/papers
  ```

  `kubectl cp` does NOT work here — the image has no `tar`.
- Vector rebuild (e.g. after changing `[ai.embedding].dims`): `index rebuild` refuses to run while the server holds the vector store, so it cannot run in the live pod.
  Scale the Deployment to 0, run the rebuild in a one-off Job on the Deployment's own image that mounts the same PVC and ConfigMap, then scale back to 1:

  ```sh
  IMAGE=$(kubectl get deploy/xuewen -o jsonpath='{.spec.template.spec.containers[0].image}')
  kubectl scale deploy/xuewen --replicas=0
  kubectl wait --for=delete pod -l app=xuewen --timeout=2m
  kubectl apply -f - <<EOF
  apiVersion: batch/v1
  kind: Job
  metadata:
    name: xuewen-rebuild
  spec:
    backoffLimit: 0
    template:
      spec:
        restartPolicy: Never
        securityContext: { runAsNonRoot: true, runAsUser: 1000, runAsGroup: 1000, fsGroup: 1000 }
        containers:
          - name: rebuild
            image: $IMAGE
            args: [index, rebuild, --vectors-only]
            envFrom:
              - secretRef: { name: xuewen-secrets }
            volumeMounts:
              - { name: data, mountPath: /data }
              - { name: config, mountPath: /etc/xuewen }
        volumes:
          - name: data
            persistentVolumeClaim: { claimName: xuewen-data }
          - name: config
            configMap: { name: xuewen-config }
  EOF
  kubectl wait --for=condition=complete --timeout=1h job/xuewen-rebuild
  kubectl logs job/xuewen-rebuild
  kubectl delete job/xuewen-rebuild
  kubectl scale deploy/xuewen --replicas=1
  ```

  The same applies to every `index rebuild` scope (drop `--vectors-only` to rebuild both indexes).
