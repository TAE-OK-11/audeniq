import questions from '../questions.json' with { type: 'json' };
import { proxyToBackend, requireHttps, secureResponse } from '../../shared/transport.js';
const MAX_BYTES=32*1024;
const API_HEADERS={'Content-Type':'application/json; charset=utf-8','Cache-Control':'no-store','X-Content-Type-Options':'nosniff','X-Robots-Tag':'noindex, nofollow','Referrer-Policy':'no-referrer'};
const json=(value,status=200)=>new Response(JSON.stringify(value),{status,headers:API_HEADERS});
const err=(message='입력 내용을 다시 확인해 주세요.',status=400)=>json({ok:false,error:message},status);
// Stop reading immediately on oversized bodies (Content-Length may be absent or inaccurate).
async function readLimitedBody(request) {
 const reader=request.body?.getReader();
 if (!reader) return '';
 const parts=[]; let size=0;
 while (true) {
  const {done,value}=await reader.read();
  if (done) break;
  size+=value.byteLength;
  if (size>MAX_BYTES) { await reader.cancel().catch(()=>{}); throw new RangeError('body too large'); }
  parts.push(value);
 }
 const bytes=new Uint8Array(size);let offset=0;
 for (const part of parts) {bytes.set(part,offset);offset+=part.byteLength;}
 return new TextDecoder('utf-8',{fatal:true}).decode(bytes);
}
const KEYS=Object.keys(questions);
const ALLOWED_TOP=new Set(['submission_id','answers','beta','contact','beta_contact_consent','survey_consent','turnstile_token']);
const isPlainObject=v=>!!v && typeof v==='object' && !Array.isArray(v) && Object.getPrototypeOf(v)===Object.prototype;
const boundedString=(v,n)=>typeof v==='string' && v.length<=n;
const optionalOther=(obj,key,chosen,options)=>{
 const value=obj[`${key}_other`];
 if (!boundedString(value,160)) return false;
 const other=options.indexOf('기타');
 if (other===-1) return value===undefined;
 const hasOther=Array.isArray(chosen)?chosen.includes(String(other)):chosen===String(other);
 return hasOther ? value.trim().length>0 : value.trim().length===0;
};
export function validateSubmission(p){
 if(!isPlainObject(p)||Object.keys(p).some(k=>!ALLOWED_TOP.has(k)))return null;
 if(!/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(p.submission_id||''))return null;
 if(!isPlainObject(p.answers))return null;
 const allowedAnswerKeys=new Set([...KEYS,'q4_provider','q14',...KEYS.filter(k=>questions[k].options.includes('기타')).map(k=>`${k}_other`)]);
 if(Object.keys(p.answers).some(k=>!allowedAnswerKeys.has(k)))return null;
 const noExperience=['2','3','4'].includes(p.answers.q2) || (Array.isArray(p.answers.q4) && p.answers.q4.includes('9'));
 for(const key of KEYS){
  const q=questions[key];const selection=p.answers[key];
  const skipped=noExperience && ['q5','q6','q12'].includes(key);
  if(skipped && !(q.type==='multi'?Array.isArray(selection)&&selection.length===0:selection===''))return null;
  if(q.type==='multi'){
   if(!Array.isArray(selection)||selection.length>(q.max||q.options.length)||(q.required && !skipped && !selection.length)||new Set(selection).size!==selection.length)return null;
   if(!selection.every(v=>typeof v==='string' && /^\d{1,2}$/.test(v) && Number(v)<q.options.length))return null;
   if(key==='q4' && selection.includes('9') && selection.length!==1)return null;
   if(key==='q6' && selection.includes('18') && selection.length!==1)return null;
   if(key==='q15' && selection.includes('13') && selection.length!==1)return null;
  }else{
   if(!boundedString(selection,4)||(!selection && q.required && !skipped))return null;
   if(selection && (!/^\d{1,2}$/.test(selection)||Number(selection)>=q.options.length))return null;
  }
  if(q.options.includes('기타') && !optionalOther(p.answers,key,selection,q.options))return null;
 }
 if(!boundedString(p.answers.q4_provider,160)||!boundedString(p.answers.q14,2000))return null;
 if(p.answers.q4.includes('9') && p.answers.q4_provider.trim())return null;
 if(['2','3','4'].includes(p.answers.q2) && p.answers.q3!=='6')return null;
 if(['0','1'].includes(p.answers.q2) && p.answers.q3==='6')return null;
 if(!['','0','1','2','3'].includes(p.beta))return null;
 if(!boundedString(p.contact,160))return null;
 const contact=p.contact.trim();
 if(contact && (p.beta==='3'||p.beta===''))return null;
 if(contact && !/^(?:[^\s@]+@[^\s@]+\.[^\s@]+|@[A-Za-z0-9._]{1,30})$/.test(contact))return null;
 if(typeof p.beta_contact_consent!=='boolean'||Boolean(contact)!==p.beta_contact_consent)return null;
 if(p.survey_consent!==true)return null;
 if(!boundedString(p.turnstile_token,2048)||!p.turnstile_token)return null;
 return {id:p.submission_id.toLowerCase(),answers:JSON.stringify(p.answers),beta:p.beta,contact:contact||null,betaConsent:Number(p.beta_contact_consent)};
}
const surveyWorker = {
 async fetch(request,env){
  const url=new URL(request.url);const path=url.pathname;
  if(path.startsWith('/api/')){
   // 접수·Turnstile 확인·저장은 백엔드가 한다 (crates/core/src/survey.rs, /api/survey/*).
   // 여기서는 경로·출처·형식만 먼저 걸러 백엔드 호출을 줄인다 (같은 규칙을 백엔드가 다시 확인).
   if(path==='/api/config' && request.method==='GET')return proxyToBackend(request,env,{site:'survey',path:'/api/survey/config'});
   if(path!=='/api/responses')return err('페이지를 찾을 수 없어요.',404);
   if(request.method!=='POST')return err('허용되지 않는 요청입니다.',405);
   if(request.headers.get('Origin')!==url.origin)return err('허용되지 않은 요청 출처입니다.',403);
   if(!/^application\/json(?:\s*;|$)/i.test(request.headers.get('Content-Type')||''))return err('JSON 형식으로 제출해 주세요.');
   if(Number(request.headers.get('Content-Length')||0)>MAX_BYTES)return err('제출 크기 제한을 초과했습니다.',413);
   let text;
   try {text=await readLimitedBody(request);}
   catch(error){return error instanceof RangeError ? err('제출 크기 제한을 초과했습니다.',413) : err('입력 형식을 확인해 주세요.');}
   let payload;
   try {payload=JSON.parse(text);}catch{return err('입력 형식을 확인해 주세요.');}
   if(!validateSubmission(payload))return err();
   return proxyToBackend(new Request(request.url,{method:'POST',headers:request.headers,body:text}),env,{site:'survey',path:'/api/survey/responses'});
  }
  if(path==='/health' && request.method==='GET')return json({ok:true,service:'AUDENIQ Survey'});
  return env.ASSETS ? env.ASSETS.fetch(request) : err('페이지를 찾을 수 없어요.',404);
 },
};

export default {
 async fetch(request,env){
  const refused=requireHttps(request);
  if(refused)return refused;
  return secureResponse(await surveyWorker.fetch(request,env));
 },
};
