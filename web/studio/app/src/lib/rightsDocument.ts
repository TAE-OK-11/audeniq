import { dspLabel } from './catalog';

// 권리 서류 서식 AUD-RIGHTS 2.0 — 권리자 본인이 본인확인 후 직접 서명하는 전자 문서.
// 문안은 일반적인 이용 허락·위임 계약의 필수 요소(당사자, 대상, 범위, 기간, 대가, 보증, 철회,
// 전자서명 합의, 사본·보관, 준거법)를 갖추도록 썼다. 실제 사용 전 법률 전문가의 검토를 권한다.
export const RIGHTS_DOCUMENTS = {
  master: { title: '마스터 음원 배급 이용 허락서', object: '마스터 녹음물', grant: '허락자는 아래 마스터 녹음물을 이용자가 AUDENIQ를 통해 디지털 음원 플랫폼에 복제·배포·전송(스트리밍·다운로드)하고 서비스하는 것을 허락합니다.' },
  artwork: { title: '커버아트 이용 허락서', object: '커버아트(사진·일러스트·디자인·글꼴 포함)', grant: '허락자는 아래 커버아트를 해당 발매의 디지털 배급과 플랫폼 내 발매 소개·홍보에 복제·전송·게시하는 것을 허락합니다.' },
  composition: { title: '작사·작곡 및 커버곡 이용 허락서', object: '원곡 저작물(작사·작곡·편곡)', grant: '허락자는 아래 원곡 저작물을 해당 녹음물의 제작(편곡·녹음)과 디지털 음원 배급에 이용하는 것을 기재한 범위에서 허락합니다. 저작권 신탁단체에 신탁된 권리의 사용료 정산은 해당 단체의 규정에 따릅니다.' },
  sample: { title: '샘플·제3자 음원 이용 허락서', object: '원본 녹음물·저작물(샘플 구간 포함)', grant: '허락자는 아래 원본 녹음물·저작물의 기재한 구간을 해당 발매에 샘플로 사용하고, 그 결과물을 디지털 음원으로 배급하는 것을 기재한 범위에서 허락합니다.' },
  performer: { title: '피처링·실연자 이용 허락서', object: '실연(가창·연주·피처링)', grant: '허락자는 아래 실연을 녹음·복제하고 그 녹음물을 디지털 음원 플랫폼에 배포·전송하는 것을 허락하며, 플랫폼 표기에 실연자 이름을 크레딧으로 표시하는 데 동의합니다.' },
  shared: { title: '공동 권리자 배급 위임 확인서', object: '공동 보유 권리(지분)', grant: '위임인은 아래 권리 중 위임인의 지분에 관하여, 해당 발매의 AUDENIQ 디지털 배급 신청·메타데이터 등록·플랫폼 공급·내림 요청을 수임인이 대신 진행하는 것을 위임합니다.' },
} as const;
export type RightsDocumentKind = keyof typeof RIGHTS_DOCUMENTS;
export const RIGHTS_SIGNER_ROLES = ['권리자 본인', '권리자의 위임을 받은 대리인', '법인 대표자'] as const;
export const RIGHTS_FORM = 'AUD-RIGHTS 2.0';

/** 서명 요청의 전달 방법 — 링크를 보내거나, 같은 기기를 권리자에게 건네서 */
export type SigningChannel = 'LINK' | 'IN_PERSON';

/** 서버가 문서에 남기는 기록 (1.0은 예전 직접 서명, 2.0은 본인확인 후 서명) */
export interface ElectronicRightsRecord {
  document_no: string;
  form: string;
  document_kind: RightsDocumentKind;
  rights_holder: string;
  signer_name: string;
  signer_role: string;
  content_hash: string;
  signing_request_id?: string;
  channel?: SigningChannel;
  identity?: { provider: string; method: string; name: string; verified_at: string };
  certificate_hash?: string;
}
export interface RightsDocumentContext {
  releaseId: string; title: string; artist: string;
  tracks: { title: string; isrc: string }[];
  territories: string[]; platforms: string[];
  source?: string;
}

