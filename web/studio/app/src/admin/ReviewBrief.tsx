// 결정 창(승인 · 보완 요청 · 거절) 안에서 바로 보는 ‘확인할 것’ — 심사 시트를 오르내리지 않고 판단하도록
// 신청서 원본 일치, 담당자 확인 검사, 아티스트가 체크하지 않은 권리 확인, 첨부가 빠진 해당 항목,
// 대기 중인 서류, 2차 승인, 막힌 플랫폼, Content ID 권리 확인, 발매일까지 남은 기간, 음원·크레딧·가사가 빠진 트랙을 한 목록으로 모은다.
// 보완 요청 창에서는 문제마다 ‘항목으로 추가’를 눌러 바로 보완 항목으로 만든다.
import type { ReactNode } from 'react';
import type { ReleaseSheet } from './api';
import { checkLabel, checkSummary, dspLabel } from './labels';
import { CONTENT_ID_ACKS, OPTION_LABELS, draftNotes, genreLabel, kindLabel, languageLabel, rightsChecksFor, wantsContentId, type Integrity, type StudioDraft, type StudioOptions } from './application';
import { correctionTarget, isKnownCorrection } from '../lib/corrections';
import { CheckIcon } from '../components/Check';
import { Glyph } from '../components/Glyph';

export type BriefTone = 'ok' | 'warn' | 'bad';
export interface BriefFix { code: string; note: string; trackId?: string; system?: boolean }
export interface BriefItem { key: string; tone: BriefTone; title: string; detail?: string; fix?: BriefFix }

const DAY = 86_400_000;
const daysUntil = (d?: string | null) => {
  if (!d || !/^\d{4}-\d{2}-\d{2}/.test(d)) return null;
  const kstToday = new Date(Date.now() + 9 * 3_600_000).toISOString().slice(0, 10);
  return Math.round((Date.parse(d.slice(0, 10)) - Date.parse(kstToday)) / DAY);
};

