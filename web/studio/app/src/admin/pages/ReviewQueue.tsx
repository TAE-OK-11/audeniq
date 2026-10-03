import { useEffect, useState } from 'react';
import { Link, useSearchParams } from '../../lib/router';
import { errorMessage } from '../../api/errors';
import { staffApi, type QueueRelease } from '../api';
import { QUEUE_FILTERS, RELEASE_STATUS, RELEASE_TYPE, ago, applicationPending, day, dspLabel, pick } from '../labels';
import { Chip, Empty, ErrorBox, Filters, Initial, PageHead, Skeleton, StatusChip, SubTabs, useStaff } from '../ui';

const PAGE = 50;
/** 심사 상세의 ‘다음 건’이 이 목록 순서를 따른다 */
export const QUEUE_ORDER_KEY = 'adm.queue.order';

const DAY = 86_400_000;
function daysLeft(d?: string | null): number | null {
  if (!d || !/^\d{4}-\d{2}-\d{2}/.test(d)) return null;
  const today = new Date(Date.now() + 9 * 3_600_000).toISOString().slice(0, 10);
  return Math.round((Date.parse(d.slice(0, 10)) - Date.parse(today)) / DAY);
}
const waitedHours = (iso?: string | null) => (iso ? (Date.now() - Date.parse(iso)) / 3_600_000 : 0);

