// 관리 단체에 가입하지 않은 작가에게 — 무엇을 못 받는지와 어디서 가입하는지 한 상자로.
// 계정 정보의 퍼블리싱 카드, 신청서의 권리 확인, 지분 편집기의 내 줄이 같은 말을 쓴다
import { SOCIETY_JOIN } from '../../lib/publishing';
import { Callout } from '../../ui';

/** inline: 카드 안에서는 상자 없이 도움말 한 줄로 (카드 안에 카드를 두지 않는다) */
export function SocietyJoin({ inline = false }: { inline?: boolean }) {
  const where = <>{SOCIETY_JOIN.map(([name, url], i) => <span key={url}>{i > 0 && ' · '}<a className="aq-inline-link" href={url} target="_blank" rel="noopener noreferrer">{name}</a></span>)}</>;
  if (inline) return <p className="ui-field-help aq-pbl-join">관리 단체 회원이어야 방송·공연의 작가 몫을 받아요. 가입은 {where}에서 하고, 가입하면 IPI 번호를 받아요.</p>;
  return (
    <Callout tone="neutral">
      <b>관리 단체에 가입하지 않았어요.</b> 방송·공연에서 생기는 작가 몫은 관리 단체 회원만 받아요. 가입 전에도 신청할 수 있고, 퍼블리셔 몫은 AUDENIQ이 받아 드려요.
      {' '}가입은 {where}에서 해요. 가입하면 IPI 번호를 받아요.
    </Callout>
  );
}