export function reviewBrief(sheet: ReleaseSheet, integrity: Integrity): BriefItem[] {
  const out: BriefItem[] = [];
  const d: StudioDraft | null = sheet.draft ?? null;
  const o = (d?.options ?? sheet.application.options ?? null) as StudioOptions | null;

  // 신청서 원본
  if (integrity === 'changed') out.push({ key: 'integrity', tone: 'bad', title: '신청서가 서명 후 달라졌어요', detail: '배급 신청서 전체를 열어 입력 내용과 대조해 주세요.' });
  else if (integrity === 'ok') out.push({ key: 'integrity', tone: 'ok', title: '신청서가 서명한 원본과 일치해요' });
  else if (integrity === 'unsigned') out.push({ key: 'integrity', tone: 'warn', title: '전자서명 신청서 기록이 없어요', detail: '스튜디오 밖에서 접수됐거나 서명 전 기록이에요.' });

  // 시스템이 넘긴 검사
  for (const c of sheet.open_checks) {
    out.push({ key: `chk-${c.check_code}`, tone: 'bad', title: checkLabel(c.check_code), detail: checkSummary(c), fix: { code: c.check_code, note: correctionTarget(c.check_code).hint, system: true } });
  }

  // 2차 승인 — 승인 버튼은 서버가 막고, 보완 요청·거절은 그대로 가능
  if (sheet.review_context?.pending_second_approval_id) {
    out.push({ key: 'second', tone: 'warn', title: '2차 승인 대기 중', detail: '다른 담당자가 승인하기 전에는 승인할 수 없어요. 보완 요청·거절은 할 수 있어요.' });
  } else if (sheet.review_context?.requires_second_approval) {
    out.push({ key: 'second', tone: 'warn', title: '승인하면 2차 승인 요청이 올라가요', detail: '민감 항목이 있어 다른 담당자의 확인이 필요해요.' });
  }

  // 플랫폼별 배급 준비 — 콘텐츠 차단·파트너 대기
  for (const row of sheet.delivery_staging ?? []) {
    if (row.readiness === 'CONTENT_BLOCKED') {
      const first = row.checks.find(c => c.severity === 'BLOCKER') ?? row.checks.find(c => c.severity === 'WARNING');
      out.push({
        key: `dsp-${row.dsp}`, tone: 'bad', title: `${dspLabel(row.dsp)} 배급이 막혀 있어요`,
        detail: first ? (first.message ?? first.detail ?? checkLabel(first.code)) : row.route_reason ?? undefined,
        fix: first && isKnownCorrection(first.code) ? { code: first.code, note: correctionTarget(first.code).hint, system: true } : undefined,
      });
    }
  }

  // YouTube Content ID — 독점 권리·원본 녹음 확인
  const platforms = d?.platforms ?? sheet.application.platforms;
  if (wantsContentId(platforms)) {
    const missingAck = CONTENT_ID_ACKS.filter(([k]) => o?.[k] !== true);
    if (missingAck.length) {
      out.push({ key: 'cid', tone: 'bad', title: 'Content ID 권리 확인이 빠졌어요', detail: missingAck.map(([, l]) => l).join(' · '), fix: { code: 'FIX_CONTENT_ID', note: 'YouTube Content ID의 독점 권리와 원본 녹음 확인을 체크하거나 Content ID 선택을 해제해 주세요.' } });
    } else {
      out.push({ key: 'cid', tone: 'ok', title: 'Content ID 권리 확인 완료' });
    }
  }

  if (d) {
    // 권리 확인 체크
    const missing = rightsChecksFor(o).filter(([k]) => !d.rightsChecks?.[k]);
    for (const [k, label] of missing) {
      out.push({ key: `rc-${k}`, tone: 'bad', title: '권리 확인을 체크하지 않았어요', detail: label, fix: { code: 'FIX_SPECIAL', note: `권리 확인 ‘${label}’을 확인하고 체크해 주세요.` } });
    }
    // 해당 항목 — 첨부·내용 누락
    if (o?.cover && !o.coverLicenseFile) out.push({ key: 'opt-cover', tone: 'warn', title: '커버곡인데 원곡 이용 허락서가 없어요', fix: { code: 'FIX_SPECIAL', note: '커버곡 원곡 이용 허락서를 첨부해 주세요.' } });
    if (o?.cover && !(o.coverTracks ?? []).some(t => t.originalTitle?.trim())) out.push({ key: 'opt-cover-info', tone: 'warn', title: '커버곡 원곡 정보가 비어 있어요', fix: { code: 'FIX_SPECIAL', note: '커버한 원곡의 제목·아티스트·작사작곡가를 입력해 주세요.' } });
    if (o?.sample && !o.sampleLicenseFile) out.push({ key: 'opt-sample', tone: 'warn', title: '샘플링 원본 이용 허락서가 없어요', fix: { code: 'FIX_SPECIAL', note: '샘플 원본의 이용 허락서를 첨부해 주세요.' } });
    if (o?.featured && !o.featuredConsentFile) out.push({ key: 'opt-feat', tone: 'warn', title: '피처링 참여자 동의서가 없어요', fix: { code: 'FIX_SPECIAL', note: '피처링 참여자의 동의서를 첨부해 주세요.' } });
    if (o?.shared && !o.sharedContractFile) out.push({ key: 'opt-shared', tone: 'warn', title: '공동 권리 계약서가 없어요', fix: { code: 'FIX_SPECIAL', note: '공동 권리자와의 배급 위임 계약서를 첨부해 주세요.' } });
    if (o?.ai && !(o.aiUses?.length || o.aiTool?.trim() || o.aiUseOther?.trim())) out.push({ key: 'opt-ai', tone: 'warn', title: 'AI 활용 내역이 비어 있어요', fix: { code: 'FIX_SPECIAL', note: 'AI를 어디에 어떻게 활용했는지 알려 주세요.' } });
    if (o?.minor && !o.guardianConsentDone) out.push({ key: 'opt-minor', tone: 'bad', title: '법정대리인 동의 절차가 끝나지 않았어요' });
    if (o?.express) out.push({ key: 'opt-express', tone: 'warn', title: '신속 발매 요청', detail: o.expressReason || '사유 미기재' });

    // 트랙
    const tracks = (d.draftTracks ?? []).filter(t => t.title?.trim());
    tracks.forEach((t, i) => {
      const name = `${i + 1}. ${t.title}`;
      const trackId = t.serverId || undefined;
      if (!t.audioName) out.push({ key: `tr-a-${i}`, tone: 'bad', title: `${name} · 음원 파일이 없어요`, fix: { code: 'FIX_AUDIO', note: '음원 파일을 올려 주세요.', trackId } });
      if (!t.composers?.trim()) out.push({ key: `tr-c-${i}`, tone: 'bad', title: `${name} · 작곡가가 비어 있어요`, fix: { code: 'FIX_COMPOSERS', note: '작곡가 실명 또는 활동명을 입력해 주세요.', trackId } });
      if (!t.instrumental && !t.lyricists?.trim()) out.push({ key: `tr-l-${i}`, tone: 'warn', title: `${name} · 작사가가 비어 있어요`, fix: { code: 'FIX_LYRICISTS', note: '작사가 실명 또는 활동명을 입력해 주세요. 연주곡이면 ‘가사 없는 연주곡이에요’를 체크해 주세요.', trackId } });
      if (!t.instrumental && !t.lyrics?.trim()) out.push({ key: `tr-y-${i}`, tone: 'warn', title: `${name} · 가사가 없어요`, detail: '국내 플랫폼 가사 노출이 안 돼요.' });
    });
    const explicit = tracks.filter(t => t.explicit).length;
    if (explicit) out.push({ key: 'explicit', tone: 'warn', title: `19세 이상 표시 ${explicit}곡`, detail: '가사·표기가 맞는지 확인해 주세요.' });
  }

  // 발매일까지
  const left = daysUntil(d?.release_date ?? sheet.application.release_date);
  if (left != null) {
    const need = o?.express ? 3 : 14;
    if (left < need) out.push({ key: 'date', tone: 'bad', title: `발매일까지 ${left < 0 ? '이미 지났어요' : `${left}일`} (준비 기간 ${need}일 필요)`, fix: { code: 'FIX_RELEASE_DATE', note: `발매일까지 준비 기간이 부족해요 (${o?.express ? '신속 3일' : '일반 14일'} 이상).` } });
    else out.push({ key: 'date', tone: 'ok', title: `발매일까지 ${left}일` });
  }

  // 서류
  const pendingDocs = sheet.documents.filter(x => !['APPROVED', 'SIGNED'].includes(x.status));
  if (pendingDocs.length) out.push({ key: 'docs', tone: 'warn', title: `처리 전 서류 ${pendingDocs.length}건`, detail: pendingDocs.map(x => x.title).join(', ') });

  // 해당 항목 요약 (문제 없음 포함)
  const picked = o ? Object.keys(OPTION_LABELS).filter(k => (o as Record<string, unknown>)[k] === true) : [];
  if (!picked.length) out.push({ key: 'opts', tone: 'ok', title: '해당 항목 없음 (일반 발매)' });

  const rank: Record<BriefTone, number> = { bad: 0, warn: 1, ok: 2 };
  return out.sort((a, b) => rank[a.tone] - rank[b.tone]);
}