export function ReviewQueue() {
  const [params, setParams] = useSearchParams();
  const status = params.get('status') || 'PENDING';
  const [items, setItems] = useState<QueueRelease[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [more, setMore] = useState(false);
  const [tick, setTick] = useState(0);
  const [search, setSearch] = useState('');
  const [sort, setSort] = useState<'wait' | 'date'>('wait');
  // 담당 필터: 전체 · 내 담당 · 담당자 없음
  const [who, setWho] = useState<'all' | 'mine' | 'free'>('all');
  const { counts, me } = useStaff();

  useEffect(() => {
    let alive = true;
    setLoading(true);
    setError('');
    staffApi.releases(status)
      .then(r => { if (!alive) return; setItems(r.items); setMore(r.items.length >= PAGE); })
      .catch(e => { if (alive) setError(errorMessage(e)); })
      .finally(() => { if (alive) setLoading(false); });
    return () => { alive = false; };
  }, [status, tick]);

  const loadMore = async () => {
    try {
      const r = await staffApi.releases(status, items.length);
      setItems(list => [...list, ...r.items]);
      setMore(r.items.length >= PAGE);
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  const term = search.trim().toLowerCase();
  const filtered = items
    .filter(r => !term || [r.title, r.artist, r.org_name].some(v => v?.toLowerCase().includes(term)))
    .filter(r => who === 'all' || (who === 'mine' ? r.claim?.user_id === me.user_id : !r.claim));
  // 기본은 서버 순서(오래 기다린 순), ‘발매일 빠른 순’이면 급한 발매부터
  const shown = sort === 'date'
    ? [...filtered].sort((a, b) => String(a.release_date || '9999').localeCompare(String(b.release_date || '9999')))
    : filtered;

  // 상세 화면에서 ‘다음 심사 건’으로 이어서 보도록 지금 보이는 순서를 기억한다
  useEffect(() => {
    try { sessionStorage.setItem(QUEUE_ORDER_KEY, JSON.stringify(shown.map(r => r.id))); } catch { /* 저장소 차단 */ }
  }, [shown]);

  return (
    <div className="view-enter">
      <PageHead
        eyebrow="발매 심사"
        title="발매 심사"
        sub="새 발매 신청과 담당자 판단이 필요한 발매예요. 오래 기다린 순서로 보여요."
        actions={<button type="button" className="adm-btn soft small" onClick={() => setTick(t => t + 1)}>새로고침</button>}
      />
      <SubTabs tabs={[{ to: '/admin/reviews', label: '심사 목록', count: counts?.review }, { to: '/admin/approvals', label: '2차 승인', count: counts?.second_approvals }]} />
      <Filters
        label="발매 상태"
        value={status}
        onChange={v => setParams({ status: v }, { replace: true })}
        options={QUEUE_FILTERS.map(s => ({ value: s, label: RELEASE_STATUS[s][0] }))}
      />
      <div className="adm-queue-tools">
        <label htmlFor="qSearch" className="sr-only">검색</label>
        <input id="qSearch" type="search" className="adm-input" placeholder="제목·아티스트·작업 공간으로 찾기" value={search} onChange={e => setSearch(e.target.value)} />
        <div className="adm-seg" role="radiogroup" aria-label="담당">
          {([['all', '전체'], ['mine', '내 담당'], ['free', '담당자 없음']] as const).map(([k, l]) => (
            <button key={k} type="button" role="radio" aria-checked={who === k} className={who === k ? 'is-on' : ''} onClick={() => setWho(k)}>{l}</button>
          ))}
        </div>
        <div className="adm-seg" role="radiogroup" aria-label="정렬">
          <button type="button" role="radio" aria-checked={sort === 'wait'} className={sort === 'wait' ? 'is-on' : ''} onClick={() => setSort('wait')}>오래 기다린 순</button>
          <button type="button" role="radio" aria-checked={sort === 'date'} className={sort === 'date' ? 'is-on' : ''} onClick={() => setSort('date')}>발매일 빠른 순</button>
        </div>
      </div>
      {!loading && items.length > 0 && <p className="adm-queue-count">{term ? `‘${search.trim()}’ 검색 결과 ${shown.length}건` : `${shown.length}건`}</p>}

      {error && <ErrorBox message={error} onRetry={() => setTick(t => t + 1)} />}
      {loading ? <Skeleton /> : shown.length === 0 ? (
        <Empty title={status === 'PENDING' ? '심사 대기 중인 발매가 없어요' : '해당 상태의 발매가 없어요'}>
          {status === 'PENDING' ? '새로 들어오는 발매 신청은 자동 검사 후 이곳에 쌓여요.' : '다른 상태를 선택해 보세요.'}
        </Empty>
      ) : (
        <div className="adm-list">
          {shown.map(r => (
            <Link key={r.id} to={`/admin/reviews/${r.id}`} className="adm-row">
              <Initial text={r.title} src={r.cover} />
              <span className="adm-min">
                <span className="adm-row-title">{r.title}</span>
                <span className="adm-row-meta">
                  <span>{r.artist || '아티스트 미기재'}</span>
                  <span>{r.org_name}</span>
                  <span>{RELEASE_TYPE[r.release_type] ?? r.release_type}</span>
                  <span>발매 예정 {day(r.release_date)}</span>
                </span>
                {r.platforms.length > 0 && (
                  <span className="adm-dsps" title={r.platforms.map(dspLabel).join(', ')}>
                    {r.platforms.slice(0, 4).map(p => <span key={p}>{dspLabel(p)}</span>)}
                    {r.platforms.length > 4 && <span className="is-more">+{r.platforms.length - 4}</span>}
                  </span>
                )}
              </span>
              <span className="adm-row-end">
                {(() => {
                  const left = daysLeft(r.release_date);
                  return left != null && left < 21
                    ? <Chip tone={left < 14 ? 'red' : 'amber'}>{left < 0 ? '발매일 지남' : left === 0 ? '오늘 발매' : `발매 D-${left}`}</Chip>
                    : null;
                })()}
                {r.claim && <Chip tone={r.claim.user_id === me.user_id ? 'blue' : 'gray'}>{r.claim.user_id === me.user_id ? '내 담당' : `담당 ${r.claim.email.split('@')[0]}`}</Chip>}
                {r.status === 'READY_FOR_DELIVERY' && applicationPending(r.agreement)
                  ? <Chip tone="violet">새 발매 신청</Chip>
                  : <StatusChip value={pick(RELEASE_STATUS, r.status)} />}
                <small className={waitedHours(r.submitted_at) >= 48 ? 'is-late' : undefined}>접수 {ago(r.submitted_at) || '—'}</small>
              </span>
            </Link>
          ))}
        </div>
      )}
      {more && !term && !loading && (
        <div className="adm-more"><button type="button" className="adm-btn soft" onClick={loadMore}>더 보기</button></div>
      )}
    </div>
  );
}
