#!/usr/bin/env node
// CloudFormation 템플릿(JSON) 생성: node deploy/aws/template.mjs <studio|admin|survey|landing>
// 뷰어 함수 코드와 _headers의 보안 헤더를 그대로 넣기 위해 YAML 대신 스크립트로 만든다.
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { APPS, parseHeaders } from './apps.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(HERE, '../..');

// AWS 관리형 정책 ID (고정값)
const CACHING_OPTIMIZED = '658327ea-f89d-4fab-a63d-7e88639e58f6';
const CACHING_DISABLED = '4135ea2d-6df8-44a3-9df3-4b5a84be39ad';
// Host를 빼고 뷰어 헤더·쿠키·쿼리 전부 + CloudFront-Viewer-Address 등 CloudFront 헤더를 전달
const ALL_VIEWER_EXCEPT_HOST = 'b689b0a8-53d0-40ab-baf2-68738e2966ac';

export const EDGE_HEADER = 'x-audeniq-edge';

export async function functionCode(name) {
  const app = APPS[name];
  let code = readFileSync(resolve(HERE, 'functions', app.function), 'utf8');
  if (app.routes) {
    const { PAGE_PATHS, FILE_PATHS } = await import(pathToFileURL(resolve(ROOT, app.routes)).href);
    code = code.replace('/*ROUTES*/', `var PAGE_PATHS = ${JSON.stringify(PAGE_PATHS)};\nvar FILE_PATHS = ${JSON.stringify(FILE_PATHS)};`);
  }
  if (Buffer.byteLength(code) > 10 * 1024) throw new Error(`${app.function}: CloudFront Function은 10KB까지`);
  return code;
}

export function headersText(name) {
  const app = APPS[name];
  return app.headersText ?? readFileSync(resolve(ROOT, app.headers), 'utf8');
}

/** _headers 의 "/*" 블록 → CloudFront 응답 헤더 정책 (+ Workers secureResponse 의 HSTS) */
export function responseHeadersPolicy(text) {
  const all = parseHeaders(text).find(rule => rule.path === '/*')?.headers ?? [];
  const security = {
    StrictTransportSecurity: { AccessControlMaxAgeSec: 31536000, IncludeSubdomains: true, Override: true },
  };
  const custom = [];
  for (const [name, value] of all) {
    switch (name.toLowerCase()) {
      case 'content-security-policy':
        security.ContentSecurityPolicy = { ContentSecurityPolicy: value, Override: true }; break;
      case 'x-content-type-options':
        if (value !== 'nosniff') throw new Error('X-Content-Type-Options는 nosniff만 지원');
        security.ContentTypeOptions = { Override: true }; break;
      case 'x-frame-options':
        security.FrameOptions = { FrameOption: value.toUpperCase(), Override: true }; break;
      case 'referrer-policy':
        security.ReferrerPolicy = { ReferrerPolicy: value, Override: true }; break;
      case 'cache-control':
        throw new Error('"/*"의 Cache-Control은 S3 메타데이터로만 지원');
      default:
        custom.push({ Header: name, Value: value, Override: true });
    }
  }
  return {
    SecurityHeadersConfig: security,
    ...(custom.length ? { CustomHeadersConfig: { Items: custom } } : {}),
  };
}