/** 서명 화면에서 받는 세 가지 동의 */
export const SIGNING_CONSENTS = {
  document: '위 문서의 내용을 끝까지 읽고 확인했으며, 기재한 권리자 본인(또는 권한 있는 대리인·대표자)으로서 서명합니다.',
  electronic_signature: '이 문서를 전자문서로 작성하고 전자서명으로 체결하는 데 동의합니다. 전자서명은 자필 서명과 같은 효력을 가지는 것으로 합의합니다.',
  privacy: '서명자 본인확인과 서명 증빙을 위한 개인정보 수집·이용에 동의합니다.',
} as const;
export type SigningConsentKey = keyof typeof SIGNING_CONSENTS;

/** 개인정보 수집·이용 안내 (개인정보 보호법 제15조 제2항의 고지 사항) */
export const SIGNING_PRIVACY_NOTICE = [
  '수집 항목: 이름, 생년월일, 본인확인 결과(인증기관 거래번호, 연계정보(CI)의 암호화된 값), 서명 이미지, 서명 시각, 접속 정보(IP 주소의 암호화된 값, 브라우저 정보)',
  '이용 목적: 서명자 본인 확인, 전자서명과 문서 체결 사실의 증빙, 권리 검토와 분쟁 대응',
  '제공: 문서를 요청한 발매 신청인에게 서명자 이름·본인확인 완료 시각·서명본이 제공돼요. 생년월일과 연계정보는 제공하지 않아요.',
  '보유 기간: 해당 발매의 배급 계약이 끝난 날부터 5년(관계 법령에서 더 길게 정한 경우 그 기간)',
  '동의를 거부할 수 있어요. 거부하면 전자 서명은 할 수 없고, 서면 허락서를 발매 신청인에게 전달하는 방법으로 대신할 수 있어요.',
];

