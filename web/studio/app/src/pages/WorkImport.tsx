// 다른 아티스트가 발매한 곡 일괄 등록 (/works/import) — 표(CSV)를 붙여 넣거나 파일로 올리면 줄마다 확인한 뒤 한 번에 만든다.
// 곡 등록 첫 화면에서 '다른 아티스트가 발매한 곡'을 고르면 들어올 수 있다. 계산은 lib/workImport
import { useMemo, useState, type ChangeEvent } from 'react';
import { Link, useNavigate } from '../lib/router';
import { pubApi } from '../api/publishing';
import { errorMessage } from '../api/errors';
import { useAsync } from '../hooks/useAsync';
import { IMPORT_COLUMNS, IMPORT_SAMPLE, blocking, importRows } from '../lib/workImport';
import { WizardShell } from '../components/WizardShell';
import { FilePicker } from '../components/FilePicker';
import { Glyph } from '../components/Glyph';
import { useToast } from '../components/Toast';
import { Badge, Callout, Field } from '../ui';
import '../styles/split/Upload.css';
import '../styles/split/Publishing.css';

export function WorkImport() {
  const nav = useNavigate();
  const toast = useToast();
  const { data: me } = useAsync(() => pubApi.myCreator(), [], { key: 'my-creator' });
  const [text, setText] = useState('');
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState<{ ok: number; failed: string[] } | null>(null);
  const rows = useMemo(() => (me && text.trim() ? importRows(text, me) : []), [text, me]);
  const ready = rows.filter(r => !blocking(r));

  const pick = async (e: ChangeEvent<HTMLInputElement>) => {
    const f = e.target.files?.[0];
    e.target.value = '';
    if (f) setText(await f.text());
  };
  const template = () => {
    const url = URL.createObjectURL(new Blob(['﻿' + IMPORT_SAMPLE], { type: 'text/csv;charset=utf-8' }));
    const a = document.createElement('a');
    a.href = url; a.download = 'AUDENIQ_곡_일괄등록.csv'; a.click();
    URL.revokeObjectURL(url);
  };
  const run = async () => {
    if (busy || !ready.length) return;
    setBusy(true);
    let ok = 0;
    const failed: string[] = [];
    for (const r of ready) {
      try { await pubApi.createWork(r.input!); ok++; } catch (err) { failed.push(`${r.line}줄 ‘${r.title}’ — ${errorMessage(err)}`); }
    }
    setBusy(false);
    setDone({ ok, failed });
    if (ok) toast(`${ok}곡을 등록했어요.`, 'success');
  };

  return (
    <WizardShell className="aq-pbl-wizard" label="곡 일괄 등록" title="여러 곡 한 번에 등록"
      steps={[{ short: '표 붙여 넣기', title: '다른 아티스트가 낸 곡을\n한 번에 등록해요.', sub: '표(CSV)를 붙여 넣거나 파일로 올리면 줄마다 확인해 드려요.' }]}
      step={0} reached={0} onStep={() => {}} onBack={() => nav(-1)}
      summary={{ title: rows.length ? `${rows.length}줄 읽음` : '표를 넣어 주세요', lines: [`등록할 수 있는 곡 ${ready.length}곡`] }}
      actions={<>
        <button type="button" id="wizardBack" className="ui-btn is-soft" onClick={() => nav(-1)} disabled={busy}>나가기</button>
        {done ? <Link to="/releases?tab=works" id="wizardNext" className="ui-btn is-primary">곡 목록 보기</Link>
          : <button type="button" id="wizardNext" className={`ui-btn is-primary${busy ? ' is-busy' : ''}`} disabled={!ready.length || busy} onClick={() => void run()}>{busy ? '등록하는 중' : `${ready.length}곡 등록`}</button>}
      </>}
    >
      {done ? (
        <>
          <Callout tone={done.failed.length ? 'warn' : 'success'}><b>{done.ok}곡을 등록했어요.</b> {done.failed.length ? `${done.failed.length}곡은 등록하지 못했어요.` : '곡 화면에서 지분을 확인하고 저작권 관리를 신청하면 돼요.'}</Callout>
          {done.failed.map(f => <p key={f} className="ui-field-help is-error">{f}</p>)}
        </>
      ) : (
        <>
          <p className="ui-field-help">칸: {IMPORT_COLUMNS.join(' · ')}. 공동 작가는 <code>이름:역할:지분</code>을 <code>|</code>로 이어 적어요. 내 이름·관리 단체·IPI는 계정 정보의 작가 정보로 채워요.</p>
          <div className="row-actions"><button type="button" className="ui-btn is-soft is-sm" onClick={template}><Glyph name="download" size={14} className="aq-glyph-lead" />견본 표 내려받기</button></div>
          <Field label="표 붙여 넣기" htmlFor="importText" help="엑셀·구글 시트에서 복사해 붙여 넣어도 돼요 (첫 줄은 칸 이름).">
            <textarea id="importText" className="ui-textarea" rows={8} value={text} onChange={e => { setText(e.target.value); setDone(null); }} placeholder={IMPORT_SAMPLE} spellCheck={false} />
          </Field>
          <Field label="또는 CSV 파일" htmlFor="importFile"><FilePicker id="importFile" accept=".csv,text/csv" onChange={e => void pick(e)} /></Field>
          {rows.length > 0 && (
            <div className="ui-list aq-pbl-import-list" aria-label="읽은 곡">
              {rows.map(r => (
                <div key={r.line} className="ui-row is-flat">
                  <span className="min-0">
                    <span className="ui-row-title">{r.line}줄 · {r.title}</span>
                    <span className="ui-row-meta">
                      {r.input?.externalRelease && <span>{r.input.externalRelease.artist} 발매</span>}
                      {r.input && <span>{r.input.contributors.map(c => `${c.name} ${c.share}%`).join(' · ')}</span>}
                    </span>
                    {r.errors.map(e => <small key={e} className={`ui-field-help${blocking(r) ? ' is-error' : ''}`}>{e}</small>)}
                  </span>
                  {blocking(r) ? <Badge tone="red">고쳐 주세요</Badge> : r.errors.length ? <Badge tone="amber">지분 확인</Badge> : <Badge tone="green">등록 가능</Badge>}
                </div>
              ))}
            </div>
          )}
        </>
      )}
    </WizardShell>
  );
}
