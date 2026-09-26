// 배급 신청서 — 신청인이 서명해 접수한 내용을 고정하고(해시), 정식 서류로 보여 줄 데이터를 만든다.
import type { ApplicationRecord, DraftTrack, ReleaseDetail, ReleasePayload } from '../api/types';
import { cleanText } from '../api/remote';
import { stampNow } from './date';

export const APPLICATION_FORM = 'AQ-DIST-APP 1.0';

/** 신고 항목 이름 (신청서 표기용) */
export const OPTION_LABELS: Record<string, string> = {
  express: '신속 발매 요청', minor: '미성년 아티스트·권리자', cover: '커버곡', sample: '샘플링·타인 음원 사용',
  featured: '피처링·공동 실연', ai: 'AI 생성·보조 제작', shared: '공동 권리자·레이블 계약', rerelease: '기존 발매 이전·재발매',
};

export const SIGNER_ROLES = ['아티스트 본인', '소속사·레이블 대표자', '법정대리인 (미성년 아티스트)'];

export const AGREEMENTS: { id: string; text: string }[] = [
  { id: 'truth', text: '신청 내용이 사실이며, 음원·가사·커버아트·크레딧을 배급할 적법한 권리를 가지고 있음을 확인합니다.' },
  { id: 'terms', text: 'AUDENIQ 디지털 음원 배급 약관과 정산·수정·테이크다운 조건에 동의합니다.' },
  { id: 'privacy', text: '배급·정산을 위한 개인정보 수집·이용 및 음원 플랫폼 제공에 동의합니다.' },
  { id: 'esign', text: '이 전자서명이 자필 서명과 같은 효력을 가진다는 데 동의합니다.' },
];

// ---------------------------------------------------------------------------
// 서명 이미지 — 그린 부분만 잘라 작게 저장 (발매 정보 64KB 한도 안)
// ---------------------------------------------------------------------------
export function compactSignature(dataUrl: string, maxW = 420, maxH = 140): Promise<string> {
  return new Promise(resolve => {
    const img = new Image();
    img.onload = () => {
      const src = document.createElement('canvas');
      src.width = img.width; src.height = img.height;
      const sctx = src.getContext('2d');
      if (!sctx) { resolve(dataUrl); return; }
      sctx.drawImage(img, 0, 0);
      const { data, width, height } = sctx.getImageData(0, 0, src.width, src.height);
      let x0 = width, y0 = height, x1 = -1, y1 = -1;
      for (let y = 0; y < height; y++) {
        for (let x = 0; x < width; x++) {
          if (data[(y * width + x) * 4 + 3] > 16) {
            if (x < x0) x0 = x; if (x > x1) x1 = x;
            if (y < y0) y0 = y; if (y > y1) y1 = y;
          }
        }
      }
      if (x1 < 0) { resolve(''); return; }
      const pad = 6;
      x0 = Math.max(0, x0 - pad); y0 = Math.max(0, y0 - pad);
      x1 = Math.min(width - 1, x1 + pad); y1 = Math.min(height - 1, y1 + pad);
      const w = x1 - x0 + 1, h = y1 - y0 + 1;
      const scale = Math.min(1, maxW / w, maxH / h);
      const out = document.createElement('canvas');
      out.width = Math.max(1, Math.round(w * scale));
      out.height = Math.max(1, Math.round(h * scale));
      out.getContext('2d')?.drawImage(src, x0, y0, w, h, 0, 0, out.width, out.height);
      resolve(out.toDataURL('image/png'));
    };
    img.onerror = () => resolve(dataUrl);
    img.src = dataUrl;
  });
}

// ---------------------------------------------------------------------------
// 해시 — 신청 내용이 접수 후 바뀌지 않았는지 확인
// ---------------------------------------------------------------------------
export async function sha256Hex(text: string): Promise<string> {
  const bytes = new TextEncoder().encode(text);
  if (globalThis.crypto?.subtle) {
    const buf = await crypto.subtle.digest('SHA-256', bytes);
    return [...new Uint8Array(buf)].map(b => b.toString(16).padStart(2, '0')).join('');
  }
  // 보안 컨텍스트가 아닌 환경(구형 브라우저) 대비 — 위·변조 확인용이 아니라 표시용
  let h = 0x811c9dc5;
  for (const b of bytes) { h ^= b; h = Math.imul(h, 0x01000193) >>> 0; }
  return h.toString(16).padStart(8, '0').repeat(8);
}

/** 해시 대상 — 저장 과정(서버 문자열 정리 포함)을 거쳐도 같은 값이 나오도록 정리한 신청 내용 */
export interface ApplicationSnapshot {
  title: string; artist: string; type: string; language: string; genre: string; label: string;
  upc: string; releaseDate: string; originalDate: string;
  territories: string[]; platforms: string[];
  ownership: string; phonogram: string; copyright: string;
  options: string[];
  artistProfile: { isNew: boolean; spotify: string; apple: string; melon: string };
  tracks: {
    title: string; version: string; featuring: string; isrc: string; composers: string; lyricists: string;
    arrangers: string; performers: string; producer: string; explicit: boolean; instrumental: boolean;
    duration: string; audioName: string; audioSpec: string;
  }[];
}

