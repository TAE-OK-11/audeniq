import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ReleasePayload } from './types';
import { ApiError } from './errors';
import { setCsrf, setOrgId } from './http';
import { cleanText, remoteApi, sanitizeProfile, serverUpc, uiStatus, uploadContentType } from './remote';

const ORG = 'org-1';

const payload = (over: Partial<ReleasePayload> = {}): ReleasePayload => ({
  title: '원격 싱글', artist: '원격 아티스트', type: 'single', language: 'ko', genre: 'Pop', genreCustom: '', label: '',
  upc: '', notes: '첫 줄\n둘째 줄', coverName: '', coverData: '', originalDate: '', release_date: '2030-01-01',
  tracks: [
    { id: 't1', title: '첫 곡', version: '', isrc: '', composers: 'c', lyricists: '', arrangers: '', performers: '', producer: '', lyrics: '가사\n둘째 줄', audioName: 'a.wav', audioSize: 1, explicit: false, duration: '03:10', assetId: 'asset-1' },
    { id: 't2', title: '', version: '', isrc: '', composers: '', lyricists: '', arrangers: '', performers: '', producer: '', lyrics: '', audioName: '', audioSize: 0, explicit: false, duration: '' },
  ],
  territories: ['WORLD'], platforms: ['spotify'], ownership: 'A', phonogram: 'p', copyright: 'c',
  rightsChecks: {}, options: {} as ReleasePayload['options'],
  ...over,
});

interface Call { method: string; path: string; body: unknown; headers: Record<string, string> }

