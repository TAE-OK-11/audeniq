import { initializeMotion, animateSurveyStep, animateSurveyComplete } from './motion.js?v=5';
const form = document.querySelector('#survey-form');
const steps = [...document.querySelectorAll('.step')];
const progress = document.querySelector('#progress');
const progressFill = document.querySelector('#progress-fill');
const progressNow = document.querySelector('#progress-now');
const nextButton = document.querySelector('#next-btn');
const prevButton = document.querySelector('#prev-btn');
const submitButton = document.querySelector('#submit-btn');
const errorBox = document.querySelector('#form-error');
const contact = document.querySelector('#contact');
const betaConsent = document.querySelector('#beta_contact_consent');
const surveyConsent = document.querySelector('#survey_consent');
const securityStatus = document.querySelector('#security-status');
const receipt = document.querySelector('#receipt-id');
let current = 0;
let turnstileToken = '';
let widgetId = null;
let serviceReady = false;
let siteKey = '';
let turnstileLoading = false;
let busy = false;
const submissionId = crypto.randomUUID();
const FIRST = 1;
const LAST = 18;
const MULTI = new Set(['q1','q4','q6','q7','q11','q15']);

function setError(message) {
  errorBox.textContent = message;
  errorBox.hidden = !message;
  if (message) errorBox.scrollIntoView({block:'nearest',behavior:'smooth'});
}
function selected(name) {
  return [...form.querySelectorAll(`[name="${name}"]:checked`)].map(item=>item.value);
}
// Keep UI synchronized with the actual native input state after selecting,
// deselecting, exclusive-option clearing, browser history and page restoration.
function syncChoiceVisual(block) {
  if (!block) return;
  block.querySelectorAll('.choice').forEach(choice=>{
    const input=choice.querySelector('input[type="checkbox"],input[type="radio"]');
    if (input) choice.classList.toggle('is-selected',input.checked);
  });
  const count=block.querySelector('.selection-count');
  if (count) {
    const n=block.querySelectorAll('.choices input:checked').length;
    count.textContent=n ? `${n}개 선택됨` : '0개 선택';
    count.classList.toggle('has-selection',n>0);
  }
}
function syncAllChoices() {
  form.querySelectorAll('[data-question]').forEach(syncChoiceVisual);
}
function noDistributorExperience(){
  return ['2','3','4'].includes(selected('q2')[0]) || selected('q4').includes('9');
}
function refreshExperienceBranch(){
  const skip=noDistributorExperience();
  for(const key of ['q5','q6','q12']){
    const block=document.querySelector(`#question-${key}`);
    if(!block)return;
    block.hidden=skip;
    block.querySelectorAll('input').forEach(input=>{
      if(skip){if(input.type==='checkbox'||input.type==='radio')input.checked=false;
        else input.value='';}
      input.disabled=skip;
    });
    if(skip)syncChoiceVisual(block);
  }
}
function showStep(index) {
  refreshExperienceBranch();
  current=index;
  steps.forEach((section,i)=>section.hidden=i!==index);
  progressNow.textContent=String(index+1).padStart(2,'0');
  progress.setAttribute('aria-valuenow',String(index+1));
  progressFill.style.width=`${((index+1)/steps.length)*100}%`;
  prevButton.hidden=index===0;
  nextButton.hidden=index===steps.length-1;
  submitButton.hidden=index!==steps.length-1;
  setError('');
  updateSubmitReady();
  if (index===steps.length-1) void renderTurnstile();
  animateSurveyStep(steps[index]);
  steps[index].querySelector('h2').focus({preventScroll:true});
  document.querySelector('.form-shell').scrollIntoView({block:'start',behavior:'smooth'});
}
function validateStep(index) {
  for(const block of steps[index].querySelectorAll('[data-question]')) {
    const key=block.dataset.question;
    if (key==='q14' || block.hidden) continue;
    if (key==='beta') {
      const v=selected('beta')[0] ?? ''; 
      const c=contact.value.trim();
      if (c && (v==='3'||v==='')) return '연락처를 입력하려면 베타 참여 또는 출시 안내 항목을 선택해 주세요.';
      if (c && !/^(?:[^\s@]+@[^\s@]+\.[^\s@]+|@[A-Za-z0-9._]{1,30})$/.test(c)) return '이메일 주소 또는 @로 시작하는 Instagram 계정을 입력해 주세요.';
      if (c && !betaConsent.checked) return '연락처를 입력했다면 연락처 수집·이용에 동의해 주세요.';
      if (!c && betaConsent.checked) return '연락처 동의 체크를 해제하거나 연락처를 입력해 주세요.';
      if (!surveyConsent.checked) return '설문 응답 수집·이용에 동의해 주세요.';
      continue;
    }
    const s=selected(key);
    if(key==='q3' && ['2','3','4'].includes(selected('q2')[0]) && s[0]!=='6')return '발매 경험이 없다면 ‘아직 정식 음원을 발매한 경험이 없음’을 선택해 주세요.';
    if(key==='q3' && ['0','1'].includes(selected('q2')[0]) && s[0]==='6')return '발매 경험이 있다면 최근 12개월간 발매 여부에 맞춰 응답해 주세요.';
    if (block.dataset.required==='true' && !s.length) return `${key.toUpperCase()} 필수 문항에 응답해 주세요.`;
    if (key==='q15' && s.includes('13') && s.length>1)return '‘잘 모르겠다’는 다른 플랫폼과 함께 선택할 수 없어요.';
    if (block.dataset.max && s.length>Number(block.dataset.max)) return `${key.toUpperCase()}는 최대 ${block.dataset.max}개까지 선택할 수 있어요.`;
    const other=block.querySelector('.other-field');
    if (other && !other.hidden && !other.querySelector('input').value.trim()) return `${key.toUpperCase()}의 기타 내용을 적어 주세요.`;
  }
  return '';
}
function updateSubmitReady(){submitButton.disabled = busy || !serviceReady || !turnstileToken || !surveyConsent.checked;}
function handleChoice(input) {
  const block=input.closest('[data-question]');
  if(!block) return;
  const key=block.dataset.question;
  const max=Number(block.dataset.max||0);
  if (input.type==='checkbox') {
    // 'No distributor' and 'No issues' exclude all other choices.
    const exclusive=key==='q4'?'9':key==='q6'?'18':key==='q15'?'13':null;
    if(exclusive!==null && input.checked) {
      if(input.value===exclusive) block.querySelectorAll('input[type=checkbox]').forEach(el=>{if(el!==input)el.checked=false;});
      else block.querySelector(`input[value="${exclusive}"]`).checked=false;
    }
    if(max && selected(key).length>max) {input.checked=false;setError(`이 문항은 최대 ${max}개까지 선택할 수 있어요.`);} else setError('');
  }
  const other=block.querySelector('.other-field');
  if(other) {
    // Last option in every source question that has an other option.
    const otherChoice=[...block.querySelectorAll('.choices input')].at(-1);
    other.hidden=!otherChoice.checked;
    other.querySelector('input').disabled=other.hidden;
    if(other.hidden) other.querySelector('input').value='';
  }
  syncChoiceVisual(block);
  if(key==='q2') {
    const noRelease=['2','3','4'].includes(selected('q2')[0]);
    const currentFrequency=selected('q3')[0];
    if(noRelease && currentFrequency!=='6') {
      document.querySelectorAll('[name=q3]').forEach(el=>{el.checked=el.value==='6';});
      syncChoiceVisual(document.querySelector('#question-q3'));
    } else if(!noRelease && currentFrequency==='6') {
      document.querySelectorAll('[name=q3]').forEach(el=>{el.checked=false;});
      syncChoiceVisual(document.querySelector('#question-q3'));
    }
  }
  if(key==='q4' && selected('q4').includes('9')){
    const provider=document.querySelector('#q4_provider');
    provider.value='';provider.disabled=true;
  }else if(key==='q4'){
    document.querySelector('#q4_provider').disabled=false;
  }
  if(key==='q2'||key==='q4')refreshExperienceBranch();
}
form.addEventListener('change', event=>{
  if(event.target.matches('input[type=checkbox],input[type=radio]')) handleChoice(event.target);
  if(event.target===surveyConsent || event.target===betaConsent) updateSubmitReady();
});
nextButton.addEventListener('click',()=>{const error=validateStep(current);if(error)return setError(error);showStep(current+1);});
prevButton.addEventListener('click',()=>showStep(current-1));
const comment=document.querySelector('#q14');
comment.addEventListener('input',()=>{document.querySelector('#char-count').textContent=`${comment.value.length.toLocaleString('ko-KR')} / 2,000`;});