const ICON: Record<BriefTone, ReactNode> = {
  ok: <CheckIcon size={11} />,
  warn: <Glyph name="alert" size={12} />,
  bad: <Glyph name="close" size={11} />,
};

export function ReviewBrief({ sheet, integrity, title = '결정 전 확인할 것', onAddFix, added = [] }: {
  sheet: ReleaseSheet; integrity: Integrity; title?: string;
  /** 보완 요청 창: 문제를 보완 항목으로 추가 */
  onAddFix?: (fix: BriefFix) => void;
  added?: string[];
}) {
  const items = reviewBrief(sheet, integrity);
  const bad = items.filter(i => i.tone === 'bad').length;
  const warn = items.filter(i => i.tone === 'warn').length;
  return (
    <section className="adm-brief" aria-label={title}>
      <div className="adm-brief-top">
        <b>{title}</b>
        <small>{bad ? `문제 ${bad}` : '문제 없음'}{warn ? ` · 주의 ${warn}` : ''}</small>
      </div>
      <ul>
        {items.map(i => {
          const fixKey = i.fix ? `${i.fix.code}@${i.fix.trackId ?? ''}` : '';
          const isAdded = !!fixKey && added.includes(fixKey);
          return (
            <li key={i.key} className={`is-${i.tone}`}>
              <span className="adm-brief-mark" aria-hidden="true">{ICON[i.tone]}</span>
              <span className="adm-brief-text">
                <b>{i.title}</b>
                {i.detail && <small>{i.detail}</small>}
              </span>
              {onAddFix && i.fix && !i.fix.system && (
                <button type="button" className="adm-btn soft small" disabled={isAdded} onClick={() => onAddFix(i.fix!)}>{isAdded ? '추가됨' : '항목으로 추가'}</button>
              )}
            </li>
          );
        })}
      </ul>
    </section>
  );
}

