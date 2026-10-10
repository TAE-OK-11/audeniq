import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import {validateSubmission,default as worker} from '../src/worker.js';
import questions from '../questions.json' with {type:'json'};
import {STEPS} from '../app/src/survey.ts';
import {applyChoice,emptyState,payload,validateAll} from '../app/src/logic.ts';
const html=fs.readFileSync(new URL('../public/index.html',import.meta.url),'utf8');
const read=p=>fs.readFileSync(new URL(p,import.meta.url),'utf8');
const client=read('../app/src/App.tsx')+read('../app/src/logic.ts');
const ui=new Map(STEPS.flatMap(s=>s.questions).map(q=>[q.key,q]));
function valid(){
 const answers={};
 for(const [key,q] of Object.entries(questions)){
  answers[key]=q.type==='multi'?(q.required?['0']:[]):(q.required?'0':'');
  if(q.options.includes('기타'))answers[`${key}_other`]='';
 }
 answers.q4_provider='';answers.q14='';
 return {submission_id:'ccb1d9d3-65de-4a58-96d7-4fe041109895',answers,beta:'',contact:'',beta_contact_consent:false,survey_consent:true,turnstile_token:'unit-test-token'};
}
test('all 18 questions in the React UI match the server questions exactly',()=>{
 assert.equal(Object.keys(questions).length,17);
 for(const [key,q] of Object.entries(questions)){
  const u=ui.get(key);
  assert.ok(u,`missing question ${key}`);
  assert.equal(u.title,q.title,`title ${key}`);
  assert.deepEqual(u.options,q.options,`options ${key}`);
  assert.equal(u.kind,q.type,`type ${key}`);
  assert.equal(u.required,!!q.required,`required ${key}`);
  assert.equal(u.max??null,q.max??null,`max ${key}`);
  assert.ok(q.options.length>=5);
 }
 assert.ok(ui.has('q14'));
 assert.equal(ui.get('beta').options.length,4);
 assert.match(client,/AUDENIQ_Logo_Light.svg/);
 assert.equal(questions.q11.max,5);
 assert.match(ui.get('q8').hint,/DSP에서 실제 발매되는 시점과는 별개/);
 assert.match(client,/noDistributorExperience/);
});
test('what the React form sends passes server validation',()=>{
 let st=emptyState();
 const pick=(k,v,on=true)=>{st=applyChoice(st,k,v,on).state;};
 for(const [k,q] of ui) if(q.required&&q.options&&k!=='beta') pick(k,'0');
 pick('q2','3'); // no release: q3 becomes '6', experience questions are cleared
 st={...st,surveyConsent:true};
 assert.equal(validateAll(st),'');
 const body=payload(st,'ccb1d9d3-65de-4a58-96d7-4fe041109895','unit-test-token');
 assert.equal(body.answers.q3,'6');
 assert.ok(validateSubmission(body),'server must accept the client payload');
 pick('q4','9'); st={...st,provider:'x'};
 assert.equal(payload(st,'ccb1d9d3-65de-4a58-96d7-4fe041109895','t').answers.q4_provider,'');
});
test('complete anonymous survey with experience passes',()=>{
 const v=validateSubmission(valid());assert.ok(v);assert.equal(v.contact,null);assert.equal(JSON.parse(v.answers).q1[0],'0');
});
test('max selections, invalid values and mutually exclusive options are rejected',()=>{
 for(const [key,newValue] of [['q1',[]],['q7',['0','1','2','3']],['q11',['0','1','2','3','4','5']],['q15',['0','13']],['q15',[]],['q3','900'],['q4',['0','9']],['q6',['0','18']]]) {
  const p=valid();p.answers[key]=newValue;assert.equal(validateSubmission(p),null,`${key} should be invalid`);
 }
});
test('first-time artists can skip experience-dependent questions but not other required answers',()=>{
 const p=valid();p.answers.q2='2';p.answers.q3='6';p.answers.q4=['9'];p.answers.q5='';p.answers.q6=[];p.answers.q12='';
 assert.ok(validateSubmission(p),'first-time respondent must pass');
 for(const key of ['q5','q12']){
  p.answers[key]='0';assert.equal(validateSubmission(p),null,`skipped ${key} must remain empty`);p.answers[key]='';
 }
 p.answers.q6=['0'];assert.equal(validateSubmission(p),null,'skipped q6 must be empty');p.answers.q6=[];
 p.answers.q15=[];assert.equal(validateSubmission(p),null,'DSP preference is required');
});
test('experienced artists selecting no distributor cannot answer experience branch',()=>{
 const p=valid();p.answers.q4=['9'];p.answers.q5='0';assert.equal(validateSubmission(p),null);
 p.answers.q5='';p.answers.q12='';assert.ok(validateSubmission(p));
 p.answers.q4_provider='name';assert.equal(validateSubmission(p),null);
});
test('no-release frequency and release experience must be consistent',()=>{
 const p=valid();p.answers.q2='3';assert.equal(validateSubmission(p),null);
 p.answers.q3='6';p.answers.q5='';p.answers.q6=[];p.answers.q12='';assert.ok(validateSubmission(p));
 p.answers.q2='0';assert.equal(validateSubmission(p),null);
});
test('other fields and free-text lengths are validated',()=>{
 const p=valid();p.answers.q1=['10'];assert.equal(validateSubmission(p),null);
 p.answers.q1_other='기타 음악';assert.ok(validateSubmission(p));
 p.answers.q1_other='X'.repeat(161);assert.equal(validateSubmission(p),null);
 const r=valid();r.answers.q14='X'.repeat(2001);assert.equal(validateSubmission(r),null);
});
test('optional beta contact requires separate consent and opted-out users cannot give contact',()=>{
 const p=valid();p.beta='0';p.contact='@audeniq.official';assert.equal(validateSubmission(p),null);
 p.beta_contact_consent=true;assert.equal(validateSubmission(p).contact,'@audeniq.official');
 p.beta='3';assert.equal(validateSubmission(p),null);
 p.beta='1';p.contact='not-an-email';assert.equal(validateSubmission(p),null);
 p.contact='sample@example.com';assert.ok(validateSubmission(p));
});
const SECRET='s'.repeat(40);
/** 백엔드(터널 → api) 대역: 받은 요청을 기록하고 정해진 응답을 돌려준다 */
async function withBackend(answer,fn){
 const seen=[];const original=globalThis.fetch;
 globalThis.fetch=async req=>{seen.push(req);return answer(req);};
 try{await fn(seen);}finally{globalThis.fetch=original;}
}
const post=(headers,body,env={EDGE_SERVICE_SECRET:SECRET})=>worker.fetch(new Request('https://survey.audeniq.com/api/responses',{method:'POST',headers,body}),env);
const ok={'Origin':'https://survey.audeniq.com','Content-Type':'application/json'};