export async function template(name) {
  const app = APPS[name];
  if (!app) throw new Error(`알 수 없는 앱: ${name} (${Object.keys(APPS).join(', ')})`);
  const hasApi = app.api.length > 0;

  const parameters = {
    AcmCertificateArn: {
      Type: 'String', AllowedPattern: '^arn:aws:acm:us-east-1:[0-9]{12}:certificate/.+$',
      Description: `${app.aliases.join(', ')} 를 덮는 us-east-1 ACM 인증서 ARN`,
    },
    PriceClass: { Type: 'String', Default: 'PriceClass_200', AllowedValues: ['PriceClass_100', 'PriceClass_200', 'PriceClass_All'] },
  };
  if (hasApi) {
    parameters.ApiOriginDomain = {
      Type: 'String', MinLength: 1, AllowedPattern: '^[a-z0-9.-]+$',
      Description: '/api 를 처리하는 Cloudflare Worker의 커스텀 도메인 (예: studio-api.audeniq.com)',
    };
    parameters.EdgeOriginSecret = {
      Type: 'String', NoEcho: true, MinLength: 32,
      Description: `Worker의 EDGE_ORIGIN_SECRET 과 같은 값. CloudFront가 ${EDGE_HEADER} 헤더로 붙인다`,
    };
  }

  const origins = [{
    Id: 's3',
    DomainName: { 'Fn::GetAtt': ['Bucket', 'RegionalDomainName'] },
    S3OriginConfig: { OriginAccessIdentity: '' },
    OriginAccessControlId: { 'Fn::GetAtt': ['OriginAccessControl', 'Id'] },
  }];
  if (hasApi) {
    origins.push({
      Id: 'worker',
      DomainName: { Ref: 'ApiOriginDomain' },
      CustomOriginConfig: { OriginProtocolPolicy: 'https-only', OriginSSLProtocols: ['TLSv1.2'], HTTPSPort: 443, OriginReadTimeout: 60 },
      OriginCustomHeaders: [{ HeaderName: EDGE_HEADER, HeaderValue: { Ref: 'EdgeOriginSecret' } }],
    });
  }

  const functions = [{ EventType: 'viewer-request', FunctionARN: { 'Fn::GetAtt': ['ViewerFunction', 'FunctionARN'] } }];
  if (app.viewerResponse) functions.push({ EventType: 'viewer-response', FunctionARN: { 'Fn::GetAtt': ['ViewerFunction', 'FunctionARN'] } });

  const distribution = {
    Enabled: true,
    Comment: `AUDENIQ ${name} UI`,
    Aliases: app.aliases,
    HttpVersion: 'http2and3',
    IPV6Enabled: true,
    PriceClass: { Ref: 'PriceClass' },
    ViewerCertificate: { AcmCertificateArn: { Ref: 'AcmCertificateArn' }, SslSupportMethod: 'sni-only', MinimumProtocolVersion: 'TLSv1.2_2021' },
    Origins: origins,
    DefaultCacheBehavior: {
      TargetOriginId: 's3',
      ViewerProtocolPolicy: 'redirect-to-https',
      AllowedMethods: ['GET', 'HEAD'],
      CachedMethods: ['GET', 'HEAD'],
      CachePolicyId: CACHING_OPTIMIZED,
      ResponseHeadersPolicyId: { Ref: 'ResponseHeaders' },
      Compress: true,
      FunctionAssociations: functions,
    },
    // /api 는 캐시하지 않고 Worker로 그대로 (Worker가 자체 보안 헤더·압축·HTTPS 검사를 한다)
    CacheBehaviors: app.api.map(PathPattern => ({
      PathPattern,
      TargetOriginId: 'worker',
      ViewerProtocolPolicy: 'https-only',
      AllowedMethods: ['GET', 'HEAD', 'OPTIONS', 'PUT', 'PATCH', 'POST', 'DELETE'],
      CachedMethods: ['GET', 'HEAD'],
      CachePolicyId: CACHING_DISABLED,
      OriginRequestPolicyId: ALL_VIEWER_EXCEPT_HOST,
      Compress: false,
    })),
  };
  if (app.notFoundPage) {
    // 오류 응답 페이지는 배포 전체에 걸리므로 404만 — Worker의 403·405 JSON은 그대로 둔다
    distribution.CustomErrorResponses = [{ ErrorCode: 404, ResponseCode: 404, ResponsePagePath: app.notFoundPage, ErrorCachingMinTTL: 10 }];
  }

  return {
    AWSTemplateFormatVersion: '2010-09-09',
    Description: `AUDENIQ ${name} UI — S3 + CloudFront${hasApi ? ' (/api → Cloudflare Worker)' : ''}`,
    Parameters: parameters,
    Resources: {
      Bucket: {
        Type: 'AWS::S3::Bucket',
        DeletionPolicy: 'Retain',
        UpdateReplacePolicy: 'Retain',
        Properties: {
          PublicAccessBlockConfiguration: { BlockPublicAcls: true, BlockPublicPolicy: true, IgnorePublicAcls: true, RestrictPublicBuckets: true },
          OwnershipControls: { Rules: [{ ObjectOwnership: 'BucketOwnerEnforced' }] },
          BucketEncryption: { ServerSideEncryptionConfiguration: [{ ServerSideEncryptionByDefault: { SSEAlgorithm: 'AES256' } }] },
        },
      },
      OriginAccessControl: {
        Type: 'AWS::CloudFront::OriginAccessControl',
        Properties: { OriginAccessControlConfig: {
          Name: { 'Fn::Sub': '${AWS::StackName}-s3' },
          OriginAccessControlOriginType: 's3', SigningBehavior: 'always', SigningProtocol: 'sigv4',
        } },
      },
      BucketPolicy: {
        Type: 'AWS::S3::BucketPolicy',
        Properties: {
          Bucket: { Ref: 'Bucket' },
          PolicyDocument: { Version: '2012-10-17', Statement: [{
            Effect: 'Allow',
            Principal: { Service: 'cloudfront.amazonaws.com' },
            // ListBucket: 없는 파일이 403 대신 404가 되게 (SPA 폴백·404 페이지)
            Action: ['s3:GetObject', 's3:ListBucket'],
            Resource: [{ 'Fn::GetAtt': ['Bucket', 'Arn'] }, { 'Fn::Sub': '${Bucket.Arn}/*' }],
            Condition: { StringEquals: { 'AWS:SourceArn': { 'Fn::Sub': 'arn:aws:cloudfront::${AWS::AccountId}:distribution/${Distribution}' } } },
          }] },
        },
      },
      ViewerFunction: {
        Type: 'AWS::CloudFront::Function',
        Properties: {
          Name: { 'Fn::Sub': '${AWS::StackName}-viewer' },
          AutoPublish: true,
          FunctionConfig: { Comment: `AUDENIQ ${name} routing`, Runtime: 'cloudfront-js-2.0' },
          FunctionCode: await functionCode(name),
        },
      },
      ResponseHeaders: {
        Type: 'AWS::CloudFront::ResponseHeadersPolicy',
        Properties: { ResponseHeadersPolicyConfig: {
          Name: { 'Fn::Sub': '${AWS::StackName}-headers' },
          ...responseHeadersPolicy(headersText(name)),
        } },
      },
      Distribution: { Type: 'AWS::CloudFront::Distribution', Properties: { DistributionConfig: distribution } },
    },
    Outputs: {
      BucketName: { Value: { Ref: 'Bucket' } },
      DistributionId: { Value: { Ref: 'Distribution' } },
      DistributionDomainName: { Value: { 'Fn::GetAtt': ['Distribution', 'DomainName'] }, Description: 'DNS(CNAME)를 이 주소로' },
    },
  };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  console.log(JSON.stringify(await template(process.argv[2]), null, 2));
}
