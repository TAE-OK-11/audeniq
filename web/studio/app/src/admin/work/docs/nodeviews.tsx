// 문서 블록의 화면 — 서버 값은 모두 React 글자로만 그린다(HTML로 넣지 않음). 실시간 값은 EmbedStore가
// 같은 화면의 블록들을 한 요청으로 묶어 불러온다.
import { useContext, type ReactNode } from 'react';
import { DocsContext, type DocsCtx } from './context';
import { NodeViewContent, NodeViewWrapper, type ReactNodeViewProps } from '@tiptap/react';
import { Link } from '../../../lib/router';
import { Glyph } from '../../../components/Glyph';
import { when } from '../../labels';
import { ENTITY_KIND, PRIORITY_CODE, PRIORITY_NAME, TASK_STATUS, DOC_STATUS, entityLink } from '../labels';
import { sizeLabel, type People } from '../ui';
import type { EntityKind, Priority, TaskStatus } from '../types';
import { useEmbed } from './embeds';
import { embedKey, isSafeUrl } from './model';

function useDocs(): DocsCtx {
  const c = useContext(DocsContext);
  if (!c) throw new Error('DocsContext missing');
  return c;
}

const s = (v: unknown): string => (v == null ? '' : typeof v === 'string' ? v : typeof v === 'number' || typeof v === 'boolean' ? String(v) : '');
const n = (v: unknown): number | null => (typeof v === 'number' && Number.isFinite(v) ? v : null);
const arr = (v: unknown): unknown[] => (Array.isArray(v) ? v : []);
const rec = (v: unknown): Record<string, unknown> => (v && typeof v === 'object' && !Array.isArray(v) ? (v as Record<string, unknown>) : {});
const dt = (v: unknown) => (typeof v === 'string' && v ? when(v) : '—');

export function CalloutView({ node, updateAttributes, editor }: ReactNodeViewProps) {
  const tone = s(node.attrs.tone) || 'info';
  const icon = tone === 'warning' ? 'warning' : tone === 'danger' ? 'stop' : tone === 'success' ? 'check' : 'info';
  return (
    <NodeViewWrapper className={`adm-doc-callout is-${tone}`} data-type="callout">
      <span className="adm-doc-callout-icon" contentEditable={false} aria-hidden="true"><Glyph name={icon} size={17} /></span>
      {editor.isEditable && (
        <select className="adm-doc-callout-tone" contentEditable={false} aria-label="강조 상자 종류" value={tone}
          onChange={e => updateAttributes({ tone: e.target.value })}>
          <option value="info">안내</option><option value="warning">주의</option><option value="danger">위험</option><option value="success">완료</option>
        </select>
      )}
      <NodeViewContent className="adm-doc-callout-body" />
    </NodeViewWrapper>
  );
}

export function ImageView({ node, selected, updateAttributes, editor }: ReactNodeViewProps) {
  const { inlineUrl } = useDocs();
  const id = s(node.attrs.attachmentId);
  const src = id ? inlineUrl(id) : '';
  const alt = s(node.attrs.alt);
  return (
    <NodeViewWrapper className={`adm-doc-figure${selected ? ' is-selected' : ''}`} data-type="doc-image">
      {src ? <img src={src} alt={alt} loading="lazy" style={n(node.attrs.width) ? { width: `${n(node.attrs.width)}px` } : undefined} />
        : <div className="adm-doc-figure-empty"><Glyph name="clip" size={16} />이미지 (체험 모드에서는 미리보기가 없어요)</div>}
      {editor.isEditable ? (
        <input className="adm-doc-caption" aria-label="이미지 설명(대체 텍스트)" placeholder="이미지 설명" maxLength={300} value={alt}
          onChange={e => updateAttributes({ alt: e.target.value })} />
      ) : alt && <figcaption>{alt}</figcaption>}
    </NodeViewWrapper>
  );
}

