// 배급 지역 — 신청서의 territories 값
//   ['WORLD']               전 세계
//   ['WORLD', '-JP', '-CN'] 전 세계에서 일본·중국 제외
//   ['KR', 'JP']            고른 지역에만
// 약관·법령상 배급이 제한된 국가는 어느 경우든 AUDENIQ가 송출에서 뺀다.

export const WORLD = 'WORLD';

/** 고를 수 있는 국가·지역 (ISO 3166-1 alpha-2) — 이름은 브라우저 한국어 이름을 쓴다 */
export const TERRITORY_GROUPS: [string, string[]][] = [
  ['아시아', ['KR', 'JP', 'CN', 'TW', 'HK', 'MO', 'MN', 'SG', 'MY', 'TH', 'VN', 'PH', 'ID', 'KH', 'LA', 'MM', 'BN', 'IN', 'PK', 'BD', 'LK', 'NP', 'KZ', 'UZ']],
  ['북미', ['US', 'CA', 'MX']],
  ['중남미', ['BR', 'AR', 'CL', 'CO', 'PE', 'EC', 'BO', 'PY', 'UY', 'VE', 'CR', 'PA', 'GT', 'HN', 'SV', 'NI', 'DO', 'PR', 'JM', 'TT']],
  ['유럽', ['GB', 'IE', 'FR', 'DE', 'NL', 'BE', 'LU', 'CH', 'AT', 'IT', 'ES', 'PT', 'SE', 'NO', 'DK', 'FI', 'IS', 'PL', 'CZ', 'SK', 'HU', 'RO', 'BG', 'GR', 'HR', 'SI', 'RS', 'UA', 'EE', 'LV', 'LT', 'TR']],
  ['중동·아프리카', ['AE', 'SA', 'QA', 'KW', 'BH', 'OM', 'IL', 'JO', 'LB', 'EG', 'MA', 'DZ', 'TN', 'NG', 'GH', 'KE', 'ZA', 'ET', 'TZ', 'UG', 'CI', 'SN']],
  ['오세아니아', ['AU', 'NZ', 'FJ', 'PG']],
];

let names: Intl.DisplayNames | null | undefined;
export function territoryName(code: string): string {
  if (code === WORLD) return '전 세계';
  if (names === undefined) {
    try { names = new Intl.DisplayNames(['ko'], { type: 'region' }); } catch { names = null; }
  }
  try { return names?.of(code) ?? code; } catch { return code; }
}

export interface TerritoryPlan { world: boolean; excluded: string[]; included: string[] }

export function parseTerritories(t: readonly string[] | undefined | null): TerritoryPlan {
  const list = t ?? [];
  const world = list.includes(WORLD);
  return {
    world,
    excluded: world ? list.filter(x => x.startsWith('-')).map(x => x.slice(1)) : [],
    included: world ? [] : list.filter(x => x && !x.startsWith('-')),
  };
}

export const worldExcept = (excluded: string[]): string[] => [WORLD, ...excluded.map(c => `-${c}`)];

/** 화면·문서용 한 줄 요약 */
export function territorySummary(t: readonly string[] | undefined | null, empty = '지정 안 함'): string {
  const p = parseTerritories(t);
  const join = (codes: string[]) => codes.map(territoryName).join(', ');
  if (p.world) return p.excluded.length ? `전 세계 (${join(p.excluded)} 제외)` : '전 세계';
  return p.included.length ? join(p.included) : empty;
}
