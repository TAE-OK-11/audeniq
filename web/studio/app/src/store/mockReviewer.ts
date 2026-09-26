// 체험(목) 모드 전용 — 실제 담당자가 없으므로 접수된 계약서 검토를 일정 시간 뒤 자동으로 완료한다.
// 실제 API 모드에서는 시작되지 않는다.
import { getDocsSnapshot, updateDoc } from './docs';
import { pushNotice } from './support';
import { parseStamp, stampNow } from '../lib/date';
import { stripSampleSuffix } from '../lib/format';
import { uid } from '../lib/store';

/** 접수 후 자동 검토 완료까지 걸리는 시간(초) — 스탬프가 분 단위라 최소 1분 */
export const MOCK_REVIEW_SECONDS = 60;

function tick() {
  const now = Date.now();
  for (const d of getDocsSnapshot()) {
    if (d.kind !== 'agreements' || !['prepared', 'review'].includes(d.reviewStatus)) continue;
    const last = parseStamp(d.reviewHistory.at(-1)?.time || d.created);
    if (last && now - last.getTime() < MOCK_REVIEW_SECONDS * 1000) continue;
    const at = stampNow();
    updateDoc(d.id, {
      reviewStatus: 'approved',
      approvedAt: at,
      reviewHistory: [...d.reviewHistory, { status: '검토 완료', time: at, detail: 'AUDENIQ 담당자 검토가 끝났어요. 서명을 진행해 주세요.' }],
    });
    pushNotice({
      id: uid('n'), kind: '서류', time: at, link: '/contracts',
      title: `${stripSampleSuffix(d.releaseTitle || d.title)} 계약서 검토가 끝났어요.`,
      detail: '계약서 메뉴에서 내용을 확인하고 서명을 진행해 주세요.',
    });
  }
}

let started = false;
export function startMockReviewer() {
  if (started) return;
  started = true;
  tick();
  window.setInterval(tick, 10000);
}
