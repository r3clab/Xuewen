/// What to tell the user about a thrown value. Anything can be thrown; only
/// an Error is known to carry a message, so everything else (and an Error
/// with an empty one) gets `fallback` rather than "[object Object]".
export function errorMessage(e: unknown, fallback: string): string {
  return e instanceof Error && e.message ? e.message : fallback;
}
