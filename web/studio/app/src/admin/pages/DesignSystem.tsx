// 디자인 시스템 — 스튜디오·관리자·업무 화면이 함께 쓰는 토큰과 컴포넌트를 한 화면에서 본다.
// 새 화면을 만들 때 여기 있는 한 벌만 쓰고, 없는 역할이 생기면 src/ui와 styles/ui.css에 더한다.
import { useState, type ReactNode } from 'react';
import { Glyph } from '../../components/Glyph';
import { CheckIcon } from '../../components/Check';
import {
  Avatar, Badge, Button, ButtonArrow, Callout, Card, ChipGroup, Count, Cover, EmptyState, Field, IconTile, KeyValue,
  PageHeader, Section, SectionHeader, Segmented, SkeletonList, StatGrid, StatTile, Tabs, Tag, type Tone,
} from '../../ui';

type View = 'tokens' | 'components' | 'decisions';

const TONES: Tone[] = ['blue', 'violet', 'amber', 'green', 'red', 'gray'];
const PALETTE: [string, string[]][] = [
  ['blue', ['25', '50', '100', '150', '200', '300', '400', '450', '500', '600', '700', '800']],
  ['gray', ['0', '25', '50', '75', '100', '150', '200', '300', '400', '500', '600', '700', '800', '900', '950']],
  ['green', ['50', '100', '500', '600', '700']],
  ['amber', ['50', '100', '500', '600', '700']],
  ['red', ['50', '100', '500', '600', '700']],
  ['violet', ['50', '100', '500', '600', '700']],
];
const SEMANTIC: [string, string][] = [
  ['--aq-text', '본문 잉크'], ['--aq-text-2', '두 번째 본문'], ['--aq-text-3', '보조 설명'], ['--aq-text-4', '메타 정보'],
  ['--aq-primary', '주 색'], ['--aq-primary-soft', '옅은 주 색'], ['--aq-surface', '카드·목록 면'], ['--aq-surface-2', '컨트롤 면'],
  ['--aq-field', '입력칸 면'], ['--aq-line', '구분선'], ['--aq-inverse', '어두운 면'],
];
const RADII: [string, string][] = [['xs', '태그'], ['sm', '작은 버튼'], ['md', '버튼·칩'], ['lg', '안쪽 칸'], ['xl', '목록 줄·숫자 칸'], ['2xl', '카드'], ['3xl', '큰 면']];
const TYPE: [string, string, string][] = [
  ['--aq-fs-h1', '화면 제목', '오늘의 업무'], ['--aq-fs-title-lg', '큰 제목', '발매 정보'], ['--aq-fs-title', '섹션 제목', '처리 이력'],
  ['--aq-fs-lg', '입력칸', '싱글 또는 앨범 제목'], ['--aq-fs-base', '본문·버튼', '신청서를 끝까지 작성해 주세요.'],
  ['--aq-fs-md', '설명', '숫자는 1분마다 새로 고쳐져요.'], ['--aq-fs-sm', '작은 버튼·보조', '접수 3일 전'], ['--aq-fs-xs', '배지·메타', '검토 중'],
];