/** 백엔드 계약(docs/API.md)을 따르는 작은 가짜 서버 */
function fakeServer() {
  const calls: Call[] = [];
  const releases = new Map<string, { id: string; title: string; release_type: string; status: string; draft: unknown; row_version: number; created_at: string; tracks: Record<string, unknown>[]; upc?: string | null; artwork_asset_id?: string | null }>();
  const artists: { id: string; name: string }[] = [];
  const parties = new Map<string, string>();
  let seq = 0;
  const json = (status: number, data: unknown) => new Response(JSON.stringify(data), { status, headers: { 'content-type': 'application/json' } });

  const handler = async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = new URL(String(input), 'http://studio.test');
    const method = init?.method ?? 'GET';
    const body = init?.body ? JSON.parse(String(init.body)) : undefined;
    calls.push({ method, path: url.pathname, body, headers: (init?.headers ?? {}) as Record<string, string> });
    const p = url.pathname.replace(`/api/orgs/${ORG}`, '');
    if (p === '/artists' && method === 'GET') return json(200, { items: artists, limit: 100, next_cursor: null });
    if (p === '/artists' && method === 'POST') { const a = { id: `artist-${++seq}`, name: body.name }; artists.push(a); return json(200, { id: a.id, row_version: 0 }); }
    // 같은 이름이면 기존 파티를 돌려준다 (서버와 같은 동작)
    if (p === '/parties' && method === 'POST') {
      const hit = parties.get(body.display_name);
      if (hit) return json(200, { party_id: hit, created: false });
      const id = `party-${++seq}`;
      parties.set(body.display_name, id);
      return json(200, { party_id: id, created: true });
    }
    const cr = p.match(/^\/releases\/([^/]+)\/tracks\/([^/]+)\/credits$/);
    if (cr && method === 'PUT') {
      const rel = releases.get(cr[1])!;
      if (body.row_version !== rel.row_version) return json(409, { error: { code: 'CONFLICT', message: 'stale' } });
      rel.row_version += 1;
      const t = rel.tracks.find(x => x.id === cr[2])!;
      t.credits = body.credits;
      return json(200, { id: cr[2], row_version: rel.row_version });
    }
    if (p === '/releases' && method === 'GET') return json(200, { items: [...releases.values()], limit: 100, next_cursor: null });
    if (p === '/releases' && method === 'POST') {
      const id = `rel-${++seq}`;
      releases.set(id, { id, title: body.name, release_type: body.release_type, status: 'DRAFT', draft: body.profile, row_version: 0, created_at: '2030-01-01T00:00:00Z', tracks: [], upc: body.upc, artwork_asset_id: body.artwork_asset_id });
      return json(200, { id, row_version: 0 });
    }
    const sub = p.match(/^\/releases\/([^/]+)\/submission$/);
    if (sub) {
      return json(200, { status: releases.get(sub[1])?.status, checks: [
        { check_code: 'IMAGE_TOO_SMALL', status: 'CORRECTION_REQUIRED', severity: 'CORRECTION', detail: 'w=1000' },
        { check_code: 'IMAGE_TOO_SMALL', status: 'CORRECTION_REQUIRED', severity: 'CORRECTION', detail: 'dup' },
        { check_code: 'TRACK_WRITER_CREDIT_MISSING', status: 'CORRECTION_REQUIRED', severity: 'CORRECTION', detail: 'track=11111111-1111-4111-8111-111111111111 writer_credit=false' },
        { check_code: 'TRACK_WRITER_CREDIT_MISSING', status: 'CORRECTION_REQUIRED', severity: 'CORRECTION', detail: 'track=22222222-2222-4222-8222-222222222222 writer_credit=false' },
        { check_code: 'AUDIO_CLIPPING', status: 'PASS', severity: 'NONE', detail: null },
      ] });
    }
    const m = p.match(/^\/releases\/([^/]+)(\/tracks(?:\/([^/]+))?)?$/);
    if (m) {
      const rel = releases.get(m[1]);
      if (!rel) return json(404, { error: { code: 'NOT_FOUND', message: 'nope' } });
      if (!m[2]) {
        if (method === 'GET') return json(200, rel);
        if (body.row_version !== rel.row_version) return json(409, { error: { code: 'CONFLICT', message: 'stale' } });
        if (method === 'PUT') { rel.title = body.name; rel.draft = body.profile; rel.upc = body.upc; rel.artwork_asset_id = body.artwork_asset_id; rel.row_version += 1; return json(200, { id: rel.id, row_version: rel.row_version }); }
        if (method === 'DELETE') { releases.delete(rel.id); return json(200, { id: rel.id }); }
      }
      if (body?.row_version !== rel.row_version) return json(409, { error: { code: 'CONFLICT', message: 'stale' } });
      rel.row_version += 1;
      if (method === 'POST') { const t = { ...body, id: `track-${++seq}` }; rel.tracks.push(t); return json(200, { id: t.id, row_version: rel.row_version }); }
      const i = rel.tracks.findIndex(t => t.id === m[3]);
      if (method === 'PUT') { rel.tracks[i] = { ...body, id: m[3], credits: rel.tracks[i].credits }; return json(200, { id: m[3], row_version: rel.row_version }); }
      if (method === 'DELETE') { rel.tracks.splice(i, 1); return json(200, { row_version: rel.row_version }); }
    }
    return json(404, { error: { code: 'NOT_FOUND', message: p } });
  };
  return { calls, releases, handler };
}

describe('서버 값 정리', () => {
  it('처리 단계를 화면 상태로 바꾼다', () => {
    expect(uiStatus('DRAFT')).toBe('draft');
    expect(uiStatus('STAGE2_CORRECTION')).toBe('needs');
    expect(uiStatus('ON_HOLD_RIGHTS')).toBe('needs');
    expect(uiStatus('READY_FOR_DELIVERY')).toBe('scheduled');
    expect(uiStatus('STAGE1_REVIEW')).toBe('review');
  });

  it('profile은 lyrics만 여러 줄을 허용하고 제어·방향 문자를 지운다', () => {
    const out = sanitizeProfile({ title: 'a\nb‮', lyrics: 'x\r\ny​', list: ['c\td'], n: 1, gone: undefined }) as Record<string, unknown>;
    expect(out).toEqual({ title: 'a b', lyrics: 'x\ny', list: ['c d'], n: 1 });
    expect(cleanText('  hi\u0007  ')).toBe('hi');
  });

  it('업로드 형식은 서버 허용 목록으로 맞춘다', () => {
    expect(uploadContentType(new File([''], 'a.WAV', { type: '' }), 'AUDIO')).toBe('audio/wav');
    expect(uploadContentType(new File([''], 'a.flac', { type: 'audio/flac' }), 'AUDIO')).toBe('audio/flac');
    expect(uploadContentType(new File([''], 'a.mp3', { type: 'audio/mpeg' }), 'AUDIO')).toBe('');
    expect(uploadContentType(new File([''], 'c.jpeg', { type: '' }), 'IMAGE')).toBe('image/jpeg');
    expect(uploadContentType(new File([''], 'c.webp', { type: 'image/webp' }), 'IMAGE')).toBe('');
  });
});

