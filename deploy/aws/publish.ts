#!/usr/bin/env node
// 빌드된 화면을 S3에 올리고 CloudFront 캐시를 비운다: node --experimental-strip-types deploy/aws/publish.ts <studio|admin|landing>
// 먼저 deploy-stack.sh 로 스택(audeniq-<app>-ui)이 있어야 한다. AWS 자격증명·리전은 환경변수(AWS_REGION 등)로.
//
// - Cache-Control은 앱의 _headers 규칙 그대로 S3 메타데이터로 넣는다 (나머지는 no-cache)
// - 지난 배포의 해시 에셋은 지우지 않는다 — 열려 있던 화면이 예전 청크를 계속 받을 수 있게
// - .br 사이드카·_headers·_redirects 는 올리지 않는다 (CloudFront가 압축, 헤더·리다이렉트는 스택이 처리)
import { execFileSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { APPS, cacheGroups, parseHeaders, stackName } from './apps.ts';
import { headersText } from './template.ts';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const SKIP = ['_headers', '_redirects', '*.br', '.DS_Store'];

/** aws s3 sync 호출 목록 (순서대로 실행) */
export function syncCommands(name, bucket) {
  const app = APPS[name];
  const source = resolve(ROOT, app.dir);
  const target = `s3://${bucket}`;
  const groups = cacheGroups(parseHeaders(headersText(name)));
  const exclude = SKIP.flatMap(p => ['--exclude', p]);
  const commands = groups.map(({ include, cacheControl }) =>
    ['s3', 'sync', source, target, '--exclude', '*', '--include', include, ...exclude, '--cache-control', cacheControl]);
  // 그 밖의 파일(HTML 포함)은 마지막에, 매번 다시 확인하게
  commands.push(['s3', 'sync', source, target, ...exclude, ...groups.flatMap(g => ['--exclude', g.include]),
    '--cache-control', 'no-cache']);
  return commands;
}

function aws(args, options = {}) {
  return execFileSync('aws', args, { encoding: 'utf8', stdio: options.quiet ? ['ignore', 'pipe', 'inherit'] : 'inherit' });
}

function stackOutputs(name) {
  const raw = aws(['cloudformation', 'describe-stacks', '--stack-name', stackName(name),
    '--query', 'Stacks[0].Outputs', '--output', 'json'], { quiet: true });
  return Object.fromEntries(JSON.parse(raw).map(o => [o.OutputKey, o.OutputValue]));
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const name = process.argv[2];
  if (!APPS[name]) throw new Error(`사용법: publish.ts <${Object.keys(APPS).join('|')}>`);
  const dir = resolve(ROOT, APPS[name].dir);
  if (!existsSync(resolve(dir, 'index.html'))) throw new Error(`${APPS[name].dir}/index.html 이 없습니다. 먼저 빌드하세요.`);
  const { BucketName, DistributionId } = stackOutputs(name);
  for (const args of syncCommands(name, BucketName)) aws(args);
  aws(['cloudfront', 'create-invalidation', '--distribution-id', DistributionId, '--paths', '/*']);
  console.log(`배포됨: ${name} → s3://${BucketName} (CloudFront ${DistributionId})`);
}