const DECISIONS: [string, string, string, string][] = [
  ['버튼', 'button(48px·모서리 17) · adm-btn(40/46/48px) · link-btn · adm-link-btn · adm-ghost', 'Button · ui-btn — 변형 10 · 크기 4(32·40·48·56)', '같은 실행은 화면이 달라도 같은 모양. 높이는 8px 단위, 모서리는 높이에 비례. 옅은 카드·어두운 패널 위에서는 버튼이 면에 맞춰 스스로 진해지거나 반전돼요.'],
  ['상태 배지', 'status-chip(11px) · adm-chip(11.5px) · aq-doc-state-pill · pill', 'Badge — 6색조', '보완 필요는 앱 전체에서 주황, 거절만 빨강, 검토 중은 보라로 한 규칙. 서류 카드의 ‘보완 필요’가 빨강이던 것도 주황으로 맞췄어요.'],
  ['필터 칩', 'studio-filter · studio-alert-filter · adm-filter · adm-tl-filter(검정) · adm-pick · adm-reason · adm-templates', 'ChipGroup · ui-chip', '고른 칩은 파란 면, 개수는 옅은 숫자. 처리 이력 필터만 검정이던 것도 같은 파랑으로.'],
  ['고르기 토글', 'aq-seg(흰 알약) · adm-seg(파란 알약)', 'Segmented — 흰 알약', '파란 알약은 파란 필터 칩·주 버튼과 같은 색이라 한 줄에 셋이 겹치면 시끄러웠어요. 흰 알약은 ‘보기 바꾸기’로 분명히 구분돼요.'],
  ['탭', 'tabs/tab(14px·밑줄 고정) · adm-subtabs(15px·미끄러지는 밑줄)', 'Tabs · LinkTabs', '글자가 큰 쪽이 읽기 좋고, 미끄러지는 밑줄이 어디로 옮겼는지 보여 줘요.'],
  ['화면 머리', 'view-title(제목 39px) · adm-head(33px)', 'PageHeader — 28~36px', '스튜디오·관리자 제목 크기와 실행 버튼 정렬(아래 맞춤)을 하나로.'],
  ['섹션 머리', 'section-top(20px) · adm-section-top(18px) · subhead(20px)', 'SectionHeader — 18px', '보조 정보(작은 글자)와 오른쪽 실행 자리를 같은 규칙으로.'],
  ['카드·면', 'surface · card · adm-card · adm-card.white', 'Card — 옅은 파랑, 모서리 28', '윤곽선·그림자 없이 면 색으로만 구분하는 AUDENIQ 언어 그대로.'],
  ['숫자 칸', 'stat-card · adm-stat(숫자 먼저) · adm-w-stat', 'StatTile — [이름표·아이콘 / 숫자 / 설명]', '이름표를 먼저 두면 금액(이번 달 수익)과 건수(심사 대기)가 같은 순서로 읽혀요. 경고·오류는 색조로.'],
  ['목록 줄', 'adm-row · aq-release-card · aq-track-row · aq-ticket-card · aq-notice-card · aq-statement-card · aq-doc-row', 'ui-row 하나', '패딩 15~19px, 모서리 22~26px, 배경 2가지, 떠오르는 호버와 면 호버가 섞여 있었어요. 지금은 면 색만 바뀌는 한 가지.'],
  ['앨범 커버', '스튜디오: 발매별 그라데이션+음표 · 관리자: 같은 그라데이션+첫 글자', 'Cover — 발매별 그라데이션+음표', '목록에서 발매끼리 색으로 구분돼 훑어보기 쉬워요.'],
  ['입력칸', '테두리 선 · 밑줄 · 면(3가지 바탕색) · 관리자 면+모서리 18 · 포커스 3가지', 'ui-input · Field — 테두리 없는 면', '선 없는 AUDENIQ 회색 면(--aq-gray-100). 포커스는 윤곽선·그림자 없이 회색이 한 단계 진해지는 것 하나. 옅은 카드 안에서는 흰 칸이 포커스 때 회색이 돼요. 기본 48px, 아티스트용 큰 폼(신청서·프로필·로그인)은 56px.'],
  ['이름표', '13 · 14 · 15px, 굵기 650~750', 'Field 이름표 — 14px', '폼마다 다르던 이름표 크기를 하나로.'],
  ['알림 상자', 'notice · feedback · adm-alert', 'Callout — 안내·완료·주의·오류·중립', '오류는 빨간 면, 주의는 주황 면으로 같은 색 규칙.'],
  ['빈 상태', 'empty-page(아이콘 없음) · adm-empty(아이콘)', 'EmptyState — 아이콘 있는 쪽', '무엇이 비었는지 아이콘이 먼저 알려 줘요.'],
  ['선택 줄', 'adm-fix 안내 문구(칩 모양 재사용)', 'ui-choice (새 컴포넌트)', '긴 문장 중 하나를 고르는 건 칩이 아니라 줄 + 동그라미 표시가 맞아요.'],
  ['타임라인', 'aq-timeline(속 빈 점) · adm-timeline(색 있는 점)', 'ui-timeline', '점 색으로 담당자·검사·플랫폼·경고를 구분하는 쪽으로.'],
  ['체크 줄', 'check-line · adm-check-line', 'ui-check — 면 / 줄', '체크박스 크기는 공통 입력 요소(controls.css) 하나만.'],
  ['숫자 세기', 'CountUp 두 벌', 'CountUp 하나', '프레임마다 다시 그리지 않고, 스크린리더에 최종값을 알려 주는 쪽.'],
  ['앱 상단', '높이 74/70px · 로고 158/142px · STUDIO 표시는 오른쪽, ADMIN 표시는 로고 옆', '높이 72(모바일 64) · 로고 144 · 제품 표시는 로고 옆', '스튜디오와 관리자를 오가도 상단이 같은 자리에 있어요.'],
  ['히어로', '홈(장식 없음) · 관리자 개요(물방울 장식)', 'ui-hero — 장식 없는 면', '앞서 홈에서 정한 ‘단색 면으로 정돈’ 결정을 관리자에도.'],
  ['아바타', '41 · 32 · 30 · 80px, 모서리 제각각', 'Avatar — 5단계, 모서리는 크기의 34%', '어느 크기에서도 같은 둥글기.'],
  ['색', '고유 색 587개', '토큰 47개(그라데이션 장식 제외)', '비슷한 회색·파랑을 가장 가까운 토큰으로 모았어요.'],
  ['모서리', '2~36px 사이 30여 가지', '7단계 + 컨트롤 4단계', ''],
];

