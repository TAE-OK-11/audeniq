// 저작권 관리 화면 조각 — 등록 상태 배지·진행, 지분 막대, 곡 줄. 곡 상세·라이브러리·곡 상세·홈이 같이 쓴다.
// 새 모양을 만들지 않고 발매 화면과 같은 부품을 쓴다: 진행은 StageCard(발매 상세), 이력은 ui-timeline(발매 '기록' 탭),
// 지분 막대는 Meter(홈 플랫폼 반영), 줄은 ui-row + IconTile(홈 피칭 카드)
import { useState } from 'react';
import { Link } from '../../lib/router';
import type { RegistrationStatus, RegistrationStep, Work, WorkContributor } from '../../api/publishing';
import { APP_LABEL, APP_TONE, REG_LABEL, REG_SENTENCE, REG_STEPS, REG_TONE, ROLE_LABEL, pct, regStep, sumShares } from '../../lib/publishing';
import { localStamp } from '../../lib/format';
import { Badge, IconTile, Meter } from '../../ui';
import { StageCard } from '../StageCard';
import { Glyph } from '../Glyph';
import { Reveal } from '../../ui/Reveal';

export function RegBadge({ status }: { status: RegistrationStatus }) {
  return <Badge tone={REG_TONE[status]}>{REG_LABEL[status]}</Badge>;
}

const TIMELINE_TONE: Partial<Record<RegistrationStatus, string>> = { needs_fix: 'is-warn', rejected: 'is-bad' };

/** 등록 진행 — 발매 진행 단계와 같은 카드. 처리 이력은 눌러서 펼친다 (기획안 7.2) */
export function RegProgress({ status, log }: { status: RegistrationStatus; log: RegistrationStep[] }) {
  const [open, setOpen] = useState(false);
  const step = regStep(status);
  const warn = status === 'needs_fix';
  return (
    <>
      {step < 0 ? (
        <p className="ui-row-meta">{REG_SENTENCE[status]}</p>
      ) : (
        <StageCard
          steps={REG_STEPS.map((s, i) => (i === step && (warn || status === 'rejected') ? REG_LABEL[status] : s))}
          current={Math.min(step, REG_STEPS.length - 1)} complete={status === 'registered'}
          tone={warn ? 'warn' : status === 'rejected' ? 'error' : undefined}
          note={REG_SENTENCE[status]} label="저작권 등록 진행 단계"
        />
      )}
      {log.length > 0 && (
        <>
          <button type="button" className="ui-btn is-text is-sm" aria-expanded={open} onClick={() => setOpen(v => !v)}>
            처리 이력 {log.length}건<Glyph name="chevron-down" size={14} />
          </button>
          <Reveal open={open}>
            <ol className="ui-timeline is-latest-first">
              {[...log].reverse().map((l, i) => (
                <li key={i} className={TIMELINE_TONE[l.status]}>
                  <strong>{REG_LABEL[l.status]}{l.society && ` · ${l.society}`}{l.note && ` — ${l.note}`}</strong>
                  <small>{localStamp(l.at)}</small>
                </li>
              ))}
            </ol>
          </Reveal>
        </>
      )}
    </>
  );
}

const TONES = ['primary', 'soft', 'green', 'amber'] as const;

/** 지분 막대 — 홈 '플랫폼 반영'과 같은 Meter. 참여자마다 한 칸 */
export function SplitBar({ list }: { list: readonly WorkContributor[]; size?: 'sm' | 'md' }) {
  const total = sumShares(list);
  return (
    <Meter
      total={Math.max(100, total)} parts={list.map((c, i) => ({ value: c.share, tone: TONES[i % TONES.length] }))}
      label={`지분 ${list.map(c => `${c.name || '이름 없음'} ${pct(c.share)}`).join(', ')} · 합계 ${pct(total)}`}
    />
  );
}

export function roleText(c: Pick<WorkContributor, 'roles'>) {
  return c.roles.map(r => ROLE_LABEL[r]).join('·') || '역할 없음';
}

/** 참여자 이름 목록 — '서린 · 김하늘 외 1명' */
export function peopleLine(list: readonly WorkContributor[], max = 2) {
  const names = list.map(c => c.name).filter(Boolean);
  return names.length > max ? `${names.slice(0, max).join(' · ')} 외 ${names.length - max}명` : names.join(' · ') || '참여자 없음';
}

/** 라이브러리·연결 목록의 곡 한 줄 — 발매 목록 줄과 같은 ui-row */
export function WorkRow({ work: w, to, onClick, end }: { work: Work; to?: string; onClick?: () => void; end?: React.ReactNode }) {
  const body = (
    <>
      <IconTile tone={w.recordings.length ? 'blue' : 'gray'}><Glyph name={w.recordings.length ? 'music' : 'pencil'} size={20} /></IconTile>
      <span className="min-0">
        <span className="ui-row-title">{w.title || '제목 없는 곡'}</span>
        <span className="ui-row-meta">
          <span>{peopleLine(w.contributors)}</span>
          <span>{w.recordings.length ? `녹음 ${w.recordings.length}개` : '미공개 곡'}</span>
          <span>{w.iswc ?? w.internalCode}</span>
        </span>
      </span>
      <span className="ui-row-end">
        {end ?? (
          <>
            {w.application && w.application.status !== 'approved' && w.application.status !== 'withdrawn'
              ? <Badge tone={APP_TONE[w.application.status]}>{APP_LABEL[w.application.status]}</Badge>
              : <RegBadge status={w.registration} />}
            {w.myRole === 'contributor' && <small>공동 참여</small>}
          </>
        )}
      </span>
    </>
  );
  if (to) return <Link to={to} className="ui-row">{body}</Link>;
  return <button type="button" className="ui-row" onClick={onClick}>{body}</button>;
}
