// 레이블 대량 작업 (/releases/bulk) — CSV(엑셀) 한 장으로 여러 발매를 임시 저장으로 만들거나(대량 등록),
// 신청 전 발매들을 내보내 고친 뒤 다시 올려 한 번에 고친다(일괄 수정). 오류가 있는 줄은 표에서 바로 고치고
// 다시 검사한다. 오류 없는 발매만 저장하고, 음원·커버·권리 확인·서명은 발매마다 위자드에서 마저 한다.
import { useMemo, useRef, useState } from 'react';
import { Link, useSearchParams } from '../lib/router';
import { api } from '../api/client';
import { PartialSaveError, errorMessage } from '../api/errors';
import { useToast } from '../components/Toast';
import {
  COLUMNS, TEMPLATE_ROWS, applyEdit, createPayload, groups, payloadFromDetail, readRows, rowsFromDetail, toCsv, validate,
  type Row, type RowIssue,
} from '../lib/bulk';

type Mode = 'create' | 'edit';
interface Result { key: string; title: string; ok: boolean; id?: string; message?: string }

function saveFile(name: string, text: string) {
  const url = URL.createObjectURL(new Blob([text], { type: 'text/csv;charset=utf-8' }));
  const a = document.createElement('a');
  a.href = url; a.download = name; a.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

export function BulkReleases() {
  const toast = useToast();
  const [params, setParams] = useSearchParams();
  const mode: Mode = params.get('tab') === 'edit' ? 'edit' : 'create';
  const [rows, setRows] = useState<Row[]>([]);
  const [headIssues, setHeadIssues] = useState<RowIssue[]>([]);
  const [onlyErrors, setOnlyErrors] = useState(false);
  const [busy, setBusy] = useState('');
  const [results, setResults] = useState<Result[]>([]);
  const fileRef = useRef<HTMLInputElement>(null);
  // 대량 등록 중 저장이 중간에 끊긴 발매 — 다시 저장할 때 새로 만들지 않고 이어서 쓴다 (발매묶음 → 발매 ID)
  const partial = useRef(new Map<string, string>());

  const issues = useMemo(() => validate(rows, mode), [rows, mode]);
  const byCell = useMemo(() => {
    const m = new Map<string, string>();
    for (const i of issues) m.set(`${i.row}:${i.column}`, i.message);
    return m;
  }, [issues]);
  const badRows = useMemo(() => new Set(issues.map(i => i.row)), [issues]);
  const all = useMemo(() => groups(rows), [rows]);
  const ready = all.filter(g => g.rows.every(r => !badRows.has(r.line)));
  const shownCols = COLUMNS.filter(c => mode === 'edit' ? c.key !== 'release_key' : c.key !== 'release_id');

  const switchMode = (m: Mode) => { partial.current.clear(); setRows([]); setResults([]); setHeadIssues([]); const n = new URLSearchParams(params); n.set('tab', m); setParams(n); };
  const load = async (file: File) => {
    if (file.size > 2 * 1024 * 1024) { toast('2MB까지 올릴 수 있어요.', 'error'); return; }
    const { rows: r, issues: h } = readRows(await file.text());
    setRows(r); setHeadIssues(h); setResults([]);
    if (!h.length) toast(`${r.length}줄을 읽었어요.`, 'success');
  };
  const edit = (line: number, key: string, v: string) => setRows(rs => rs.map((r, i) => (i === line - 1 ? { ...r, [key]: v } : r)));

  const exportDrafts = async () => {
    setBusy('내보내는 중');
    try {
      const drafts = (await api.listReleases()).filter(r => r.status === 'draft');
      const out: Row[] = [];
      for (const d of drafts) out.push(...rowsFromDetail(await api.getRelease(d.id)));
      if (!out.length) { toast('신청 전(임시 저장) 발매가 없어요.', 'info'); return; }
      saveFile('audeniq-drafts.csv', toCsv(out));
    } catch (e) { toast(errorMessage(e), 'error'); } finally { setBusy(''); }
  };

  const run = async () => {
    const done: Result[] = [];
    for (const [n, g] of ready.entries()) {
      const first = g.rows[0].row;
      setBusy(`${n + 1} / ${ready.length} 저장 중`);
      try {
        const resumeId = mode === 'create' ? partial.current.get(g.key) : undefined;
        if (resumeId) {
          const d = await api.getRelease(resumeId);
          const r = await api.saveDraft(d.id, applyEdit(payloadFromDetail(d), g));
          partial.current.delete(g.key);
          done.push({ key: g.key, title: first.release_title, ok: true, id: r.id });
        } else if (mode === 'create') {
          const r = await api.saveDraft(null, createPayload(g));
          done.push({ key: g.key, title: first.release_title, ok: true, id: r.id });
        } else {
          const d = await api.getRelease(first.release_id);
          if (d.status !== 'draft') throw new Error('신청한 발매는 여기서 고칠 수 없어요. 발매 상세의 정보 수정 요청을 써 주세요.');
          const r = await api.saveDraft(d.id, applyEdit(payloadFromDetail(d), g));
          done.push({ key: g.key, title: first.release_title, ok: true, id: r.id });
        }
      } catch (e) {
        if (mode === 'create' && e instanceof PartialSaveError && e.releaseId) partial.current.set(g.key, e.releaseId);
        done.push({ key: g.key, title: first.release_title, ok: false, message: errorMessage(e) });
      }
      setResults([...done]);
    }
    setBusy('');
    const ok = done.filter(d => d.ok).length;
    toast(`${ok}개 발매를 ${mode === 'create' ? '만들었어요' : '고쳤어요'}.${done.length - ok ? ` ${done.length - ok}개는 실패했어요.` : ''}`, ok ? 'success' : 'error');
    // 저장된 발매의 줄은 표에서 뺀다 (실패·오류 줄만 남겨 마저 고친다)
    const saved = new Set(done.filter(d => d.ok).map(d => d.key));
    setRows(rs => rs.filter(r => !saved.has(groups([r])[0].key)));
  };

  const visible = rows.map((r, i) => ({ r, line: i + 1 })).filter(x => !onlyErrors || badRows.has(x.line));

  return (
    <div className="view">
      <div className="ui-page-head">
        <div>
          <p className="eyebrow">발매·곡 관리</p>
          <h1>대량 작업</h1>
          <p>여러 발매의 정보를 CSV(엑셀) 한 장으로 등록하거나 한 번에 고쳐요. 음원·커버·서명은 발매마다 마저 해 주세요.</p>
        </div>
        <Link to="/releases" className="ui-btn is-soft">발매 목록</Link>
      </div>
      <div className="ui-chips" role="tablist" aria-label="작업">
        <button type="button" role="tab" aria-selected={mode === 'create'} className={`ui-chip${mode === 'create' ? ' is-on' : ''}`} onClick={() => switchMode('create')}>대량 등록</button>
        <button type="button" role="tab" aria-selected={mode === 'edit'} className={`ui-chip${mode === 'edit' ? ' is-on' : ''}`} onClick={() => switchMode('edit')}>일괄 수정 (신청 전 발매)</button>
      </div>

      <section className="ui-card aq-bulk-start">
        {mode === 'create' ? (
          <p>1. 양식을 받아 한 줄에 곡 하나씩 채워 주세요. 같은 발매의 곡은 <b>발매묶음</b> 값을 같게 적어요.
            <button type="button" className="ui-btn is-soft is-sm" onClick={() => saveFile('audeniq-bulk-template.csv', toCsv(TEMPLATE_ROWS))}>양식 받기</button></p>
        ) : (
          <p>1. 신청 전 발매를 내보내 엑셀에서 고쳐 주세요. <b>발매ID</b>와 <b>트랙번호</b>는 그대로 두고, 새 곡은 다음 번호로 줄을 더해요.
            <button type="button" className="ui-btn is-soft is-sm" disabled={!!busy} onClick={() => void exportDrafts()}>{busy === '내보내는 중' ? busy : '임시 저장 발매 내보내기'}</button></p>
        )}
        <p>2. 고친 파일을 올리면 바로 검사해요.
          <input ref={fileRef} type="file" accept=".csv,text/csv" hidden onChange={e => { const f = e.target.files?.[0]; if (f) void load(f); e.target.value = ''; }} />
          <button type="button" className="ui-btn is-primary is-sm" onClick={() => fileRef.current?.click()}>CSV 올리기</button></p>
        <details className="small muted"><summary>열 설명</summary>
          <ul>{COLUMNS.map(c => <li key={c.key}><b>{c.label}</b>{c.required ? ' (필수)' : ''} — {c.help}</li>)}</ul>
        </details>
      </section>

      {headIssues.length > 0 && (
        <div className="ui-callout is-danger" role="alert">{headIssues.map((h, i) => <p key={i}>{h.message}</p>)}</div>
      )}

      {rows.length > 0 && (
        <section className="aq-bulk-table-wrap">
          <div className="aq-bulk-summary">
            <span>{all.length}개 발매 · {rows.length}곡 · <b className={issues.length ? 'aq-bulk-bad' : ''}>오류 {issues.length}건</b>
              {issues.length > 0 && <> ({badRows.size}줄)</>}</span>
            <label className="small"><input type="checkbox" checked={onlyErrors} onChange={e => setOnlyErrors(e.target.checked)} /> 오류 줄만 보기</label>
            <button type="button" className="ui-btn is-soft is-sm" onClick={() => saveFile('audeniq-bulk-fixed.csv', toCsv(rows))}>고친 내용 CSV로 받기</button>
            <button type="button" className="ui-btn is-primary" disabled={!ready.length || !!busy} onClick={() => void run()}>
              {busy || (mode === 'create' ? `오류 없는 ${ready.length}개 발매 임시 저장` : `오류 없는 ${ready.length}개 발매 고치기`)}
            </button>
          </div>
          <div className="aq-bulk-scroll">
            <table className="aq-bulk-table">
              <thead><tr><th>줄</th>{shownCols.map(c => <th key={c.key}>{c.label}{c.required ? ' *' : ''}</th>)}</tr></thead>
              <tbody>
                {visible.map(({ r, line }) => (
                  <tr key={line} className={badRows.has(line) ? 'is-bad' : ''}>
                    <td>{line}</td>
                    {shownCols.map(c => {
                      const msg = byCell.get(`${line}:${c.key}`);
                      return (
                        <td key={c.key} className={msg ? 'is-bad' : ''} title={msg}>
                          <input value={r[c.key] ?? ''} aria-label={`${line}번째 줄 ${c.label}`} aria-invalid={!!msg}
                            readOnly={c.key === 'release_id'} onChange={e => edit(line, c.key, e.target.value)} />
                        </td>
                      );
                    })}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          {issues.length > 0 && (
            <ul className="aq-bulk-issues">
              {issues.slice(0, 200).map((i, n) => <li key={n}><b>{i.row}번째 줄</b> · {COLUMNS.find(c => c.key === i.column)?.label ?? i.column}: {i.message}</li>)}
              {issues.length > 200 && <li>…외 {issues.length - 200}건</li>}
            </ul>
          )}
        </section>
      )}

      {results.length > 0 && (
        <section className="ui-card aq-bulk-results">
          <h2 className="subhead">결과</h2>
          <ul className="ui-list">
            {results.map(r => (
              <li key={r.key} className="ui-row">
                <span className="min-0"><span className="ui-row-title">{r.title}</span>
                  {r.message && <span className="ui-row-meta"><span className="aq-bulk-bad">{r.message}</span></span>}</span>
                {r.ok && r.id && <Link to={`/upload?edit=${encodeURIComponent(r.id)}`} className="ui-btn is-soft is-sm">음원·커버 마저 올리기</Link>}
              </li>
            ))}
          </ul>
        </section>
      )}
    </div>
  );
}
