import{t as e}from"./react-Dlbx1jMz.js";import{O as t}from"./index-TnQibp8P.js";var n=e(),r=t(),i=[{id:`n1`,title:`AUDENIQ STUDIO 정식 서비스 안내`,date:`2026-09-26`,pinned:!0,body:`AUDENIQ STUDIO가 정식 서비스를 시작합니다.

이제 발매 접수부터 정산 확인까지 모든 과정을 스튜디오에서 진행할 수 있어요.

이용 중 궁금한 점은 문의 페이지에서 남겨주세요.`},{id:`n2`,title:`정산·지급 페이지 개편 안내`,date:`2026-09-26`,body:`정산·지급 페이지가 더 간결하게 바뀌었어요.

요청 전 잔액을 한눈에 확인하고, 정산 내역과 지급 요청 기록은 탭으로 나눠 볼 수 있습니다.
수령 계좌 정보도 상단에서 바로 확인하고 변경할 수 있어요.`},{id:`n3`,title:`9월 시스템 점검 안내 (완료)`,date:`2026-09-15`,body:`9월 15일 새벽에 진행된 시스템 점검이 완료됐습니다.

점검 시간: 2026-09-15 02:00 ~ 04:00 (KST)
영향: 점검 시간 중 발매 접수 일시 중단

이용에 불편을 드려 죄송합니다.`}];function a(){let[e,t]=(0,n.useState)(i[0]?.id??null),a=i.slice().sort((e,t)=>!!e.pinned==!!t.pinned?t.date.localeCompare(e.date):e.pinned?-1:1),o=a.filter(e=>e.pinned),s=a.filter(e=>!e.pinned);return(0,r.jsxs)(`div`,{id:`view-notices`,className:`view`,children:[(0,r.jsx)(`div`,{className:`view-title`,children:(0,r.jsxs)(`div`,{children:[(0,r.jsx)(`p`,{className:`eyebrow`,children:`NOTICES`}),(0,r.jsx)(`h1`,{children:`공지사항`}),(0,r.jsx)(`p`,{children:`꼭 알아야 할 소식과 업데이트를 전해 드려요.`})]})}),o.length>0&&(0,r.jsxs)(r.Fragment,{children:[(0,r.jsx)(`div`,{className:`aq-notice-featured-list`,children:o.map(n=>(0,r.jsxs)(`button`,{type:`button`,className:`aq-notice-featured`,onClick:()=>t(e===n.id?null:n.id),"aria-expanded":e===n.id,children:[(0,r.jsx)(`span`,{className:`aq-pin-badge`,children:`고정`}),(0,r.jsx)(`span`,{className:`aq-notice-featured-title`,children:n.title}),(0,r.jsx)(`span`,{className:`aq-notice-featured-date`,children:n.date}),e===n.id&&(0,r.jsx)(`span`,{className:`aq-notice-featured-body`,children:n.body.split(`
`).map((e,t)=>(0,r.jsxs)(`span`,{children:[e||`\xA0`,(0,r.jsx)(`br`,{})]},t))})]},n.id))}),(0,r.jsx)(`div`,{className:`section-top`,children:(0,r.jsx)(`h2`,{children:`전체 공지`})})]}),(0,r.jsx)(`div`,{className:`aq-notice-list`,children:s.map(n=>{let i=e===n.id;return(0,r.jsxs)(`div`,{className:`aq-notice-item`,children:[(0,r.jsxs)(`button`,{type:`button`,className:`aq-notice-head`,"aria-expanded":i,onClick:()=>t(i?null:n.id),children:[(0,r.jsxs)(`span`,{className:`min-0`,children:[(0,r.jsxs)(`span`,{className:`row-name`,children:[n.pinned&&(0,r.jsx)(`em`,{className:`aq-pin-badge`,children:`고정`}),n.title]}),(0,r.jsx)(`span`,{className:`row-sub`,children:n.date})]}),(0,r.jsx)(`span`,{className:`aq-chevron${i?` is-open`:``}`,"aria-hidden":`true`,children:`›`})]}),i&&(0,r.jsx)(`div`,{className:`aq-notice-body`,children:n.body.split(`
`).map((e,t)=>(0,r.jsx)(`p`,{children:e||`\xA0`},t))})]},n.id)})})]})}export{a as Notices};