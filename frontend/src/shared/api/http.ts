/** A non-2xx API response, carrying the HTTP status so callers can branch on
 *  it (e.g. 404 → the paper is gone) without parsing the message back apart. */
export class ApiError extends Error {
  constructor(
    message: string,
    readonly status: number,
  ) {
    super(message);
    this.name = 'ApiError';
  }
}

/** `{error: "..."}` from the response body when present, else
 *  `${fallback}: ${status}`. The one home for API error extraction. */
async function errorFromResponse(res: Response, fallback: string): Promise<never> {
  let msg = `${fallback}: ${res.status}`;
  try {
    const j = await res.json();
    if (j && typeof j.error === 'string') msg = j.error;
  } catch {
    /* non-JSON error body */
  }
  throw new ApiError(msg, res.status);
}

export interface RequestOpts {
  method?: string;
  /** JSON-encoded request body, sent as `content-type: application/json`. */
  json?: unknown;
  /** Pre-encoded body (FormData upload) — the browser picks the content-type. */
  body?: BodyInit;
  signal?: AbortSignal;
}

/** fetch with every non-2xx funnelled through `errorFromResponse`, so the
 *  server's `{error}` body reaches the user from any endpoint. The variants
 *  below cover the response shapes; nothing in the api modules calls fetch
 *  itself. */
export async function requestRaw(url: string, fallback: string, opts: RequestOpts = {}): Promise<Response> {
  const init: RequestInit = { method: opts.method, signal: opts.signal };
  if (opts.json !== undefined) {
    init.headers = { 'content-type': 'application/json' };
    init.body = JSON.stringify(opts.json);
  } else if (opts.body !== undefined) {
    init.body = opts.body;
  }
  const res = await fetch(url, init);
  if (!res.ok) return errorFromResponse(res, fallback);
  return res;
}

export async function request<T>(url: string, fallback: string, opts: RequestOpts = {}): Promise<T> {
  const res = await requestRaw(url, fallback, opts);
  return res.json();
}

/** `request` for endpoints whose success body is ignored (204/empty). */
export async function requestVoid(url: string, fallback: string, opts: RequestOpts = {}): Promise<void> {
  await requestRaw(url, fallback, opts);
}

export async function requestText(url: string, fallback: string): Promise<string> {
  const res = await requestRaw(url, fallback);
  return res.text();
}
