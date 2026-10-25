// 플랫폼 요청 종류 — 아티스트 프로필 사후 관리와 유튜브 Content ID(UGC). 서버 crates/core/src/platform_requests.rs와 같은 키.
export type RequestGroup = 'profile' | 'content_id';

export interface KindSpec {
  kind: string; group: RequestGroup; label: string; help: string;
  /** 링크 입력 칸 이름들 (첫 칸부터 필수 개수만큼) */
  urls: string[]; minUrls: number;
  release: 'required' | 'optional' | 'none';
  description: 'required' | 'optional';
  /** 플랫폼 고정 (유튜브) */
  youtube?: boolean;
}

export const KINDS: KindSpec[] = [
  { kind: 'PROFILE_MISMATCH', group: 'profile', label: '다른 아티스트 페이지에 올라갔어요',
    help: '동명이인 등 다른 아티스트 페이지에 내 발매가 붙은 경우. 올바른 페이지가 있으면 함께 적어 주세요.',
    urls: ['잘못 붙은 아티스트 페이지 링크', '올바른 내 아티스트 페이지 링크 (있으면)'], minUrls: 1, release: 'required', description: 'optional' },
  { kind: 'PROFILE_SPLIT', group: 'profile', label: '다른 아티스트와 한 페이지를 쓰고 있어요',
    help: '나와 다른 아티스트의 음원이 한 페이지에 섞인 경우 페이지를 나눠 달라고 요청해요.',
    urls: ['섞여 있는 아티스트 페이지 링크'], minUrls: 1, release: 'optional', description: 'required' },
  { kind: 'PROFILE_MERGE', group: 'profile', label: '내 아티스트 페이지가 여러 개예요',
    help: '같은 아티스트인데 페이지가 나뉜 경우 하나로 합쳐 달라고 요청해요. 남길 페이지를 첫 칸에 적어 주세요.',
    urls: ['남길 아티스트 페이지 링크', '합칠 아티스트 페이지 링크', '합칠 페이지 링크 (더 있으면)'], minUrls: 2, release: 'none', description: 'optional' },
  { kind: 'OAC', group: 'profile', label: '유튜브 공식 아티스트 채널(OAC) 신청', youtube: true,
    help: '내 유튜브 채널과 음원 주제(Topic) 채널을 하나의 공식 아티스트 채널로 묶어요. 배급된 발매가 3개 이상이면 승인될 가능성이 높아요.',
    urls: ['내 유튜브 채널 링크', '주제(Topic) 채널 링크 (있으면)'], minUrls: 1, release: 'none', description: 'optional' },
  { kind: 'CID_ALLOWLIST', group: 'content_id', label: '내 채널은 소유권 주장에서 빼 주세요', youtube: true,
    help: '내 채널이나 사용을 허락한 채널의 영상에 내 음원 소유권 주장이 걸리지 않게 해요.',
    urls: ['제외할 유튜브 채널 링크', '채널 링크 (더 있으면)', '채널 링크 (더 있으면)'], minUrls: 1, release: 'optional', description: 'optional' },
  { kind: 'CID_RELEASE_CLAIM', group: 'content_id', label: '이 영상의 소유권 주장을 풀어 주세요', youtube: true,
    help: '허락한 영상에 소유권 주장이 이미 걸린 경우 해제를 요청해요.',
    urls: ['영상 링크', '영상 링크 (더 있으면)', '영상 링크 (더 있으면)'], minUrls: 1, release: 'optional', description: 'optional' },
  { kind: 'CID_DISPUTE', group: 'content_id', label: '다른 사람이 내 음원에 소유권을 주장해요', youtube: true,
    help: '내 음원을 쓴 영상(또는 내 음원 자체)에 다른 권리자가 소유권을 주장한 경우 이의를 제기해요. 누가, 왜 잘못 주장했는지 적어 주세요.',
    urls: ['문제의 영상 링크', '영상 링크 (더 있으면)'], minUrls: 1, release: 'optional', description: 'required' },
  { kind: 'CID_OPT_OUT', group: 'content_id', label: '이 발매의 Content ID를 꺼 주세요', youtube: true,
    help: '유튜브에서 이 발매로 소유권 주장·수익 창출을 하지 않게 해요.',
    urls: [], minUrls: 0, release: 'required', description: 'optional' },
  { kind: 'CID_OPT_IN', group: 'content_id', label: '이 발매의 Content ID를 다시 켜 주세요', youtube: true,
    help: '꺼 둔 Content ID를 다시 켜서 유튜브 수익을 받아요.',
    urls: [], minUrls: 0, release: 'required', description: 'optional' },
];

/** 문의 화면에서는 만들지 않는 요청 — 아티스트 페이지 > 프로필 사진에서 사진과 함께 만든다 */
const MADE_ELSEWHERE: KindSpec[] = [
  { kind: 'PROFILE_IMAGE', group: 'profile', label: '프로필 사진 바꾸기',
    help: '아티스트 페이지에서 올린 프로필 사진을 플랫폼에 보내 바꿔요.',
    urls: ['플랫폼 아티스트 페이지'], minUrls: 0, release: 'none', description: 'optional' },
];

export const kindSpec = (kind: string) => KINDS.find(k => k.kind === kind) ?? MADE_ELSEWHERE.find(k => k.kind === kind);

export const STATUS_LABEL: Record<string, string> = {
  SUBMITTED: '접수', IN_PROGRESS: '처리 중', SENT_TO_PLATFORM: '플랫폼 전달',
  RESOLVED: '완료', REJECTED: '처리 불가', CANCELLED: '취소',
};
export const STATUS_TONE: Record<string, 'blue' | 'green' | 'red' | 'gray'> = {
  SUBMITTED: 'blue', IN_PROGRESS: 'blue', SENT_TO_PLATFORM: 'gray', RESOLVED: 'green', REJECTED: 'red', CANCELLED: 'gray',
};
export const MONETIZATION_LABEL: Record<string, string> = {
  UNKNOWN: '확인 전', MONETIZING: '수익 창출 중', TRACKING: '추적만 (수익 없음)', BLOCKED: '차단', INELIGIBLE: '대상 아님',
};

export interface PlatformRequest {
  id: string; kind: string; platform: string; release_id: string | null; release_title: string | null;
  artist_name: string; urls: string[]; description: string; status: string; platform_ref: string; resolution: string;
  created_at: string; updated_at: string; org_name?: string;
  /** 사진을 담은 요청 (PROFILE_IMAGE) — 스태프는 /api/staff/platform-requests/{id}/image로 연다 */
  image?: boolean;
  events: { status: string; note: string; by_staff: boolean; at: string }[];
}