function Demo({ title, note, children, code }: { title: string; note?: ReactNode; children: ReactNode; code?: string }) {
  return (
    <Card className="ds-demo">
      <SectionHeader as="h3" title={title} meta={note} />
      <div className="ds-demo-body">{children}</div>
      {code && <pre className="ds-code"><code>{code}</code></pre>}
    </Card>
  );
}

function Tokens() {
  return (
    <>
      <Section title="색" meta="원색 단계 — 화면 CSS는 아래 의미 토큰을 먼저 써요">
        <div className="ds-palette">
          {PALETTE.map(([hue, steps]) => (
            <div key={hue} className="ds-ramp">
              <b>{hue}</b>
              <div>
                {steps.map(s => (
                  <span key={s} className="ds-swatch" style={{ background: `var(--aq-${hue}-${s})` }} title={`--aq-${hue}-${s}`}>
                    <small>{s}</small>
                  </span>
                ))}
              </div>
            </div>
          ))}
        </div>
      </Section>
      <Section title="의미 토큰" meta="역할 이름으로 쓴다">
        <div className="ds-semantic">
          {SEMANTIC.map(([name, label]) => (
            <div key={name}><span className="ds-chip-swatch" style={{ background: `var(${name})` }} /><b>{label}</b><code>{name}</code></div>
          ))}
        </div>
      </Section>
      <Section title="상태 색조" meta="배지·숫자 칸·아이콘 칸·알림이 같은 짝">
        <div className="ds-row">{TONES.map(t => <Badge key={t} tone={t}>{t}</Badge>)}</div>
      </Section>
      <Section title="글자" meta="Pretendard Variable (jsDelivr · 없으면 시스템 글꼴)">
        <div className="ds-type">
          {TYPE.map(([v, label, sample]) => (
            <div key={v}><small>{label} · <code>{v}</code></small><span style={{ fontSize: `var(${v})`, fontWeight: v.includes('h1') || v.includes('title') ? 780 : undefined, letterSpacing: v.includes('h1') || v.includes('title') ? '-.045em' : undefined }}>{sample}</span></div>
          ))}
        </div>
      </Section>
      <Section title="모서리" meta="7단계 · 누르는 요소는 높이에 비례">
        <div className="ds-radii">
          {RADII.map(([k, label]) => <div key={k}><span style={{ borderRadius: `var(--aq-r-${k})` }} /><b>{k}</b><small>{label}</small></div>)}
        </div>
      </Section>
      <Section title="높이" meta="누르는 요소는 8px 단위">
        <div className="ds-row is-end">
          {(['xs', 'sm', 'md', 'lg'] as const).map(s => <Button key={s} size={s} variant="primary">{({ xs: '32', sm: '40', md: '48', lg: '56' })[s]}px</Button>)}
        </div>
      </Section>
    </>
  );
}

