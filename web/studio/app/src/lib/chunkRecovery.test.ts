import { afterEach, describe, expect, it, vi } from 'vitest';
import { claimChunkReload, clearChunkReload, isChunkError } from './chunkRecovery';

afterEach(() => { vi.restoreAllMocks(); sessionStorage.clear(); });

describe('Safari module load recovery', () => {
  it('recognizes Safari and Chromium module errors without masking a render error', () => {
    for (const message of ['Load failed', 'Importing a module script failed.', 'Failed to fetch dynamically imported module: /assets/old.js']) {
      expect(isChunkError(new TypeError(message))).toBe(true);
    }
    expect(isChunkError(new TypeError('loginFrom.startsWith is not a function'))).toBe(false);
    expect(isChunkError(null)).toBe(false);
  });
  it('allows one automatic reload across both preload and lazy-page handlers', () => {
    expect(claimChunkReload()).toBe(true);
    expect(claimChunkReload()).toBe(false);
    clearChunkReload();
    expect(claimChunkReload()).toBe(true);
  });
  it('keeps the original error and allows manual retry when Safari blocks storage', () => {
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => { throw new DOMException('Blocked', 'SecurityError'); });
    vi.spyOn(Storage.prototype, 'removeItem').mockImplementation(() => { throw new DOMException('Blocked', 'SecurityError'); });
    expect(claimChunkReload()).toBe(false);
    expect(() => clearChunkReload()).not.toThrow();
  });
});
