import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import {validateSubmission,default as worker} from '../src/worker.js';
import questions from '../questions.json' with {type:'json'};
const html=fs.readFileSync(new URL('../public/index.html',import.meta.url),'utf8');
const client=fs.readFileSync(new URL('../public/app.js',import.meta.url),'utf8');
function valid(){
 const answers={};
 for(const [key,q] of Object.entries(questions)){
  answers[key]=q.type==='multi'?(q.required?['0']:[]):(q.required?'0':'');
  if(q.options.includes('기타'))answers[`${key}_other`]='';
 }
 answers.q4_provider='';answers.q14='';
 return {submission_id:'ccb1d9d3-65de-4a58-96d7-4fe041109895',answers,beta:'',contact:'',beta_contact_consent:false,survey_consent:true,turnstile_token:'unit-test-token'};
}
test('all 18 questions, added DSP, payout, pricing and release timing are present',()=>{
 assert.equal(Object.keys(questions).length,17);
 for(const [key,q] of Object.entries(questions)){
  assert.match(html,new RegExp(`id="question-${key}"`));
  assert.ok(html.includes(q.title),`missing question ${key}`);
  assert.ok(q.options.length>=5);
 }
 assert.match(html,/id="question-q14"/);
 assert.match(html,/name="beta"/);
 assert.match(html,/AUDENIQ_Logo_Light.svg/);
 assert.equal(questions.q11.max,5);
 assert.match(html,/DSP에서 실제 발매되는 시점과는 별개/);
 assert.match(client,/refreshExperienceBranch/);
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
test('config cannot accept submissions without credentials; invalid Origin forbidden',async()=>{
 let response=await worker.fetch(new Request('https://survey.audeniq.com/api/config'),{});
 assert.equal((await response.json()).ready,false);
 response=await worker.fetch(new Request('https://survey.audeniq.com/api/responses',{method:'POST',headers:{Origin:'https://untrusted.example','Content-Type':'application/json'},body:JSON.stringify(valid())}),{});
 assert.equal(response.status,403);
 response=await worker.fetch(new Request('https://survey.audeniq.com/api/responses',{method:'POST',headers:{Origin:'https://survey.audeniq.com','Content-Type':'application/json'},body:JSON.stringify(valid())}),{});
 assert.equal(response.status,503);
});
test('unknown URL is 404; static files bypass Worker, with security headers',async()=>{
 const r=await worker.fetch(new Request('https://survey.audeniq.com/invalid'),{});
 assert.equal(r.status,404);
 const headers=fs.readFileSync(new URL('../public/_headers',import.meta.url),'utf8');
 assert.match(headers,/X-Robots-Tag: noindex/);
 assert.match(headers,/frame-ancestors 'none'/);
});
test('D1 insert is idempotent and records survey version 4',async()=>{
 const originalFetch=globalThis.fetch;
 const saved=[];
 const db={prepare(sql){return {bind(...params){return {async first(){return saved.includes(params[0]?.id??params[0])?{id:params[0]}:null;},async run(){saved.push(params[0]);return {success:true};}};}}}};
 globalThis.fetch=async(url)=> {assert.match(String(url),/turnstile\/v0\/siteverify/);return Response.json({success:true,hostname:'survey.audeniq.com',action:'audeniq_survey'});};
 try{
  const env={DB:db,TURNSTILE_SECRET_KEY:'testsecret',TURNSTILE_SITE_KEY:'testsitekey'};
  const request=()=>new Request('https://survey.audeniq.com/api/responses',{method:'POST',headers:{Origin:'https://survey.audeniq.com','Content-Type':'application/json'},body:JSON.stringify(valid())});
  let r=await worker.fetch(request(),env);assert.equal(r.status,200);assert.equal((await r.json()).ok,true);
  r=await worker.fetch(request(),env);assert.equal(r.status,200);assert.equal((await r.json()).duplicate,true);assert.equal(saved.length,1);
 }finally{globalThis.fetch=originalFetch;}
});

test('public API cannot enumerate private D1 data or reveal secret and DB ID', async()=>{
 const db={prepare(){throw new Error('Public request must not access D1');}};
 const env={DB:db,TURNSTILE_SECRET_KEY:'keep-this-private',TURNSTILE_SITE_KEY:'public-site-key'};
 let r=await worker.fetch(new Request('https://survey.audeniq.com/api/config'),env);
 assert.equal(r.status,200);
 const cfg=await r.json();
 assert.deepEqual(cfg,{turnstileSitekey:'public-site-key',ready:true});
 assert.ok(!JSON.stringify(cfg).includes('keep-this-private'));
 for(const url of ['/api/responses','/api/admin','/api/export','/questions.json','/migrations/0001_create_responses.sql','/wrangler.jsonc']){
   r=await worker.fetch(new Request('https://survey.audeniq.com'+url),env);
   assert.ok([404,405].includes(r.status), `${url}: should not allow public reads`);
 }
});

test('invalid body, CSRF origin and oversized submissions never touch D1',async()=>{
 const env={DB:{prepare(){throw new Error('D1 should not be called');}},TURNSTILE_SECRET_KEY:'private',TURNSTILE_SITE_KEY:'public-site-key'};
 const origin='https://survey.audeniq.com';
 const post=(headers,body)=>worker.fetch(new Request(origin+'/api/responses',{method:'POST',headers,body}),env);
 let r=await post({'Content-Type':'application/json'},JSON.stringify(valid()));assert.equal(r.status,403);
 r=await post({'Origin':'https://wrong.example','Content-Type':'application/json'},JSON.stringify(valid()));assert.equal(r.status,403);
 r=await post({'Origin':origin,'Content-Type':'text/plain'},JSON.stringify(valid()));assert.equal(r.status,400);
 r=await post({'Origin':origin,'Content-Type':'application/json'},'{invalid');assert.equal(r.status,400);
 r=await post({'Origin':origin,'Content-Type':'application/json'},JSON.stringify({ ...valid(), answers: {}}));assert.equal(r.status,400);
 r=await post({'Origin':origin,'Content-Type':'application/json'},' '.repeat(33*1024));assert.equal(r.status,413);
});

test('Turnstile server validation failure blocks D1 insert',async()=>{
 const oldFetch=globalThis.fetch;
 let inserted=false;
 const env={DB:{prepare(sql){return{bind(){return{async first(){return null;},async run(){inserted=true;return {success:true};}};}};}},TURNSTILE_SECRET_KEY:'private',TURNSTILE_SITE_KEY:'public-site-key'};
 globalThis.fetch=async()=>Response.json({success:true,hostname:'evil.example',action:'audeniq_survey'});
 try{
  const r=await worker.fetch(new Request('https://survey.audeniq.com/api/responses',{method:'POST',headers:{Origin:'https://survey.audeniq.com','Content-Type':'application/json'},body:JSON.stringify(valid())}),env);
  assert.equal(r.status,403);assert.equal(inserted,false);
 }finally{globalThis.fetch=oldFetch;}
});

test('retention runs only in scheduled Worker handler, never from browser route',async()=>{
 let deletion='';
 await worker.scheduled(null,{DB:{prepare(sql){deletion=sql;return{async run(){return{success:true};}};}}});
 assert.match(deletion,/DELETE FROM survey_responses/);
 assert.match(deletion,/-12 months/);
});


test('static HTML/JS bypass Worker CPU and /api responses retain private server-only DB',()=>{
 const cfg=JSON.parse(fs.readFileSync(new URL('../wrangler.jsonc',import.meta.url),'utf8'));
 assert.deepEqual(cfg.assets.run_worker_first,['/api/*','/health']);
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