test('a valid submission goes to the backend survey API with the service secret and site',async()=>{
 await withBackend(()=>Response.json({ok:true,receipt:'r',duplicate:false}),async seen=>{
  const r=await post({...ok,'CF-Connecting-IP':'203.0.113.9','X-Audeniq-Service':'forged'},JSON.stringify(valid()));
  assert.equal(r.status,200);assert.equal((await r.json()).ok,true);
  assert.equal(seen.length,1);
  assert.equal(new URL(seen[0].url).pathname,'/api/survey/responses');
  assert.equal(seen[0].headers.get('x-audeniq-service'),SECRET);
  assert.equal(seen[0].headers.get('x-audeniq-site'),'survey');
  assert.equal(seen[0].headers.get('x-audeniq-client-ip'),'203.0.113.9');
  assert.equal(seen[0].headers.get('origin'),'https://survey.audeniq.com');
  assert.deepEqual(JSON.parse(await seen[0].text()),valid());
 });
});

test('config comes from the backend; without the service secret nothing is forwarded',async()=>{
 await withBackend(()=>Response.json({turnstileSitekey:'public-site-key',ready:true}),async seen=>{
  const r=await worker.fetch(new Request('https://survey.audeniq.com/api/config'),{EDGE_SERVICE_SECRET:SECRET});
  assert.deepEqual(await r.json(),{turnstileSitekey:'public-site-key',ready:true});
  assert.equal(new URL(seen[0].url).pathname,'/api/survey/config');
  assert.equal((await worker.fetch(new Request('https://survey.audeniq.com/api/config'),{})).status,503);
  assert.equal((await post(ok,JSON.stringify(valid()),{})).status,503);
  assert.equal(seen.length,1);
 });
});

