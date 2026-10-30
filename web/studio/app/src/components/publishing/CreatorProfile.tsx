// 계정 정보의 '작가' — 한 계정이 아티스트·작곡가·작사가·레이블 담당 역할을 함께 가진다 (기획안 5.1).
// 법적 이름은 본인 화면에서만 보인다. 관리 단체·IPI는 작품을 만들 때 참여자 칸에 자동으로 들어간다
import { useState } from 'react';
import { pubApi, type Creator, type CreatorRole } from '../../api/publishing';
import { errorMessage } from '../../api/errors';
import { useAsync } from '../../hooks/useAsync';
import { SOCIETIES } from '../../lib/publishing';
import { Link } from '../../lib/router';
import { useToast } from '../Toast';
import { Modal } from '../Modal';
import { Glyph } from '../Glyph';
import { Button, Field, inputClass, selectClass } from '../../ui';

const ROLES: [CreatorRole, string][] = [['artist', '아티스트'], ['composer', '작곡가'], ['lyricist', '작사가'], ['arranger', '편곡가'], ['producer', '프로듀서'], ['label', '레이블 담당자']];
const COUNTRIES: [string, string][] = [['KR', '대한민국'], ['US', '미국'], ['JP', '일본'], ['OTHER', '기타']];

function Editor({ c, onClose, onSaved }: { c: Creator; onClose: () => void; onSaved: (c: Creator) => void }) {
  const toast = useToast();
  const [f, setF] = useState<Creator>(c);
  const [busy, setBusy] = useState(false);
  const ipiBad = !!f.ipi && !/^\d{9,11}$/.test(f.ipi);
  const save = async () => {
    if (!f.name.trim() || ipiBad) return;
    setBusy(true);
    try { onSaved(await pubApi.saveMyCreator({ ...f, name: f.name.trim() })); toast('작가 정보를 저장했어요.', 'success'); onClose(); }
    catch (e) { toast(errorMessage(e), 'error'); } finally { setBusy(false); }
  };
  return (
    <Modal title="작가 정보" onClose={onClose} dismissible={false}>
      <div>
        <Field label="작가 활동명" htmlFor="crName" required error={!f.name.trim() ? '활동명을 입력해 주세요.' : undefined}>
          <input id="crName" className={inputClass()} value={f.name} onChange={e => setF({ ...f, name: e.target.value })} />
        </Field>
        <Field label="법적 이름" htmlFor="crLegal" help="관리 단체 등록·계약에만 쓰고 다른 사람에게는 보이지 않아요.">
          <input id="crLegal" className={inputClass()} value={f.legalName ?? ''} onChange={e => setF({ ...f, legalName: e.target.value || null })} autoComplete="name" />
        </Field>
        <Field label="역할" help="여러 개를 고를 수 있어요. 역할마다 계정을 따로 만들지 않아요.">
          <div className="ui-chips is-wrap" role="group" aria-label="역할">
            {ROLES.map(([r, l]) => (
              <button key={r} type="button" className="ui-chip is-sm" aria-pressed={f.roles.includes(r)}
                onClick={() => setF({ ...f, roles: f.roles.includes(r) ? f.roles.filter(x => x !== r) : [...f.roles, r] })}>{l}</button>
            ))}
          </div>
        </Field>
        <div className="form-grid">
          <Field label="저작권 관리 단체" htmlFor="crSoc">
            <select id="crSoc" className={selectClass()} value={f.society ?? ''} onChange={e => setF({ ...f, society: e.target.value || null })}>
              <option value="">가입하지 않았어요</option>
              {SOCIETIES.filter(s => s !== '없음').map(s => <option key={s} value={s}>{s}</option>)}
            </select>
          </Field>
          <Field label="IPI/CAE" htmlFor="crIpi" error={ipiBad ? '숫자 9~11자리예요.' : undefined}>
            <input id="crIpi" className={inputClass()} inputMode="numeric" value={f.ipi ?? ''} onChange={e => setF({ ...f, ipi: e.target.value.replace(/\D/g, '') || null })} placeholder="00712345678" />
          </Field>
          <Field label="소속 조직" htmlFor="crOrg">
            <input id="crOrg" className={inputClass()} value={f.org ?? ''} onChange={e => setF({ ...f, org: e.target.value || null })} placeholder="예: 소속 퍼블리셔·레이블" />
          </Field>
          <Field label="국가" htmlFor="crCountry">
            <select id="crCountry" className={selectClass()} value={f.country ?? 'KR'} onChange={e => setF({ ...f, country: e.target.value })}>
              {COUNTRIES.map(([v, l]) => <option key={v} value={v}>{l}</option>)}
            </select>
          </Field>
        </div>
        <div className="aq-confirm-actions">
          <Button variant="soft" onClick={onClose}>닫기</Button>
          <Button variant="primary" busy={busy} disabled={!f.name.trim() || ipiBad} onClick={() => void save()}>{busy ? '저장 중…' : '저장'}</Button>
        </div>
      </div>
    </Modal>
  );
}

export function CreatorProfile() {
  const { data, error } = useAsync(() => pubApi.myCreator(), [], { key: 'my-creator' });
  const { data: works } = useAsync(() => pubApi.listWorks(), [], { key: 'works' });
  const [local, setLocal] = useState<Creator | null>(null);
  const [open, setOpen] = useState(false);
  const c = local ?? data;
  // 퍼블리싱 API가 없는 서버는 이 묶음을 그리지 않는다
  if (error || !c) return null;
  const roleText = c.roles.map(r => ROLES.find(x => x[0] === r)?.[1]).filter(Boolean).join(' · ');
  const mineCount = (works ?? []).filter(w => w.contributors.some(x => x.isMe)).length;
  const row = (label: string, value: string | null | undefined, empty = '입력하기') => (
    <button type="button" className="aq-set-row" onClick={() => setOpen(true)}>
      <span className="aq-set-label">{label}</span>
      {value ? <span className="aq-set-value">{value}</span> : <span className="aq-set-value is-empty">{empty}</span>}
      <Glyph name="chevron-right" size={16} className="aq-set-chev" />
    </button>
  );
  return (
    <section className="aq-set-card" aria-labelledby="setCreator">
      <h2 id="setCreator">작가·저작권자</h2>
      {row('작가 활동명', c.name)}
      {row('법적 이름', c.legalName)}
      {row('역할', roleText)}
      {row('저작권 관리 단체', c.society, '가입하지 않음')}
      {row('IPI/CAE', c.ipi)}
      {row('소속 조직', c.org, '없음')}
      <Link to="/releases?tab=works&wf=mine" className="aq-set-row">
        <span className="aq-set-label">참여한 작품</span>
        <span className="aq-set-value">{works ? `${mineCount}개` : '…'}</span>
        <Glyph name="chevron-right" size={16} className="aq-set-chev" />
      </Link>
      {open && <Editor c={c} onClose={() => setOpen(false)} onSaved={setLocal} />}
    </section>
  );
}