function Components() {
  const [chip, setChip] = useState('all');
  const [seg, setSeg] = useState('list');
  const [tab, setTab] = useState('a');
  const [choice, setChoice] = useState(0);
  return (
    <div className="ds-grid">
      <Demo title="Button" note="primary는 화면에 하나" code={'<Button variant="primary">저장</Button>\n<Button size="sm">새로고침</Button>'}>
        <div className="ds-row">
          <Button variant="primary">주 실행</Button><Button>보조</Button><Button variant="neutral">중립</Button><Button variant="ghost">고스트</Button>
        </div>
        <div className="ds-row">
          <Button variant="warn">보완 요청</Button><Button variant="warn-solid">보완하기</Button><Button variant="danger">거절</Button><Button variant="danger-solid">긴급 점검</Button>
        </div>
        <div className="ds-row">
          <Button variant="primary" busy>저장 중</Button><Button disabled>비활성</Button><Button variant="text">글자 링크 <Glyph name="chevron-right" size={13} /></Button>
          <Button variant="primary">발매 등록하기<ButtonArrow /></Button>
        </div>
      </Demo>
      <Demo title="면에 따라 바뀌는 버튼" note="버튼에 따로 쓰지 않아요">
        <div className="ds-surfaces">
          <div className="ds-surface"><small>흰 바탕</small><div className="ds-row"><Button>보조</Button><Button variant="warn">주의</Button></div></div>
          <div className="ui-card is-tight"><small>옅은 카드 위</small><div className="ds-row"><Button>보조</Button><Button variant="warn">주의</Button></div></div>
          <div className="ui-dark ds-dark"><small>어두운 패널 위</small><div className="ds-row"><Button variant="primary">승인</Button><Button>보류</Button></div></div>
        </div>
      </Demo>
      <Demo title="Badge · Tag · Count" note="상태 · 정보 이름표 · 개수">
        <div className="ds-row">{TONES.map(t => <Badge key={t} tone={t} dot={t === 'red'}>{({ blue: '접수 대기', violet: '검토 중', amber: '보완 필요', green: '발매 완료', red: '긴급', gray: '작성 중' } as Record<Tone, string>)[t]}</Badge>)}</div>
        <div className="ds-row"><Tag>DSP</Tag><Tag mono>WRK-1043</Tag><Tag tone="blue">계약서</Tag><Count value={3} /><Count value={17} tone="hot" /><Count value={120} tone="alert" /></div>
      </Demo>
      <Demo title="ChipGroup" note="목록 거르기" code={'<ChipGroup label="상태" value={v} onChange={setV} options={[…]} />'}>
        <ChipGroup label="상태" value={chip} onChange={setChip} options={[{ value: 'all', label: '전체', count: 12 }, { value: 'draft', label: '작성 중', count: 3 }, { value: 'review', label: '검토 중', count: 4 }, { value: 'needs', label: '보완 필요', count: 0 }]} />
      </Demo>
      <Demo title="Segmented" note="보기 바꾸기 — 흰 알약">
        <div className="ds-row"><Segmented label="보기" value={seg} onChange={setSeg} options={[{ value: 'list', label: '목록' }, { value: 'board', label: '보드' }]} /><Segmented size="sm" label="정렬" value={seg} onChange={setSeg} options={[{ value: 'list', label: '오래된 순' }, { value: 'board', label: '최신 순' }]} /></div>
        <Segmented fill label="구분" value={seg} onChange={setSeg} options={[{ value: 'list', label: '정산 내역' }, { value: 'board', label: '지급 요청 기록' }]} />
      </Demo>
      <Demo title="Tabs" note="하위 화면 — 미끄러지는 밑줄">
        <Tabs label="예시" value={tab} onChange={setTab} items={[{ value: 'a', label: '업무함', count: 17 }, { value: 'b', label: '업무' }, { value: 'c', label: '예외 처리', count: 7 }]} />
      </Demo>
      <Demo title="PageHeader · SectionHeader">
        <div className="ds-mini">
          <PageHeader eyebrow="업무" title="업무함" sub="나에게 온 일을 모아 보여 줘요." actions={<Button size="sm">새로고침</Button>} />
          <SectionHeader title="처리 이력" meta="7건" action={<Button size="sm">전체 보기</Button>} />
        </div>
      </Demo>
      <Demo title="StatTile" note="[이름표·아이콘 / 숫자 / 설명]">
        <StatGrid cols={3}>
          <StatTile label="심사 대기" value={4} hint="판단이 필요한 발매" icon={<IconTile size="sm"><Glyph name="review" size={18} /></IconTile>} onClick={() => {}} />
          <StatTile label="배급 문제" value={1} hint="전송이 막힌 플랫폼" tone="alert" icon={<IconTile size="sm"><Glyph name="truck" size={18} /></IconTile>} onClick={() => {}} />
          <StatTile label="이번 달 수익" value="27,118" unit="원" hint="전월 대비 +133.6%" />
        </StatGrid>
      </Demo>
      <Demo title="목록 줄 (ui-row)" note="카드 목록 · 카드 안 목록(is-inset)">
        <div className="ui-list">
          <a className="ui-row" href="#row" onClick={e => e.preventDefault()}>
            <Cover id="r1a2" text="새벽의 온도" />
            <span className="ui-row-body"><b className="ui-row-title">새벽의 온도</b><span className="ui-row-meta"><span>한결</span><span>싱글</span><span>발매 예정 10월 17일</span></span></span>
            <span className="ui-row-end"><Badge tone="violet">검사 판단 필요</Badge><small>접수 26시간 전</small></span>
          </a>
          <div className="ui-row is-warn">
            <IconTile tone="amber"><Glyph name="alert" size={18} /></IconTile>
            <span className="ui-row-body"><b className="ui-row-title">보완이 필요한 발매 1건</b><span className="ui-row-meta"><span>커버아트를 고쳐 주세요</span></span></span>
            <span className="ui-row-end"><Button size="xs" variant="white">보완</Button></span>
          </div>
        </div>
      </Demo>
      <Demo title="Avatar · Cover · IconTile">
        <div className="ds-row is-end"><Avatar name="kim" size="xs" /><Avatar name="kim" size="sm" /><Avatar name="kim" /><Avatar me size="lg" /><Avatar empty size="lg" /><Avatar name="서린" size="xl" /></div>
        <div className="ds-row is-end"><Cover id="a" text="A" /><Cover id="b" text="B" size="lg" /><Cover text="권리 증빙" plain /><IconTile size="sm"><Glyph name="doc" size={16} /></IconTile><IconTile tone="green"><CheckIcon size={18} /></IconTile><IconTile tone="violet" size="lg"><Glyph name="pencil" size={22} /></IconTile></div>
      </Demo>
      <Demo title="입력칸 · Field" note="회색 면, 포커스 때 한 단계 진하게 — 윤곽선 없음">
        <Field label="발매 제목" htmlFor="dsTitle" required help="싱글 또는 앨범 제목"><input id="dsTitle" className="ui-input" placeholder="제목을 입력해 주세요" /></Field>
        <div className="ui-form-grid">
          <Field label="장르" htmlFor="dsGenre"><select id="dsGenre" className="ui-select"><option>인디 팝</option><option>발라드</option></select></Field>
          <Field label="찾기" htmlFor="dsSearch" error="두 글자 이상 입력해 주세요."><input id="dsSearch" type="search" className="ui-input" aria-invalid="true" placeholder="이름으로 찾기" /></Field>
        </div>
        <div className="ds-row"><input className="ui-input is-sm" aria-label="작은 입력칸" placeholder="도구줄 40px" style={{ maxWidth: 220 }} /><input className="ui-input is-lg" aria-label="큰 입력칸" placeholder="아티스트용 56px" style={{ maxWidth: 260 }} /></div>
      </Demo>
      <Demo title="선택 줄 · 체크 줄">
        <div className="ui-choices" role="radiogroup" aria-label="안내 문구">
          {['3000×3000 이상 정사각형 이미지로 다시 올려 주세요.', '커버에 다른 서비스 로고가 들어 있어요.'].map((t, i) => (
            <button key={t} type="button" role="radio" aria-checked={choice === i} className="ui-choice" onClick={() => setChoice(i)}><span>{t}</span><span className="ui-choice-mark" aria-hidden="true">{choice === i && <CheckIcon size={11} />}</span></button>
          ))}
        </div>
        <label className="ui-check"><input type="checkbox" defaultChecked /> <span>권리 확인 항목을 모두 확인했어요.<small>담당자 결정과 함께 감사 기록에 남아요.</small></span></label>
      </Demo>
      <Demo title="Callout" note="안내 · 완료 · 주의 · 오류 · 중립">
        <Callout>지급 요청 <b>2건</b>이 운영 처리를 기다리고 있어요.</Callout>
        <Callout tone="success">서명이 끝났어요.</Callout>
        <Callout tone="warn" action={<Button size="xs" variant="warn">보완하기</Button>}>보완 요청 2건이 있어요.</Callout>
        <Callout tone="danger" role="status">목록을 불러오지 못했어요.</Callout>
      </Demo>
      <Demo title="EmptyState · Skeleton">
        <EmptyState compact title="처리할 심사가 없어요">새 발매 신청이 들어오면 여기에 보여요.</EmptyState>
        <SkeletonList rows={2} />
      </Demo>
      <Demo title="Timeline · KeyValue">
        <ol className="ui-timeline">
          <li className="is-staff"><b>담당자 승인</b><small>10:20 · kim</small></li>
          <li className="is-warn"><b>크레딧 확인 필요</b><small>09:35 · 검사</small></li>
          <li className="is-check"><b>1차 검사 통과</b><small>09:12 · 시스템</small></li>
        </ol>
        <KeyValue cols={3} items={[['아티스트', '한결'], ['발매 유형', '싱글'], ['UPC', '발급 전']]} />
      </Demo>
    </div>
  );
}