function resetTurnstile(){turnstileToken='';if(widgetId!==null && window.turnstile)window.turnstile.reset(widgetId);updateSubmitReady();}
function initializeTurnstile() {
  // Public site key comes from an immutable static HTML asset: no Worker CPU on page views.
  const publicKey = document.querySelector('meta[name="turnstile-site-key"]')?.content || '';
  if (!publicKey || publicKey.includes('REPLACE_WITH')) {
    securityStatus.textContent='설문 설정을 준비하고 있어요. 잠시 후 다시 방문해 주세요.';
    return;
  }
  siteKey=publicKey;
  serviceReady=true; // Server still independently checks D1, secret and token on POST.
  securityStatus.textContent='마지막 단계에서 보안 확인을 진행해 주세요.';
  if(current===steps.length-1)void renderTurnstile();
}
async function renderTurnstile() {
  if (!siteKey || widgetId!==null || turnstileLoading) return;
  turnstileLoading=true;
  try {
    await new Promise((resolve,reject)=>{
      const script=document.createElement('script');
      script.src='https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit';
      script.async=true;script.onload=resolve;script.onerror=reject;
      document.head.append(script);
    });
    // Render only after the final step is visible (hidden containers can fail).
    widgetId=window.turnstile.render('#turnstile-widget',{
      sitekey:siteKey,
      action:'audeniq_survey',
      callback(value){turnstileToken=value;securityStatus.textContent='보안 확인이 완료됐어요.';updateSubmitReady();},
      'expired-callback'(){turnstileToken='';securityStatus.textContent='보안 확인이 만료됐어요. 다시 확인해 주세요.';updateSubmitReady();},
      'error-callback'(){turnstileToken='';securityStatus.textContent='보안 확인에 문제가 있어요. 페이지를 새로고침해 주세요.';updateSubmitReady();}
    });
    securityStatus.textContent='아래 보안 확인을 완료해 주세요.';
    updateSubmitReady();
  } catch {
    securityStatus.textContent='보안 확인을 불러오지 못했어요. 네트워크를 확인하고 새로고침해 주세요.';
  } finally {turnstileLoading=false;}
}