// ---------------------------------------------------------------------------
// 보완 요청 창 — 고른 칸에 아티스트가 지금 입력한 내용
// ---------------------------------------------------------------------------
export function CurrentValue({ sheet, code, trackId }: { sheet: ReleaseSheet; code: string; trackId?: string }) {
  const d = sheet.draft;
  const app = sheet.application;
  const server = app.tracks ?? [];
  const drafts = (d?.draftTracks ?? []).filter(t => t.title?.trim());
  const si = trackId ? server.findIndex(t => t.id === trackId) : -1;
  const t = trackId ? drafts.find(x => x.serverId === trackId) ?? (si >= 0 ? drafts[si] : undefined) : undefined;
  const st = si >= 0 ? server[si] : undefined;
  const txt = (v?: string | null) => (v && v.trim() ? v : '(비어 있음)');
  let body: ReactNode = null;
  switch (code) {
    case 'FIX_TITLE': body = txt(sheet.release.title); break;
    case 'FIX_ARTIST': body = txt(d?.artist ?? app.artist); break;
    case 'FIX_TYPE': body = d ? txt(kindLabel(d.type)) : null; break;
    case 'FIX_GENRE': body = d ? txt(genreLabel(d)) : txt(app.genre); break;
    case 'FIX_LANGUAGE': body = d ? txt(languageLabel(d.language)) : txt(app.language); break;
    case 'FIX_LABEL': body = txt(d?.label ?? app.label); break;
    case 'FIX_NOTES': body = d ? <pre>{txt(draftNotes(d))}</pre> : null; break;
    case 'FIX_COVER': {
      const src = d?.coverData || sheet.release.cover;
      body = src ? <img className="adm-now-cover" src={src} alt="현재 커버" /> : '(커버 미리보기 없음)';
      break;
    }
    case 'FIX_RELEASE_DATE': body = txt(d?.release_date ?? app.release_date); break;
    case 'FIX_UPC': body = txt(d?.upc || sheet.release.upc || ''); break;
    case 'FIX_PLATFORMS': body = (d?.platforms ?? app.platforms).map(dspLabel).join(', ') || '(비어 있음)'; break;
    case 'FIX_SPECIAL': {
      const o = (d?.options ?? app.options ?? {}) as Record<string, unknown>;
      body = Object.keys(OPTION_LABELS).filter(k => o[k] === true).map(k => OPTION_LABELS[k]).join(', ') || '해당 항목 없음';
      break;
    }
    case 'FIX_CONTENT_ID': {
      const o = (d?.options ?? app.options ?? {}) as StudioOptions;
      body = wantsContentId(d?.platforms ?? app.platforms)
        ? CONTENT_ID_ACKS.map(([k, l]) => `${o[k] === true ? '확인함' : '체크 안 함'} · ${l}`).join('\n')
        : 'Content ID를 신청하지 않았어요';
      body = <pre>{body}</pre>;
      break;
    }
    case 'FIX_OWNERSHIP': body = txt(d?.ownership); break;
    case 'FIX_PLINE': body = `℗ ${txt(d?.phonogram ?? app.p_line)}`; break;
    case 'FIX_CLINE': body = `© ${txt(d?.copyright ?? app.c_line)}`; break;
    default: {
      if (!trackId) return null;
      if (!t && !st) return null;
      switch (code) {
        case 'FIX_TRACK_TITLE': body = txt(t?.title ?? st?.title); break;
        case 'FIX_TRACK_VERSION': body = txt(t?.version ?? st?.version); break;
        case 'FIX_AUDIO': body = [t?.audioName, t?.audioSpec, t?.duration].filter(Boolean).join(' · ') || (st?.asset_kind ?? '(파일 없음)'); break;
        case 'FIX_COMPOSERS': body = txt(t?.composers); break;
        case 'FIX_LYRICISTS': body = t?.instrumental ? '연주곡으로 표시됨' : txt(t?.lyricists); break;
        case 'FIX_ARRANGERS': body = txt(t?.arrangers); break;
        case 'FIX_PERFORMERS': body = txt([t?.performers, t?.featuring && `피처링 ${t.featuring}`].filter(Boolean).join(' · ')); break;
        case 'FIX_LYRICS': body = t?.instrumental ? '연주곡으로 표시됨' : <pre>{txt(t?.lyrics)}</pre>; break;
        case 'FIX_ISRC': body = txt(st?.isrc || t?.isrc || ''); break;
        default: return null;
      }
    }
  }
  if (body == null) return null;
  return (
    <div className="adm-now">
      <span className="adm-fix-label">지금 입력된 내용</span>
      <div className="adm-now-body">{body}</div>
    </div>
  );
}
