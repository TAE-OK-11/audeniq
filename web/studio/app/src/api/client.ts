// API 진입점 — 화면은 이 파일의 `api`만 쓴다.
// `bun run build:edge`(또는 VITE_MOCK=false)로 빌드하면 실제 서버
// (엣지 Worker → Workers VPC → Cloudflare Tunnel → 메인 서버)를 호출하고,
// 기본값은 브라우저 저장소 기반 체험(목) 모드다.
import type { AssetChecks, DeliveryItem, DspAvailability, Invitation, OrgMember, WithdrawQuota, Org, PreflightIssue, Release, ReleaseDetail, ReleasePayload, SaveResult, UploadKind, UploadResult, User } from './types';
import { ApiError } from './errors';
import { setOrgId } from './http';
import { mockApi } from './mock';
import { remoteApi } from './remote';

export * from './types';
export { ApiError };

import { MOCK } from '../lib/mode';
import { DSP } from '../lib/catalog';
export { MOCK };

/** 체험 모드는 비밀번호 8자, 실서버는 12자 이상 (서버 정책) */
export const PASSWORD_MIN = MOCK ? 8 : 12;

/** AuthProvider가 선택한 조직을 알려준다 (조직 범위 API 경로에 사용) */
export function setCurrentOrg(id: string) {
  setOrgId(id);
}

const delay = (ms: number) => new Promise(r => setTimeout(r, ms));

export const api = {
  login: (email: string, password: string): Promise<User> => (MOCK ? mockApi.login(email, password) : remoteApi.login(email, password)),
  loginPasskey: (email: string): Promise<User> => (MOCK
    ? Promise.reject(new ApiError('체험 모드에서는 패스키를 쓸 수 없어요.', 400, 'PASSKEY_UNSUPPORTED'))
    : remoteApi.loginPasskey(email)),
  signup: (email: string, password: string): Promise<User> => (MOCK ? mockApi.signup(email, password) : remoteApi.signup(email, password)),
  logout: (): Promise<void> => (MOCK ? mockApi.logout() : remoteApi.logout()),
  me: (): Promise<User> => (MOCK ? mockApi.me() : remoteApi.me()),
  listOrgs: (): Promise<Org[]> => (MOCK ? mockApi.listOrgs() : remoteApi.listOrgs()),
  /** 작업 공간 멤버 — 소유자만 초대·역할 변경·해제 */
  listMembers: (): Promise<{ role: string; items: OrgMember[] }> => (MOCK ? mockApi.listMembers() : remoteApi.listMembers()),
  inviteMember: (email: string, role: 'EDITOR' | 'VIEWER'): Promise<void> => (MOCK ? mockApi.inviteMember(email, role) : remoteApi.inviteMember(email, role)),
  setMember: (userId: string, role: 'EDITOR' | 'VIEWER', status: 'ACTIVE' | 'REVOKED'): Promise<void> => (MOCK ? mockApi.setMember(userId, role, status) : remoteApi.setMember(userId, role, status)),
  /** 내가 받은 작업 공간 초대 */
  listInvitations: (): Promise<Invitation[]> => (MOCK ? mockApi.listInvitations() : remoteApi.listInvitations()),
  acceptInvitation: (orgId: string): Promise<void> => (MOCK ? mockApi.acceptInvitation(orgId) : remoteApi.acceptInvitation(orgId)),
  declineInvitation: (orgId: string): Promise<void> => (MOCK ? mockApi.declineInvitation(orgId) : remoteApi.declineInvitation(orgId)),
  /** 올린 곡의 사전 품질 검사 결과 */
  assetChecks: (assetId: string): Promise<AssetChecks> => (MOCK ? mockApi.assetChecks(assetId) : remoteApi.assetChecks(assetId)),

  listReleases: (): Promise<Release[]> => (MOCK ? mockApi.listReleases() : remoteApi.listReleases()),
  listDsps: (): Promise<DspAvailability[]> => (MOCK ? Promise.resolve(DSP.map(([slug, name], i) => ({ slug, name, region: i < 4 ? 'KR' : 'GLOBAL', category: [24, 25, 26, 27, 31, 32, 35, 38, 39].includes(i) ? 'SOCIAL' : [28, 29, 33].includes(i) ? 'STORE' : [40, 41, 42, 43, 44].includes(i) ? 'LICENSING' : 'STREAMING', available: true }))) : remoteApi.listDsps()),
  getRelease: (id: string): Promise<ReleaseDetail> => (MOCK ? mockApi.getRelease(id) : remoteApi.getRelease(id)),
  /** 임시 저장 — id가 없으면 새 draft를 만들고, 있으면 같은 draft를 갱신 */
  saveDraft: (id: string | null, data: ReleasePayload): Promise<SaveResult> => (MOCK ? mockApi.saveDraft(id, data) : remoteApi.saveDraft(id, data)),
  /** 발매 신청 접수 (새 발매 또는 기존 draft/보완 요청 발매) */
  submitRelease: (id: string | null, data: ReleasePayload): Promise<Release> => (MOCK ? mockApi.submitRelease(id, data) : remoteApi.submitRelease(id, data)),
  deleteRelease: (id: string): Promise<void> => (MOCK ? mockApi.deleteRelease(id) : remoteApi.deleteRelease(id)),
  /** 접수한 발매 신청 취소 — 한 달에 3회까지 직접, 그 이상은 문의로 */
  withdrawRelease: (id: string): Promise<WithdrawQuota> => (MOCK ? mockApi.withdrawRelease(id) : remoteApi.withdrawRelease(id)),
  withdrawQuota: (): Promise<WithdrawQuota> => (MOCK ? mockApi.withdrawQuota() : remoteApi.withdrawQuota()),
  preflight: async (id: string): Promise<PreflightIssue[]> => (MOCK ? [] : remoteApi.preflight(id)),
  /** 플랫폼별 배급 진행 — 체험 모드에는 실제 배급이 없어 빈 목록 */
  getDelivery: async (id: string): Promise<DeliveryItem[]> => (MOCK ? [] : remoteApi.getDelivery(id)),

  /**
   * 파일 업로드. 실서버: 서명 URL로 R2에 직접 PUT 후 등록.
   * 체험 모드: 업로드 없이 진행률만 흉내 낸다.
   */
  uploadFile: async (file: File, kind: UploadKind, onProgress?: (r: number) => void, signal?: AbortSignal): Promise<UploadResult> => {
    if (!MOCK) return remoteApi.uploadFile(file, kind, onProgress, signal);
    for (let i = 1; i <= 5; i++) {
      if (signal?.aborted) throw new ApiError('업로드를 취소했어요.', 0, 'ABORTED');
      await delay(60);
      onProgress?.(i / 5);
    }
    return { assetId: `mock-${kind.toLowerCase()}-${Date.now().toString(36)}` };
  },
};