export function FileView({ node, selected }: ReactNodeViewProps) {
  const { fileUrl } = useDocs();
  const id = s(node.attrs.attachmentId);
  const href = id ? fileUrl(id) : '';
  const name = s(node.attrs.name) || '파일';
  const body = <><Glyph name="clip" size={15} /><b>{name}</b>{n(node.attrs.size) != null && <small>{sizeLabel(n(node.attrs.size) ?? 0)}</small>}<Glyph name="download" size={14} /></>;
  return (
    <NodeViewWrapper className={`adm-doc-file${selected ? ' is-selected' : ''}`} data-type="file-attachment">
      {href ? <a href={href} download contentEditable={false}>{body}</a> : <span>{body}</span>}
    </NodeViewWrapper>
  );
}

export function LinkPreviewView({ node, selected }: ReactNodeViewProps) {
  const url = s(node.attrs.url);
  const safe = isSafeUrl(url);
  let host = '';
  try { host = new URL(url).host; } catch { host = url; }
  return (
    <NodeViewWrapper className={`adm-doc-linkcard${selected ? ' is-selected' : ''}`} data-type="link-preview">
      <a href={safe ? url : undefined} target="_blank" rel="noopener noreferrer nofollow" contentEditable={false}>
        <b>{s(node.attrs.title) || host}</b>
        {s(node.attrs.description) && <span>{s(node.attrs.description)}</span>}
        <small><Glyph name="arrow-up-right" size={12} />{host}</small>
      </a>
    </NodeViewWrapper>
  );
}

// ---------- AUDENIQ 블록 ----------
function Row({ label, children }: { label: string; children: ReactNode }) {
  return <div className="adm-doc-embed-row"><dt>{label}</dt><dd>{children}</dd></div>;
}
function Pri({ v }: { v: unknown }) {
  const p = s(v) as Priority;
  return PRIORITY_CODE[p] ? <span className={`adm-doc-pri is-${p.toLowerCase()}`}>{PRIORITY_CODE[p]} {PRIORITY_NAME[p]}</span> : <>—</>;
}
const statusLabel = (v: unknown) => TASK_STATUS[s(v) as TaskStatus]?.[0] ?? s(v);

