// 장르 고르기 — 배급 신청서와 같은 목록(lib/catalog GENRES)에서 고르고, 목록에 없으면 '기타 · 직접 입력'으로 적는다.
// 값은 장르 이름 하나(목록 값 또는 직접 적은 말) — 곡 정보·Open A&R이 배급 신청서와 같은 말로 장르를 남긴다
import { useState } from 'react';
import { GENRES } from '../lib/catalog';
import { inputClass, selectClass } from '../ui';

const known = (v: string) => GENRES.some(([g]) => g && g !== '__other__' && g === v);

export function GenreSelect({ id, value, onChange, required }: { id: string; value: string; onChange: (v: string) => void; required?: boolean }) {
  // 목록에 없는 장르가 이미 적혀 있거나 '직접 입력'을 고르면 입력칸을 연다
  const [other, setOther] = useState(!!value && !known(value));
  const sel = known(value) ? value : other ? '__other__' : '';
  return (
    <>
      <select id={id} className={selectClass()} value={sel} required={required}
        onChange={e => {
          const v = e.target.value;
          if (v === '__other__') {
            setOther(true);
            onChange('');
            requestAnimationFrame(() => document.getElementById(`${id}-custom`)?.focus());
          } else { setOther(false); onChange(v); }
        }}>
        {GENRES.map(([g, label]) => <option key={g || 'empty'} value={g} disabled={g === ''}>{label}</option>)}
      </select>
      {sel === '__other__' && (
        <input id={`${id}-custom`} className={inputClass(undefined, 'aq-reveal')} style={{ marginTop: 10 }} maxLength={60}
          value={value} onChange={e => onChange(e.target.value)} placeholder="예: 시티팝" aria-label="직접 입력할 장르" />
      )}
    </>
  );
}