describe('remoteApi (가짜 서버)', () => {
  let server: ReturnType<typeof fakeServer>;
  beforeEach(() => {
    server = fakeServer();
    vi.stubGlobal('fetch', vi.fn(server.handler));
    setOrgId(ORG);
    setCsrf('csrf-token');
  });
  afterEach(() => { vi.unstubAllGlobals(); setCsrf(''); });

  it('임시 저장: 발매 생성 → 아티스트 → 트랙 → profile 저장, 변경 요청마다 CSRF를 붙인다', async () => {
    const r = await remoteApi.saveDraft(null, payload());
    expect(r.status).toBe('draft');
    expect(r.trackServerIds).toEqual({ t1: expect.stringMatching(/^track-/) });

    const rel = server.releases.get(r.id)!;
    expect(rel.tracks).toHaveLength(1); // 제목 없는 트랙은 profile에만
    expect(rel.tracks[0]).toMatchObject({ title: '첫 곡', track_number: 1, asset_id: 'asset-1', lyrics: '가사\n둘째 줄' });
    const profile = rel.draft as Record<string, unknown>;
    expect(profile.notes_lines).toEqual(['첫 줄', '둘째 줄']);
    expect(JSON.stringify(profile)).not.toMatch(/첫 줄\\n/);

    const mutations = server.calls.filter(c => c.method !== 'GET');
    expect(mutations.length).toBeGreaterThan(0);
    for (const c of mutations) expect(c.headers['X-CSRF-Token']).toBe('csrf-token');
  });

  it('권리 표기를 p_line/c_line으로 보내고, 작곡·작사 이름을 파티 크레딧으로 저장한다', async () => {
    const data = payload();
    data.tracks[0] = { ...data.tracks[0], composers: '김작곡, 이작곡', lyricists: '김작곡', producer: '' };
    const r = await remoteApi.saveDraft(null, data);
    const rel = server.releases.get(r.id)!;
    expect(rel.draft).toMatchObject({ p_line: 'p', c_line: 'c', phonogram: 'p', copyright: 'c' });
    const credits = rel.tracks[0].credits as { party_id: string; role: string }[];
    expect(credits.map(c => c.role).sort()).toEqual(['Composer', 'Composer', 'Lyricist']);
    // 같은 이름은 한 파티로
    const kim = credits.filter(c => c.role === 'Composer')[0].party_id;
    expect(credits.find(c => c.role === 'Lyricist')!.party_id).toBe(kim);

    // 연주곡은 작사 크레딧을 보내지 않는다
    server.calls.length = 0;
    const inst = { ...data, tracks: [{ ...data.tracks[0], serverId: r.trackServerIds!.t1, instrumental: true }] };
    await remoteApi.saveDraft(r.id, inst);
    expect((rel.tracks[0].credits as { role: string }[]).map(c => c.role)).toEqual(['Composer', 'Composer']);
  });

  it('UPC·커버·ISRC를 서버 칸으로 보내고, 없으면 비워서 3단계 발급에 맡긴다', async () => {
    expect(serverUpc('036000291452')).toBe('036000291452');
    expect(serverUpc('0036000291452')).toBe('036000291452');
    expect(serverUpc('4006381333931')).toBeNull();
    expect(serverUpc('')).toBeNull();

    const data = payload({ upc: '036000291452', coverAssetId: 'cover-1' });
    data.tracks[0] = { ...data.tracks[0], isrc: 'kr-abc-26-00001' };
    const r = await remoteApi.saveDraft(null, data);
    const rel = server.releases.get(r.id)!;
    expect(rel).toMatchObject({ upc: '036000291452', artwork_asset_id: 'cover-1' });
    expect(rel.tracks[0].isrc).toBe('KRABC2600001');

    // 비우면 null (3단계가 발급), 커버는 이전 저장값 유지
    const empty = payload({ upc: '' });
    empty.tracks[0] = { ...empty.tracks[0], serverId: r.trackServerIds!.t1 };
    await remoteApi.saveDraft(r.id, empty);
    expect(rel).toMatchObject({ upc: null, artwork_asset_id: 'cover-1' });
    expect(rel.tracks[0].isrc).toBeNull();
  });

  it('보완 요청은 트랙별로 나누고 트랙 ID를 붙인다', async () => {
    const r = await remoteApi.saveDraft(null, payload());
    server.releases.get(r.id)!.status = 'STAGE1_CORRECTION';
    const d = await remoteApi.getRelease(r.id);
    expect(d.corrections).toEqual([
      { code: 'IMAGE_TOO_SMALL', message: '' },
      { code: 'TRACK_WRITER_CREDIT_MISSING', message: '', trackId: '11111111-1111-4111-8111-111111111111' },
      { code: 'TRACK_WRITER_CREDIT_MISSING', message: '', trackId: '22222222-2222-4222-8222-222222222222' },
    ]);
  });

  it('다시 저장하면 바뀐 트랙만 고치고, 지운 트랙은 보관 처리한다', async () => {
    const first = await remoteApi.saveDraft(null, payload());
    const serverId = first.trackServerIds!.t1;
    server.calls.length = 0;

    // 변경 없음 → 트랙 요청 없음
    const same = payload();
    same.tracks[0].serverId = serverId;
    await remoteApi.saveDraft(first.id, same);
    expect(server.calls.filter(c => c.path.includes('/tracks'))).toHaveLength(0);

    // 제목 변경 + 새 곡 추가
    const edited = payload();
    edited.tracks[0] = { ...edited.tracks[0], serverId, title: '첫 곡 (수정)' };
    edited.tracks[1] = { ...edited.tracks[1], title: '둘째 곡' };
    const r2 = await remoteApi.saveDraft(first.id, edited);
    const rel = server.releases.get(first.id)!;
    expect(rel.tracks.map(t => [t.title, t.track_number])).toEqual([['첫 곡 (수정)', 1], ['둘째 곡', 2]]);

    // 첫 곡 삭제
    const removed = payload({ tracks: [{ ...edited.tracks[1], serverId: r2.trackServerIds!.t2 }] });
    await remoteApi.saveDraft(first.id, removed);
    expect(rel.tracks.map(t => t.title)).toEqual(['둘째 곡']);
  });

  it('접수된 발매는 저장하지 않는다', async () => {
    const r = await remoteApi.saveDraft(null, payload());
    server.releases.get(r.id)!.status = 'STAGE1_REVIEW';
    await expect(remoteApi.saveDraft(r.id, payload())).rejects.toMatchObject({ code: 'NOT_EDITABLE' });
  });

  it('보완 단계 발매는 검사 결과의 보완 항목을 함께 돌려준다', async () => {
    const r = await remoteApi.saveDraft(null, payload());
    server.releases.get(r.id)!.status = 'STAGE1_CORRECTION';
    const detail = await remoteApi.getRelease(r.id);
    expect(detail.status).toBe('needs');
    expect(detail.corrections?.map(c => c.code)).toEqual(['IMAGE_TOO_SMALL', 'TRACK_WRITER_CREDIT_MISSING', 'TRACK_WRITER_CREDIT_MISSING']);
    const list = await remoteApi.listReleases();
    expect(list[0].corrections).toHaveLength(3);
  });

  it('서버 오류 코드는 한국어 문구로 바뀐다', async () => {
    await expect(remoteApi.getRelease('missing')).rejects.toSatisfy((e: unknown) =>
      e instanceof ApiError && e.code === 'NOT_FOUND' && /찾을 수 없어요/.test(e.message));
  });

  it('엣지가 본문 없이 돌려준 503도 안내 문구로 바뀐다', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('Private API unavailable', { status: 503 })));
    await expect(remoteApi.listReleases()).rejects.toMatchObject({ code: 'DATABASE_UNAVAILABLE', status: 503 });
  });
});