function Decisions() {
  return (
    <Section title="같은 역할, 한 벌로" meta={`${DECISIONS.length}가지`}>
      <p className="ds-lead">같은 일을 하는 요소를 나란히 놓고, 더 보기 좋고 더 쓰기 편한 쪽을 남겼어요. 남은 쪽은 디자인 시스템 컴포넌트가 됐고, 나머지는 지웠어요.</p>
      <div className="ds-table-wrap">
        <table className="ds-table">
          <thead><tr><th>역할</th><th>전에는</th><th>지금은</th><th>이유</th></tr></thead>
          <tbody>
            {DECISIONS.map(([role, before, after, why]) => (
              <tr key={role}><th scope="row">{role}</th><td>{before}</td><td><b>{after}</b></td><td>{why}</td></tr>
            ))}
          </tbody>
        </table>
      </div>
    </Section>
  );
}

export function DesignSystem() {
  const [view, setView] = useState<View>('components');
  return (
    <div className="view-enter ds-page">
      <PageHeader
        eyebrow="시스템"
        title="디자인 시스템"
        sub="스튜디오·관리자·업무 화면이 함께 쓰는 토큰과 컴포넌트예요. 같은 역할의 요소는 여기 있는 한 벌만 써요."
      />
      <Tabs label="디자인 시스템" value={view} onChange={setView} items={[{ value: 'components', label: '컴포넌트' }, { value: 'tokens', label: '토큰' }, { value: 'decisions', label: '통일 기록', count: DECISIONS.length }]} />
      <div className="ui-tab-panel" key={view}>
        {view === 'tokens' ? <Tokens /> : view === 'components' ? <Components /> : <Decisions />}
      </div>
    </div>
  );
}