const c = (v: unknown) => cleanText(typeof v === 'string' ? v : '');
const OPTION_KEYS = ['express', 'minor', 'cover', 'sample', 'featured', 'ai', 'shared', 'rerelease'] as const;

function snapTracks(tracks: DraftTrack[]): ApplicationSnapshot['tracks'] {
  return tracks.filter(t => c(t.title)).map(t => ({
    title: c(t.title), version: c(t.version), featuring: c(t.featuring), isrc: c(t.isrc),
    composers: c(t.composers), lyricists: t.instrumental ? '' : c(t.lyricists), arrangers: c(t.arrangers),
    performers: c(t.performers), producer: c(t.producer), explicit: !!t.explicit, instrumental: !!t.instrumental,
    duration: c(t.duration), audioName: c(t.audioName), audioSpec: c(t.audioSpec),
  }));
}

function snapBase(p: {
  title: string; artist?: string; type?: string; language?: string; genre?: string; label?: string; upc?: string;
  releaseDate?: string | null; originalDate?: string; territories?: string[]; platforms?: string[];
  ownership?: string; phonogram?: string; copyright?: string; options?: object; artistProfile?: ApplicationSnapshot['artistProfile'];
  tracks: DraftTrack[];
}): ApplicationSnapshot {
  const o = (p.options ?? {}) as Record<string, unknown>;
  const ap = p.artistProfile;
  return {
    title: c(p.title), artist: c(p.artist), type: c(p.type), language: c(p.language), genre: c(p.genre),
    label: c(p.label), upc: c(p.upc), releaseDate: c(p.releaseDate ?? ''), originalDate: c(p.originalDate),
    territories: [...(p.territories ?? [])], platforms: [...(p.platforms ?? [])],
    ownership: c(p.ownership), phonogram: c(p.phonogram), copyright: c(p.copyright),
    options: OPTION_KEYS.filter(k => o[k] === true),
    artistProfile: ap && !ap.isNew
      ? { isNew: false, spotify: c(ap.spotify), apple: c(ap.apple), melon: c(ap.melon) }
      : { isNew: true, spotify: '', apple: '', melon: '' },
    tracks: snapTracks(p.tracks),
  };
}

export function snapshotFromPayload(p: ReleasePayload): ApplicationSnapshot {
  return snapBase({ ...p, releaseDate: p.release_date });
}

export function snapshotFromRelease(r: ReleaseDetail): ApplicationSnapshot {
  const d = r.draft;
  return snapBase({
    ...d, title: r.title, artist: d?.artist ?? r.artist, releaseDate: r.release_date,
    tracks: d?.draftTracks ?? [],
  });
}

function hashInput(snap: ApplicationSnapshot, rec: Omit<ApplicationRecord, 'hash'>): string {
  return JSON.stringify({
    form: rec.form, no: rec.no, submittedAt: rec.submittedAt, signerName: c(rec.signerName), signerRole: rec.signerRole,
    agreements: rec.agreements, signature: rec.signature, release: snap,
  });
}

function newApplicationNo(date: string): string {
  const alphabet = 'ABCDEFGHJKLMNPQRSTUVWXYZ23456789';
  const bytes = new Uint8Array(6);
  crypto.getRandomValues(bytes);
  return `AQ-${date.replace(/-/g, '')}-${[...bytes].map(b => alphabet[b % alphabet.length]).join('')}`;
}

export async function createApplication(args: {
  payload: ReleasePayload; signerName: string; signerRole: string; signature: string; agreements: string[];
}): Promise<ApplicationRecord> {
  const submittedAt = stampNow();
  const base: Omit<ApplicationRecord, 'hash'> = {
    no: newApplicationNo(submittedAt.slice(0, 10)),
    form: APPLICATION_FORM,
    submittedAt,
    signerName: args.signerName.trim(),
    signerRole: args.signerRole,
    signature: args.signature,
    agreements: args.agreements,
  };
  return { ...base, hash: await sha256Hex(hashInput(snapshotFromPayload(args.payload), base)) };
}

/** 저장된 발매 내용으로 해시를 다시 계산해 신청서가 바뀌지 않았는지 확인 */
export async function verifyApplication(rec: ApplicationRecord, rel: ReleaseDetail): Promise<boolean> {
  const { hash, ...base } = rec;
  return (await sha256Hex(hashInput(snapshotFromRelease(rel), base))) === hash;
}

/** 해시를 사람이 읽기 좋게: 4자리씩 끊어 앞 32자 */
export const hashLabel = (h: string) => h.slice(0, 32).toUpperCase().match(/.{1,4}/g)?.join(' ') ?? '';
