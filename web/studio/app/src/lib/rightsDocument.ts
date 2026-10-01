import { dspLabel } from './catalog';

export const RIGHTS_DOCUMENTS = {
  master: { title: '마스터 음원 배급 이용 허락서', grant: '아래 마스터 녹음물을 AUDENIQ를 통해 디지털 음원 플랫폼에 배급·서비스하는 것을 허락합니다.' },
  artwork: { title: '커버아트 이용 허락서', grant: '아래 커버아트를 해당 발매의 디지털 배급 및 발매 소개에 사용하는 것을 허락합니다.' },
  composition: { title: '작사·작곡 및 커버곡 이용 허락서', grant: '아래 원곡 저작물을 해당 커버 녹음물의 제작 및 디지털 음원 배급에 사용하는 것을 기재한 범위에서 허락합니다.' },
  sample: { title: '샘플·제3자 음원 이용 허락서', grant: '아래 원본 녹음물·저작물을 해당 발매에 샘플로 사용하고 디지털 음원으로 배급하는 것을 기재한 범위에서 허락합니다.' },
  performer: { title: '피처링·실연자 이용 허락서', grant: '아래 참여 실연을 녹음하고 해당 발매를 디지털 음원 플랫폼에 배급·서비스하는 것을 허락합니다.' },
  shared: { title: '공동 권리자 배급 위임 확인서', grant: '아래 권리에 관해 기재한 지분·범위 안에서 해당 발매의 AUDENIQ 디지털 배급 신청을 위임합니다.' },
} as const;
export type RightsDocumentKind = keyof typeof RIGHTS_DOCUMENTS;
export const RIGHTS_SIGNER_ROLES = ['권리자 본인', '권리자의 위임을 받은 대리인'] as const;
export const RIGHTS_FORM = 'AUD-RIGHTS 1.0';

export interface ElectronicRightsInput {
  document_no: string;
  form: string;
  document_kind: RightsDocumentKind;
  rights_holder: string;
  signer_name: string;
  signer_role: string;
  signature: string;
  consent: boolean;
}
export interface ElectronicRightsRecord extends Omit<ElectronicRightsInput, 'signature' | 'consent'> {
  content_hash: string;
}
export interface RightsDocumentContext {
  releaseId: string; title: string; artist: string;
  tracks: { title: string; isrc: string }[];
  territories: string[]; platforms: string[];
  source?: string;
}

export function rightsDocumentBody(kind: RightsDocumentKind, context: RightsDocumentContext, input: {
  rightsHolder: string; source: string; scope: string; period: string; conditions: string;
}): string {
  return [
    RIGHTS_DOCUMENTS[kind].title, '',
    `서식: ${RIGHTS_FORM}`, `발매명: ${context.title.trim()}`, `아티스트: ${context.artist.trim()}`,
    `대상 트랙: ${context.tracks.map((t, i) => `${i + 1}. ${t.title}${t.isrc ? ` (ISRC ${t.isrc})` : ''}`).join(' / ')}`,
    `허락하는 권리자: ${input.rightsHolder.trim()}`, `대상 저작물·녹음물·참여 내용: ${input.source.trim()}`,
    '', '1. 이용 허락', RIGHTS_DOCUMENTS[kind].grant,
    `허락하는 권리·지분·사용 범위: ${input.scope.trim()}`,
    `배급 지역: ${context.territories.includes('WORLD') ? '전 세계' : context.territories.join(', ')}`,
    `배급 플랫폼: ${context.platforms.map(dspLabel).join(', ') || '배급 신청서에 지정하는 플랫폼'}`,
    `허락 기간: ${input.period.trim()}`, `추가 조건·대가: ${input.conditions.trim()}`,
    '', '2. 서명자의 확인',
    '서명자는 기재한 권리를 보유하거나 권리자로부터 이 허락서를 작성·서명할 권한을 위임받았음을 확인합니다.',
    '이 문서는 기재한 권리자의 허락 범위에 한합니다. 다른 권리자·권리의 허락이 필요한 경우 별도 문서를 작성합니다.',
    '서명 후 작성 내용과 서명 기록을 함께 보관하며, AUDENIQ의 권리 검토는 별도로 진행합니다.',
  ].join('\n');
}

/** 서버와 같은 정규 JSON 순서. 문서 본문·서명·권리자·서명자 정보를 함께 고정한다. */
export function rightsHashInput(title: string, body: string, e: ElectronicRightsInput): string {
  return JSON.stringify({ body: body.trim(), document_kind: e.document_kind, document_no: e.document_no,
    form: e.form, rights_holder: e.rights_holder.trim(), signature: e.signature,
    signer_name: e.signer_name.trim(), signer_role: e.signer_role, title: title.trim() });
}