function Body({ kind, view, d, people }: { kind: string; view: string; d: Record<string, unknown>; people: People }) {
  const person = (id: unknown) => (s(id) ? people.get(s(id))?.name ?? '담당자' : '없음');
  if (kind === 'release') {
    const base = (
      <dl className="adm-doc-embed-grid">
        <Row label="아티스트">{s(d.artist) || '—'}</Row><Row label="UPC">{s(d.upc) || '—'}</Row>
        <Row label="발매일">{s(d.release_date) || '—'}</Row><Row label="상태">{s(d.status)}</Row><Row label="작업 공간">{s(d.org_name)}</Row>
      </dl>
    );
    if (view === 'qc') {
      const qc = rec(d.qc);
      const checks = rec(qc.checks);
      const loud = rec(qc.loudness);
      const audio = rec(qc.audio);
      const issues = arr(qc.issues).map(rec);
      const fmt = (v: unknown, unit: string) => (n(v) == null ? '—' : `${(n(v) ?? 0).toFixed(1)} ${unit}`);
      return (
        <>
          <dl className="adm-doc-embed-grid">
            <Row label="검사">{n(checks.total) == null ? '아직 없음' : `${s(checks.passed)}/${s(checks.total)} 통과${n(checks.blocked) ? ` · 차단 ${s(checks.blocked)}` : ''}`}</Row>
            <Row label="라우드니스">{n(loud.min_lufs) == null ? '—' : loud.min_lufs === loud.max_lufs ? fmt(loud.min_lufs, 'LUFS') : `${fmt(loud.min_lufs, '')}~${fmt(loud.max_lufs, 'LUFS')}`}</Row>
            <Row label="트루 피크">{fmt(loud.max_true_peak, 'dBTP')}</Row>
            <Row label="샘플레이트">{arr(audio.sample_rates).map(v => `${(Number(v) / 1000).toFixed(1)}kHz`).join(', ') || '—'}</Row>
            <Row label="채널">{arr(audio.channels).map(v => (v === 1 ? '모노' : v === 2 ? '스테레오' : `${s(v)}ch`)).join(', ') || '—'}</Row>
          </dl>
          {issues.length > 0 && (
            <ul className="adm-doc-embed-issues" aria-label="열린 검사 결과">
              {issues.slice(0, 6).map((i, k) => <li key={k}><b>{s(i.code)}</b> <span className="adm-doc-tag">{s(i.status)}</span> {s(i.detail)}</li>)}
              {issues.length > 6 && <li className="muted">외 {issues.length - 6}건</li>}
            </ul>
          )}
        </>
      );
    }
    if (view === 'rights') {
      const r = rec(d.rights);
      return (
        <dl className="adm-doc-embed-grid">
          <Row label="배급 계약서">{s(r.agreement) || '없음'}</Row><Row label="2차 승인 대기">{s(r.pending_approvals) || '0'}건</Row>
          <Row label="열린 검사">{s(r.open_checks) || '0'}건</Row><Row label="심사 담당">{r.claimed_by ? person(r.claimed_by) : '없음'}</Row>
          <Row label="권리 회차">{s(r.rights_epoch)}</Row>
        </dl>
      );
    }
    if (view === 'distribution') {
      const rows = arr(d.distribution).map(rec);
      return rows.length === 0 ? <p className="muted small">아직 배급 준비된 DSP가 없어요.</p> : (
        <table className="adm-doc-embed-table">
          <thead><tr><th scope="col">DSP</th><th scope="col">전송</th><th scope="col">전송 시각</th><th scope="col">라이브</th><th scope="col">오류</th></tr></thead>
          <tbody>{rows.map((r, k) => (
            <tr key={k}><td>{s(r.name)}</td><td>{s(r.delivery) || s(r.readiness)}</td><td>{dt(r.delivered_at)}</td><td>{s(r.live) || '—'}</td><td className="adm-doc-err">{s(r.error) || '—'}</td></tr>
          ))}</tbody>
        </table>
      );
    }
    return base;
  }
  if (kind === 'track') return (
    <dl className="adm-doc-embed-grid">
      <Row label="ISRC">{s(d.isrc) || '—'}</Row><Row label="아티스트">{s(d.artist)}</Row><Row label="발매">{s(d.release_title)}</Row>
      <Row label="오디오">{n(d.sample_rate) ? `${((n(d.sample_rate) ?? 0) / 1000).toFixed(1)}kHz · ${s(d.channels)}ch` : '—'}</Row><Row label="QC">{s(d.qc_status) || '—'}</Row>
    </dl>
  );
  if (kind === 'artist') return <dl className="adm-doc-embed-grid"><Row label="작업 공간">{s(d.org_name)}</Row><Row label="발매">{s(d.releases)}건</Row></dl>;
  if (kind === 'dsp') return (
    <dl className="adm-doc-embed-grid">
      <Row label="상태">{s(d.status)} · {s(d.region)}</Row><Row label="30일 전송">{s(d.delivered_30d)}건 성공 · {s(d.failed_30d)}건 실패</Row>
      <Row label="진행 중">{s(d.in_flight)}건</Row><Row label="마지막 전송">{dt(d.last_delivered_at)}</Row>
      <Row label="마지막 오류"><span className="adm-doc-err">{s(d.last_error) || '없음'}</span></Row>
    </dl>
  );
  if (kind === 'inquiry') {
    const t = rec(d.task);
    return (
      <dl className="adm-doc-embed-grid">
        <Row label="상태">{s(d.status)}</Row><Row label="분류">{s(d.category)}</Row><Row label="작업 공간">{s(d.org_name)}</Row>
        <Row label="중요도">{t.priority ? <Pri v={t.priority} /> : '—'}</Row><Row label="담당">{t.assignee_id ? person(t.assignee_id) : '없음'}</Row>
      </dl>
    );
  }
  if (kind === 'task') return (
    <dl className="adm-doc-embed-grid">
      <Row label="우선순위"><Pri v={d.priority} /></Row><Row label="열">{s(d.column) || statusLabel(d.status)}</Row><Row label="보드">{s(d.board) || '—'}</Row>
      <Row label="담당">{person(d.assignee_id)}</Row><Row label="마감">{dt(d.due_at)}</Row>
    </dl>
  );
  if (kind === 'approval') return (
    <dl className="adm-doc-embed-grid">
      <Row label="상태">{s(d.status)}</Row><Row label="발매">{s(d.release_title)}</Row><Row label="항목">{arr(d.check_codes).map(s).join(', ')}</Row>
      <Row label="요청">{person(d.requested_by)}</Row><Row label="만료">{dt(d.expires_at)}</Row>
    </dl>
  );
  if (kind === 'doc') return <dl className="adm-doc-embed-grid"><Row label="상태">{DOC_STATUS[s(d.status)]?.[0] ?? s(d.status)}</Row><Row label="고친 때">{dt(d.updated_at)}</Row></dl>;
  if (kind === 'audit') {
    const ev = arr(d.events).map(rec);
    return (
      <ol className="adm-doc-embed-audit">
        {ev.map((e, k) => <li key={k}><time>{dt(e.at)}</time><b>{s(e.action)}</b><span>{s(e.reason)}</span><small>{e.actor_id ? person(e.actor_id) : s(e.service)}</small></li>)}
        {n(d.total) != null && (n(d.total) ?? 0) > ev.length && <li className="muted">전체 {s(d.total)}건 중 최근 {ev.length}건</li>}
      </ol>
    );
  }
  return <p className="small muted">{s(d.sub)}</p>;
}