test('unknown URL uses the static 404 page, with security headers',async()=>{
 const r=await worker.fetch(new Request('https://survey.audeniq.com/invalid'),{ASSETS:{fetch:async()=>new Response('Not found',{status:404})}});
 assert.equal(r.status,404);
 const headers=fs.readFileSync(new URL('../app/public/_headers',import.meta.url),'utf8');
 assert.equal(fs.readFileSync(new URL('../public/_headers',import.meta.url),'utf8'),headers,'build copies _headers');
 assert.match(headers,/X-Robots-Tag: noindex/);
 assert.match(headers,/frame-ancestors 'none'/);
});

test('public API cannot enumerate responses: other /api paths never reach the backend',async()=>{
 await withBackend(()=>{throw new Error('must not be forwarded');},async()=>{
  for(const url of ['/api/responses','/api/admin','/api/export','/api/survey/responses']){
   const r=await worker.fetch(new Request('https://survey.audeniq.com'+url),{EDGE_SERVICE_SECRET:SECRET});
   assert.ok([404,405].includes(r.status),`${url}: should not allow public reads`);
  }
 });
});

test('invalid body, CSRF origin and oversized submissions never reach the backend',async()=>{
 await withBackend(()=>{throw new Error('must not be forwarded');},async()=>{
  let r=await post({'Content-Type':'application/json'},JSON.stringify(valid()));assert.equal(r.status,403);
  r=await post({...ok,'Origin':'https://wrong.example'},JSON.stringify(valid()));assert.equal(r.status,403);
  r=await post({...ok,'Content-Type':'text/plain'},JSON.stringify(valid()));assert.equal(r.status,400);
  r=await post(ok,'{invalid');assert.equal(r.status,400);
  r=await post(ok,JSON.stringify({ ...valid(), answers: {}}));assert.equal(r.status,400);
  r=await post(ok,' '.repeat(33*1024));assert.equal(r.status,413);
 });
});

test('a dead backend answers 502; the Worker keeps no D1 or retention cron',async()=>{
 await withBackend(()=>{throw new Error('down');},async()=>{
  assert.equal((await post(ok,JSON.stringify(valid()))).status,502);
 });
 assert.equal(worker.scheduled,undefined);
 const cfg=JSON.parse(fs.readFileSync(new URL('../wrangler.jsonc',import.meta.url),'utf8'));
 assert.equal(cfg.d1_databases,undefined);assert.equal(cfg.triggers,undefined);
});

test('all requests pass the HTTPS guard and secrets stay out of the static site',()=>{
 const cfg=JSON.parse(fs.readFileSync(new URL('../wrangler.jsonc',import.meta.url),'utf8'));
 assert.equal(cfg.assets.run_worker_first,true);
 assert.equal(cfg.assets.html_handling,'auto-trailing-slash');
 assert.equal(cfg.assets.not_found_handling,'404-page');
 assert.match(html,/name="turnstile-site-key"/);
 assert.ok(!client.includes("fetch('/api/config'"));
 assert.ok(!html.includes('b6d9d9d9-9075-41e0-97bf-2614ab0b2926'));
 assert.ok(!html.includes('TURNSTILE_SECRET_KEY'));
 assert.ok(!client.includes('TURNSTILE_SECRET_KEY'));
 assert.ok(!fs.existsSync(new URL('../public/questions.json',import.meta.url)));
 assert.ok(!fs.existsSync(new URL('../public/wrangler.jsonc',import.meta.url)));
});
