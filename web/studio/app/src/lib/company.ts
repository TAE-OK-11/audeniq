/** 계약서·신청서에 찍는 회사(AUDENIQ) 정보.
 *  비어 있는 칸은 서류에 나오지 않는다 — 법인 정보가 확정되면 여기만 채우면 된다.
 *  직인(sealSrc): 배경이 투명한 PNG를 public/static/ 아래에 두고 그 경로를 적는다 (예: 'static/company-seal.png').
 *  직인은 담당자가 계약서를 승인한 뒤에만 회사 서명란에 찍힌다. */
export const COMPANY = {
  name: '주식회사 AUDENIQ',
  /** 대표이사 이름 */
  ceo: '',
  /** 사업자등록번호 (000-00-00000) */
  bizNo: '',
  /** 본점 주소 */
  address: '',
  /** 직인 이미지 경로 (BASE_URL 기준) */
  sealSrc: '',
};

/** 계약 당사자 표의 '회사' 줄 아래에 덧붙일 법인 정보 (채워진 것만) */
export function companyRows(): [string, string][] {
  return ([['대표이사', COMPANY.ceo], ['사업자등록번호', COMPANY.bizNo], ['주소', COMPANY.address]] as [string, string][])
    .filter(([, v]) => v.trim());
}

export const companySeal = () => (COMPANY.sealSrc ? `${import.meta.env.BASE_URL}${COMPANY.sealSrc.replace(/^\//, '')}` : '');
