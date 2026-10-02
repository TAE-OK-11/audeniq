// Safari may report a failed module request as "Load failed". Storage denial
// must not replace that error or start an automatic reload loop.
const RELOAD_FLAG = 'aq.chunk-reload';

export function isChunkError(error: unknown): boolean {
  const message = error && typeof error === 'object' && 'message' in error ? error.message : error;
  return typeof message === 'string'
    && /Loading chunk|dynamically imported module|Importing a module script failed|Failed to fetch|^Load failed\.?$/i.test(message);
}

export function claimChunkReload(): boolean {
  try {
    if (sessionStorage.getItem(RELOAD_FLAG)) return false;
    sessionStorage.setItem(RELOAD_FLAG, '1');
    return true;
  } catch { return false; }
}

export function clearChunkReload(): void {
  try { sessionStorage.removeItem(RELOAD_FLAG); } catch { /* Storage may be disabled in Safari. */ }
}
