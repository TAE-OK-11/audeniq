/** Read persisted login navigation only after checking its runtime shape. */
export function loginRedirect(state: unknown): string | undefined {
  if (!state || typeof state !== 'object') return undefined;
  const from = (state as { from?: unknown }).from;
  let path: unknown = from;
  if (from && typeof from === 'object') {
    const location = from as { pathname?: unknown; search?: unknown; hash?: unknown };
    if (typeof location.pathname !== 'string'
      || (location.search !== undefined && typeof location.search !== 'string')
      || (location.hash !== undefined && typeof location.hash !== 'string')) return undefined;
    path = location.pathname + (location.search ?? '') + (location.hash ?? '');
  }
  if (typeof path !== 'string' || !path.startsWith('/') || path.startsWith('//')
    || /[\\\u0000-\u0020]/.test(path) || /^\/(login|signup|find-account)(?:[/?#]|$)/.test(path)) return undefined;
  return path;
}