const VIEW_TITLE: Record<string, string> = { qc: 'QC 결과', rights: '권리 검토', distribution: '배급 현황' };

export function EmbedView({ node, selected }: ReactNodeViewProps) {
  const { embeds, people } = useDocs();
  const kind = s(node.attrs.kind);
  const id = s(node.attrs.id);
  const view = s(node.attrs.view) || 'summary';
  const key = kind && id ? embedKey(kind, id, view) : null;
  const { data, error, loading, refresh } = useEmbed(embeds, key);
  const d = rec(data);
  const title = s(d.title) || s(d.name) || s(d.subject) || s(d.label) || s(node.attrs.label) || id;
  const known = kind in ENTITY_KIND;
  const href = kind === 'audit' ? entityLink('release', id) : known ? entityLink(kind as EntityKind, id) : null;
  const label = kind === 'audit' ? '감사 기록' : known ? ENTITY_KIND[kind as EntityKind].label : kind;
  return (
    <NodeViewWrapper className={`adm-doc-embed is-${kind}${selected ? ' is-selected' : ''}`} data-type="entity-embed" contentEditable={false}>
      <header>
        <span className="adm-doc-embed-kind">{label}{VIEW_TITLE[view] ? ` · ${VIEW_TITLE[view]}` : ''}</span>
        {kind === 'task' && s(d.seq) && <span className="ui-tag is-mono">WRK-{s(d.seq)}</span>}
        {href && !d.missing ? <Link to={href} className="adm-doc-embed-title">{title}</Link> : <b className="adm-doc-embed-title">{title}</b>}
        <button type="button" className="ui-btn is-text is-muted" onClick={refresh} aria-label={`${title} 새로고침`}>새로고침</button>
      </header>
      {error ? <p className="adm-doc-embed-note is-error">불러오지 못했어요. <button type="button" className="ui-btn is-text is-muted" onClick={refresh}>다시 시도</button></p>
        : loading && !data ? <div className="ui-skel-rows"><span /></div>
          : d.restricted ? <p className="adm-doc-embed-note">감사 기록은 심사 권한이 있어야 볼 수 있어요.</p>
            : d.missing ? <p className="adm-doc-embed-note">찾을 수 없거나 볼 권한이 없어요. (저장된 이름: {s(node.attrs.label) || '없음'})</p>
              : <Body kind={kind} view={view} d={d} people={people} />}
    </NodeViewWrapper>
  );
}