export function rightsDocumentBody(kind: RightsDocumentKind, context: RightsDocumentContext, input: {
  documentNo: string; rightsHolder: string; signerName: string; signerRole: string;
  source: string; scope: string; period: string; conditions: string;
}): string {
  const d = RIGHTS_DOCUMENTS[kind];
  const shared = kind === 'shared';
  const giver = shared ? '위임인' : '허락자';
  const taker = shared ? '수임인' : '이용자';
  const artist = context.artist.trim() || '발매 신청인';
  const signer = input.signerRole === RIGHTS_SIGNER_ROLES[0]
    ? `${input.signerName.trim()} (권리자 본인)`
    : `${input.signerName.trim()} (${input.signerRole}, 권리자: ${input.rightsHolder.trim()})`;
  return [
    d.title, '',
    `서식 ${RIGHTS_FORM} · 문서번호 ${input.documentNo}`,
    '',
    '[당사자]',
    `${giver}(권리자): ${input.rightsHolder.trim()}`,
    `서명자: ${signer}`,
    `${taker}: ${artist} (해당 발매의 배급 신청인)`,
    '배급 대행: AUDENIQ (이용자의 신청에 따라 디지털 음원 플랫폼에 공급)',
    '',
    '[대상]',
    `발매명: ${context.title.trim()}`,
    `아티스트: ${artist}`,
    `대상 트랙: ${context.tracks.map((t, i) => `${i + 1}. ${t.title}${t.isrc ? ` (ISRC ${t.isrc})` : ''}`).join(' / ') || '해당 발매의 전체 트랙'}`,
    `대상 ${d.object}: ${input.source.trim()}`,
    '',
    `제1조 (${shared ? '위임' : '이용 허락'})`,
    d.grant,
    `${shared ? '위임하는 지분·범위' : '허락하는 권리·지분·사용 범위'}: ${input.scope.trim()}`,
    `배급 지역: ${context.territories.includes('WORLD') ? '전 세계' : context.territories.join(', ') || '배급 신청서에 지정한 지역'}`,
    `배급 플랫폼: ${context.platforms.map(dspLabel).join(', ') || '배급 신청서에 지정한 플랫폼'}`,
    '',
    '제2조 (기간)',
    `${shared ? '위임' : '허락'} 기간: ${input.period.trim()}`,
    '기간이 끝나거나 제5조에 따라 철회되면 이용자는 플랫폼에 내림(테이크다운)을 요청합니다. 내림이 처리되기 전까지 이미 공급된 음원의 서비스와 그 기간의 정산은 이 문서에 따른 것으로 봅니다.',
    '',
    '제3조 (대가와 조건)',
    input.conditions.trim(),
    '여기에 따로 적지 않은 대가는 없는 것으로 합니다. 저작권 신탁단체가 징수·분배하는 사용료는 이 문서와 별개로 해당 단체의 규정에 따릅니다.',
    '',
    '제4조 (권리 보증)',
    `${giver}는 제1조의 권리를 적법하게 보유하거나 처분할 권한이 있고, 이 문서가 제3자의 권리를 침해하지 않으며, 같은 권리에 관해 이와 충돌하는 독점 계약을 맺지 않았음을 보증합니다.`,
    '대리인 또는 법인 대표자가 서명하는 경우, 서명자는 권리자로부터 이 문서를 체결할 권한을 받았음을 보증하고, 요청받으면 위임장·등기사항증명서 등 권한을 증명하는 서류를 제출합니다.',
    `보증이 사실과 다른 경우 ${giver}는 그로 인해 ${taker}와 AUDENIQ에 생긴 손해를 배상합니다.`,
    '',
    '제5조 (철회)',
    `${giver}는 ${taker}에게 서면(전자문서 포함)으로 알려 장래를 향해 ${shared ? '위임' : '허락'}을 철회할 수 있습니다. ${taker}는 알림을 받은 날부터 14일 안에 플랫폼 내림을 요청합니다. 제3조에서 철회에 관해 따로 정한 경우에는 그에 따릅니다.`,
    '',
    '제6조 (전자문서와 전자서명)',
    '당사자는 이 문서를 전자문서로 작성하고 전자서명으로 체결하는 데 합의합니다. 이 문서와 전자서명은 전자적 형태라는 이유만으로 효력이 부인되지 않습니다(「전자문서 및 전자거래 기본법」 제4조, 「전자서명법」 제3조).',
    '서명자는 본인확인 기관을 통해 본인임을 확인한 뒤 직접 서명하며, 본인확인 결과, 서명 시각, 접속 기록, 문서와 서명의 해시값이 서명 기록으로 함께 보관됩니다. 서명 후 문서 내용은 바꿀 수 없고, 고쳐야 하면 새 문서를 작성합니다.',
    '',
    '제7조 (사본과 보관)',
    '서명이 끝나면 서명자는 서명 링크에서 서명본과 서명 기록을 확인·저장·인쇄할 수 있습니다. 서명본은 해당 발매의 배급 계약이 끝난 날부터 5년간(관계 법령에서 더 길게 정한 경우 그 기간) 보관됩니다.',
    '',
    '제8조 (그 밖의 사항)',
    '이 문서는 기재한 권리자와 권리의 범위에만 적용됩니다. 다른 권리자나 권리의 허락이 필요하면 별도 문서를 작성합니다. AUDENIQ의 권리 검토는 이 문서와 별도로 진행됩니다.',
    '이 문서는 대한민국 법을 따르며, 이 문서로 생긴 분쟁은 「민사소송법」에 따른 관할 법원에서 해결합니다.',
  ].join('\n');
}

/** 서버와 같은 정규 JSON 순서(키 사전순). 문서 본문·서명·권리자·서명자 정보를 함께 고정한다 (1.0·2.0 공통). */
export function rightsHashInput(title: string, body: string, e: {
  document_no: string; form: string; document_kind: string; rights_holder: string;
  signer_name: string; signer_role: string; signature: string;
}): string {
  return JSON.stringify({ body: body.trim(), document_kind: e.document_kind, document_no: e.document_no,
    form: e.form, rights_holder: e.rights_holder.trim(), signature: e.signature,
    signer_name: e.signer_name.trim(), signer_role: e.signer_role, title: title.trim() });
}
