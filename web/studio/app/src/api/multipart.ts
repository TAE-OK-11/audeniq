// 큰 음원 이어 올리기 — 16 MiB 조각으로 나눠 올린다 (서버: /uploads/{id}/parts).
// - 조각마다 서명 링크를 그때그때 받아(15분) 올리므로 느린 회선에서도 링크가 만료되지 않는다.
// - 연결이 끊기면 그 조각만 다시 보낸다 (처음부터 다시 올리지 않는다).
// - 새로고침·창을 닫았다가 같은 파일을 다시 고르면, 서버에 이미 있는 조각은 건너뛰고 이어서 올린다 (24시간).
import { ApiError } from './errors';
import { orgId, orgPath, putToGrant, req, type UploadGrant } from './http';
import { readJSON, removeKey, writeJSON } from '../lib/storage';

export interface MultipartIssued {
  upload_session_id: string;
  asset_id: string;
  expected_key: string;
  multipart: { part_size: number; part_count: number };
  expires_at: string;
}

interface Saved { session: string; assetId: string; key: string; org: string; expiresAt: string }

const PARALLEL = 2;
const PART_RETRIES = 5;

/** 같은 파일인지 알아보는 표시 (이름·크기·수정 시각) */
const fileKey = (file: File) => `upload.resume.${file.name}.${file.size}.${file.lastModified}`;

const sleep = (ms: number, signal?: AbortSignal) => new Promise<void>((resolve, reject) => {
  const abort = () => { clearTimeout(t); reject(new ApiError('업로드를 취소했어요.', 0, 'ABORTED')); };
  const t = setTimeout(() => { signal?.removeEventListener('abort', abort); resolve(); }, ms);
  signal?.addEventListener('abort', abort, { once: true });
  if (signal?.aborted) abort();
});

/** 전에 올리다 만 같은 파일의 세션 (아직 열려 있을 때만) */
export function savedSession(file: File): Saved | null {
  const s = readJSON<Saved | null>(fileKey(file), null);
  if (!s || s.org !== orgId() || !s.session || Date.parse(s.expiresAt) <= Date.now() + 60_000) {
    if (s) removeKey(fileKey(file));
    return null;
  }
  return s;
}
export const forgetSession = (file: File) => removeKey(fileKey(file));

export function rememberSession(file: File, issued: MultipartIssued) {
  writeJSON(fileKey(file), {
    session: issued.upload_session_id, assetId: issued.asset_id, key: issued.expected_key,
    org: orgId(), expiresAt: issued.expires_at,
  } satisfies Saved);
}

/**
 * 조각을 모두 올린다. 이미 서버에 있는 조각은 건너뛴다.
 * 실패해도 세션은 취소하지 않는다 — 같은 파일을 다시 고르면 이어서 올린다.
 */
export async function uploadParts(
  file: File,
  session: string,
  onProgress?: (r: number) => void,
  signal?: AbortSignal,
): Promise<void> {
  const base = orgPath(`/uploads/${encodeURIComponent(session)}/parts`);
  const stored = await req<{ part_size: number; part_count: number; parts: { part_number: number; size_bytes: number }[] }>(base);
  const { part_size: partSize, part_count: count } = stored;
  const done = new Set(stored.parts.map(p => p.part_number));
  const loaded = new Map<number, number>();
  for (const p of stored.parts) loaded.set(p.part_number, p.size_bytes);
  const report = () => {
    let n = 0;
    for (const v of loaded.values()) n += v;
    onProgress?.(Math.min(1, n / file.size));
  };
  report();
  const todo: number[] = [];
  for (let n = 1; n <= count; n++) if (!done.has(n)) todo.push(n);

  // 링크는 여러 조각씩 미리 받아 두고, 오래된 링크는 다시 받는다.
  const grants = new Map<number, { grant: UploadGrant; expires: number }>();
  const grantFor = async (n: number): Promise<UploadGrant> => {
    const g = grants.get(n);
    if (g && g.expires - Date.now() > 60_000) return g.grant;
    const batch = [n, ...todo.filter(x => x > n && !grants.has(x))].slice(0, 8);
    const r = await req<{ parts: { part_number: number; grant: UploadGrant }[] }>(base, { method: 'POST', body: { part_numbers: batch } });
    for (const p of r.parts) grants.set(p.part_number, { grant: p.grant, expires: Date.parse(p.grant.expires_at) });
    return grants.get(n)!.grant;
  };

  const sendPart = async (n: number) => {
    const start = (n - 1) * partSize;
    const blob = file.slice(start, Math.min(file.size, start + partSize));
    for (let attempt = 0; ; attempt++) {
      if (signal?.aborted) throw new ApiError('업로드를 취소했어요.', 0, 'ABORTED');
      try {
        const grant = await grantFor(n);
        await putToGrant(grant, blob, r => { loaded.set(n, r * blob.size); report(); }, signal);
        loaded.set(n, blob.size);
        report();
        return;
      } catch (e) {
        loaded.set(n, 0);
        report();
        if (e instanceof ApiError && e.code === 'ABORTED') throw e;
        // 링크가 만료됐거나 거절되면 새 링크로 다시 시도한다.
        grants.delete(n);
        if (attempt + 1 >= PART_RETRIES) {
          throw new ApiError('연결이 끊겨 업로드를 멈췄어요. 같은 파일을 다시 선택하면 올린 부분부터 이어서 올려요.', 0, 'UPLOAD_INTERRUPTED');
        }
        await sleep(Math.min(16_000, 1000 * 2 ** attempt) + Math.random() * 500, signal);
      }
    }
  };

  let next = 0;
  const worker = async () => {
    while (next < todo.length) {
      const n = todo[next++];
      await sendPart(n);
    }
  };
  await Promise.all(Array.from({ length: Math.min(PARALLEL, todo.length) }, worker));
}