describe('remoteApi.uploadFile', () => {
  let xhrStatus = 200;
  const sent: { method: string; url: string; headers: Record<string, string> }[] = [];
  class FakeXhr {
    upload: { onprogress: ((e: { lengthComputable: boolean; loaded: number; total: number }) => void) | null } = { onprogress: null };
    onload: (() => void) | null = null;
    onerror: (() => void) | null = null;
    onabort: (() => void) | null = null;
    status = 0;
    private req = { method: '', url: '', headers: {} as Record<string, string> };
    open(method: string, url: string) { this.req.method = method; this.req.url = url; }
    setRequestHeader(k: string, v: string) { this.req.headers[k] = v; }
    abort() { this.onabort?.(); }
    send() {
      sent.push(this.req);
      this.upload.onprogress?.({ lengthComputable: true, loaded: 5, total: 10 });
      this.status = xhrStatus;
      queueMicrotask(() => this.onload?.());
    }
  }

  const calls: string[] = [];
  beforeEach(() => {
    sent.length = 0; calls.length = 0; xhrStatus = 200;
    setOrgId(ORG); setCsrf('csrf');
    vi.stubGlobal('XMLHttpRequest', FakeXhr);
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input).replace(`/api/orgs/${ORG}`, '');
      calls.push(`${init?.method} ${path}`);
      if (path === '/uploads') {
        return Response.json({
          upload_session_id: 'up-1', asset_id: 'asset-9', expected_key: 'k',
          grant: { url: 'https://acct.r2.cloudflarestorage.com/b/k?sig', method: 'PUT', headers: { 'content-type': 'audio/wav', 'content-length': '10', 'x-amz-meta-nonce': 'n' }, expires_at: '' },
        });
      }
      if (path === '/uploads/up-1/complete') return Response.json({ asset_id: 'asset-9', sha256: 'abc' });
      return Response.json({});
    }));
  });
  afterEach(() => vi.unstubAllGlobals());

  it('서명 URL로 직접 올리고 완료를 알린다 (content-length는 브라우저에 맡김)', async () => {
    const progress: number[] = [];
    const r = await remoteApi.uploadFile(new File([new Uint8Array(10)], 'a.wav', { type: 'audio/wav' }), 'AUDIO', p => progress.push(p));
    expect(r.assetId).toBe('asset-9');
    expect(progress).toEqual([0.5]);
    expect(sent[0].url).toContain('r2.cloudflarestorage.com');
    expect(sent[0].headers).toEqual({ 'content-type': 'audio/wav', 'x-amz-meta-nonce': 'n' });
    expect(calls).toEqual(['POST /uploads', 'POST /uploads/up-1/complete']);
  });

  it('저장소 업로드가 실패하면 업로드 세션을 취소한다', async () => {
    xhrStatus = 403;
    await expect(remoteApi.uploadFile(new File([new Uint8Array(10)], 'a.wav'), 'AUDIO')).rejects.toMatchObject({ code: 'UPLOAD_PUT_FAILED' });
    await Promise.resolve();
    expect(calls).toEqual(['POST /uploads', 'POST /uploads/up-1/cancel']);
  });

  it('허용하지 않는 형식은 요청 전에 막는다', async () => {
    await expect(remoteApi.uploadFile(new File([''], 'a.mp3', { type: 'audio/mpeg' }), 'AUDIO')).rejects.toMatchObject({ code: 'UPLOAD_TYPE_UNSUPPORTED' });
    expect(calls).toHaveLength(0);
  });
});
