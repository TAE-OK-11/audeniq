// 미성년 아티스트의 발매 — 법정대리인이 직접 본인확인 후 서명하는 동의서와, 관계를 확인할
// 가족관계증명서 (서버: crates/core/src/signing.rs guardian_consent).
import { req } from './http';
import type { IssuedLink, SigningRequest } from './signing';
import type { SigningChannel } from '../lib/rightsDocument';

export type GuardianRelation = '부' | '모' | '기타 법정대리인';

export interface GuardianStatus {
  release_id: string;
  /** 발매 초안에 적은 법정대리인 */
  guardian: { name: string; relation: string };
  requests: SigningRequest[];
  relation_document: { id: string; status: string; has_file: boolean } | null;
  /** 서명과 가족관계증명서가 모두 준비돼 신청할 수 있는지 */
  ready: boolean;
}

const path = (org: string, release: string) =>
  `/api/orgs/${encodeURIComponent(org)}/releases/${encodeURIComponent(release)}/guardian-consent`;

export const guardianApi = {
  status: (org: string, release: string) => req<GuardianStatus>(path(org, release)),
  /** 법정대리인의 서명 링크 (같은 사람에게 다시 열면 새 링크가 나와요) */
  open: (org: string, release: string, i: { guardian_name: string; relation: string; channel: SigningChannel }) =>
    req<IssuedLink>(path(org, release), { method: 'POST', body: i }),
};
