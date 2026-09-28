import { useEffect, useState } from 'react';
import { Link, useSearchParams } from '../../lib/router';
import { errorMessage } from '../../api/errors';
import { staffApi, type QueueRelease } from '../api';
import { QUEUE_FILTERS, RELEASE_STATUS, RELEASE_TYPE, ago, applicationPending, day, dspLabel, pick } from '../labels';
import { Chip, Empty, ErrorBox, Filters, Initial, PageHead, Skeleton, StatusChip, SubTabs, useStaff } from '../ui';

const PAGE = 50;

export function ReviewQueue() {
  const [params, setParams] = useSearchParams();
  const status = params.get('status') || 'PENDING';
  const [items, setItems] = useState<QueueRelease[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [more, setMore] = useState(false);
  const [tick, setTick] = useState(0);
  const [search, setSearch] = useState('');
  const { counts } = useStaff();

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
  const shown = term
    ? items.filter(r => [r.title, r.artist, r.org_name].some(v => v?.toLowerCase().includes(term)))
    : items;

  return (
    <div className="view-enter">
      <PageHead
        eyebrow="RELEASE REVIEW"
        title="발매 심사"
        sub="새로 들어온 발매 신청과 2차 검사에서 담당자 판단이 필요한 발매예요. 오래 기다린 순서로 보여요. 발매를 열어 신청서와 검사 결과를 보고 승인·보완 요청·거절을 결정해 주세요."
        actions={<button type="button" className="adm-btn soft small" onClick={() => setTick(t => t + 1)}>새로고침</button>}
      />
      <SubTabs tabs={[{ to: '/admin/reviews', label: '심사 목록', count: counts?.review }, { to: '/admin/approvals', label: '2차 승인', count: counts?.second_approvals }]} />
      <Filters
        label="발매 상태"
        value={status}
        onChange={v => setParams({ status: v }, { replace: true })}
        options={QUEUE_FILTERS.map(s => ({ value: s, label: RELEASE_STATUS[s][0] }))}
      />
      {items.length > 8 && (
        <div className="adm-field">
          <label htmlFor="qSearch" className="sr-only">검색</label>
          <input id="qSearch" className="adm-input" placeholder="제목·아티스트·작업 공간으로 찾기" value={search} onChange={e => setSearch(e.target.value)} />
        </div>
      )}

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
                {r.platforms.length > 0 && <span className="adm-dsps">{r.platforms.map(p => <span key={p}>{dspLabel(p)}</span>)}</span>}
              </span>
              <span className="adm-row-end">
                {r.status === 'READY_FOR_DELIVERY' && applicationPending(r.agreement)
                  ? <Chip tone="violet">새 발매 신청</Chip>
                  : <StatusChip value={pick(RELEASE_STATUS, r.status)} />}
                <small>접수 {ago(r.submitted_at) || '—'}</small>
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