function getPayload() {
 const answers={};
 for(let i=FIRST;i<=LAST;i++) {
   const key=`q${i}`;
   answers[key]=MULTI.has(key)?selected(key):selected(key)[0]??'';
   const other=form.elements.namedItem(`${key}_other`);
   if(other)answers[`${key}_other`]=other.disabled?'':other.value.trim();
 }
 answers.q4_provider=form.elements.namedItem('q4_provider').value.trim();
 answers.q14=comment.value.trim();
 return {
   submission_id:submissionId,
   answers,
   beta:selected('beta')[0]??'',
   contact:contact.value.trim(),
   beta_contact_consent:betaConsent.checked,
   survey_consent:surveyConsent.checked,
   turnstile_token:turnstileToken
 };
}
form.addEventListener('submit',async event=>{
 event.preventDefault();
 if(busy)return;
 refreshExperienceBranch();
 const error=steps.map((_,i)=>validateStep(i)).find(Boolean);if(error)return setError(error);
 if(!serviceReady || !turnstileToken)return setError('보안 확인을 완료해 주세요.');
 busy=true;updateSubmitReady();submitButton.textContent='제출하는 중…';setError('');
 try {
   const response=await fetch('/api/responses',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(getPayload()),cache:'no-store'});
   const data=await response.json();
   if(!response.ok || !data.ok)throw new Error(data.error||'설문 제출에 실패했어요.');
   receipt.textContent=data.receipt;
   form.hidden=true;document.querySelector('#complete').hidden=false;
   animateSurveyComplete(document.querySelector('#complete'));
   document.querySelector('.progress-head').hidden=true;document.querySelector('.progress-track').hidden=true;
   document.querySelector('#complete').scrollIntoView({block:'center',behavior:'smooth'});
 } catch(error) {setError(error instanceof Error?error.message:'제출에 실패했어요.');resetTurnstile();}
 finally {busy=false;submitButton.textContent='설문 제출하기 ↗';updateSubmitReady();}
});
document.querySelector('#copy-receipt').addEventListener('click',async()=>{
 try {await navigator.clipboard.writeText(receipt.textContent);document.querySelector('#copy-receipt').textContent='복사했어요 ✓';}
 catch {document.querySelector('#copy-receipt').textContent='접수번호를 길게 눌러 복사해 주세요.';}
});
refreshExperienceBranch();
if(selected('q4').includes('9')) document.querySelector('#q4_provider').disabled=true;
syncAllChoices();
// Safari may restore checked inputs from page history without firing change.
window.addEventListener('pageshow',()=>{refreshExperienceBranch();if(selected('q4').includes('9')){const provider=document.querySelector('#q4_provider');provider.value='';provider.disabled=true;}syncAllChoices();});
initializeTurnstile();
initializeMotion();
