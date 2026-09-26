import{r as x,j as e}from"./index-CbHDp4nS.js";const r=[{id:"n1",title:"AUDENIQ STUDIO 정식 서비스 안내",date:"2026-09-26",pinned:!0,body:`AUDENIQ STUDIO가 정식 서비스를 시작합니다.

이제 발매 접수부터 정산 확인까지 모든 과정을 스튜디오에서 진행할 수 있어요.

이용 중 궁금한 점은 문의 페이지에서 남겨주세요.`},{id:"n2",title:"정산·지급 페이지 개편 안내",date:"2026-09-26",body:`정산·지급 페이지가 더 간결하게 바뀌었어요.

요청 전 잔액을 한눈에 확인하고, 정산 내역과 지급 요청 기록은 탭으로 나눠 볼 수 있습니다.
수령 계좌 정보도 상단에서 바로 확인하고 변경할 수 있어요.`},{id:"n3",title:"9월 시스템 점검 안내 (완료)",date:"2026-09-15",body:`9월 15일 새벽에 진행된 시스템 점검이 완료됐습니다.

점검 시간: 2026-09-15 02:00 ~ 04:00 (KST)
영향: 점검 시간 중 발매 접수 일시 중단

이용에 불편을 드려 죄송합니다.`}];function m(){var c;const[i,d]=x.useState(((c=r[0])==null?void 0:c.id)??null),t=r.slice().sort((s,n)=>!!s.pinned!=!!n.pinned?s.pinned?-1:1:n.date.localeCompare(s.date)),l=t.filter(s=>s.pinned),o=t.filter(s=>!s.pinned);return e.jsxs("div",{id:"view-notices",className:"view",children:[e.jsx("div",{className:"view-title",children:e.jsxs("div",{children:[e.jsx("p",{className:"eyebrow",children:"NOTICES"}),e.jsx("h1",{children:"공지사항"}),e.jsx("p",{children:"꼭 알아야 할 소식과 업데이트를 전해 드려요."})]})}),l.length>0&&e.jsxs(e.Fragment,{children:[e.jsx("div",{className:"aq-notice-featured-list",children:l.map(s=>e.jsxs("button",{type:"button",className:"aq-notice-featured",onClick:()=>d(i===s.id?null:s.id),"aria-expanded":i===s.id,children:[e.jsx("span",{className:"aq-pin-badge",children:"고정"}),e.jsx("span",{className:"aq-notice-featured-title",children:s.title}),e.jsx("span",{className:"aq-notice-featured-date",children:s.date}),i===s.id&&e.jsx("span",{className:"aq-notice-featured-body",children:s.body.split(`
`).map((n,a)=>e.jsxs("span",{children:[n||" ",e.jsx("br",{})]},a))})]},s.id))}),e.jsx("div",{className:"section-top",children:e.jsx("h2",{children:"전체 공지"})})]}),e.jsx("div",{className:"aq-notice-list",children:o.map(s=>{const n=i===s.id;return e.jsxs("div",{className:"aq-notice-item",children:[e.jsxs("button",{type:"button",className:"aq-notice-head","aria-expanded":n,onClick:()=>d(n?null:s.id),children:[e.jsxs("span",{className:"min-0",children:[e.jsxs("span",{className:"row-name",children:[s.pinned&&e.jsx("em",{className:"aq-pin-badge",children:"고정"}),s.title]}),e.jsx("span",{className:"row-sub",children:s.date})]}),e.jsx("span",{className:`aq-chevron${n?" is-open":""}`,"aria-hidden":"true",children:"›"})]}),n&&e.jsx("div",{className:"aq-notice-body",children:s.body.split(`
`).map((a,p)=>e.jsx("p",{children:a||" "},p))})]},s.id)})})]})}export{m as Notices};
