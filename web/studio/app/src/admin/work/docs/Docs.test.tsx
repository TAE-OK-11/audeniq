// 문서 화면 — 임베드 일괄 조회, 권한(보기 전용), 자동 저장(기준 버전·정리된 내용), 충돌(409), 없는 문서,
// '/' 명령을 키보드로 고르기.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Editor } from '@tiptap/core';
import { BrowserRouter, Route, Routes } from '../../../lib/router';
import { ToastProvider } from '../../../components/Toast';
import { ConfirmProvider } from '../../../components/Confirm';
import { ApiError } from '../../../api/errors';
import { StaffContext } from '../../ui';
import type { DocDetail, DocLevel, DocNode, DocSaveInput } from '../types';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const api = vi.hoisted(() => ({
  people: vi.fn(),
  doc: vi.fn(),
  docTree: vi.fn(),
  saveDoc: vi.fn(),
  embeds: vi.fn(),
  searchEntities: vi.fn(),
  docComments: vi.fn(),
  attachmentInline: (id: string) => `/a/${id}?inline=true`,
  attachmentUrl: (id: string) => `/a/${id}`,
}));
vi.mock('../api', () => ({ workApi: api }));

import { Docs } from '../pages/Docs';

const content: DocNode = {
  type: 'doc', content: [
    { type: 'paragraph', content: [{ type: 'text', text: '원인 분석' }] },
    { type: 'entityEmbed', attrs: { kind: 'release', id: 'r1', view: 'qc', label: '새벽의 온도' } },
    { type: 'entityEmbed', attrs: { kind: 'task', id: 't1', view: 'summary', label: '재전송' } },
  ],
};
function detail(level: DocLevel = 'OWNER', version = 4): DocDetail {
  return {
    doc: {
      id: 'd1', parent_id: null, depth: 0, title: '멜론 장애', icon: '🚨', status: 'DRAFT', staff_access: 'EDITOR', version, position: 1024,
      template_id: null, created_by: 'me', updated_by: 'me', created_at: '2026-10-01T00:00:00Z', updated_at: '2026-10-01T00:00:00Z', archived_at: null,
      review: { requested_by: null, requested_at: null, approved_by: null, approved_at: null, rejected_by: null, rejected_at: null, rejection_reason: null },
      level, content,
    },
    breadcrumb: [], related: [], cards: [], backlinks: [], open_comments: 0,
  };
}

let host: HTMLDivElement;
let root: Root;
const flush = (ms = 0) => act(async () => { await new Promise(r => setTimeout(r, ms)); });
const editor = () => (host.querySelector('.ProseMirror') as (HTMLElement & { editor?: Editor }) | null)?.editor;

async function mount(path = '/admin/docs/d1') {
  window.history.replaceState(null, '', path);
  host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
  const staff = { me: { user_id: 'me', role: 'OPERATOR' as const, duties: [] }, counts: null, work: null, refreshCounts: () => {}, can: () => true };
  await act(async () => {
    root.render(
      <BrowserRouter><ToastProvider><ConfirmProvider><StaffContext.Provider value={staff}>
        <Routes><Route path="/admin/docs/:id" element={<Docs />} /></Routes>
      </StaffContext.Provider></ConfirmProvider></ToastProvider></BrowserRouter>,
    );
  });
  await flush(30);
  // 편집기는 문서를 열 때 따로 받는다 — 받은 뒤 그려질 때까지 기다린다
  await act(async () => { await import('./DocEditor'); });
  await flush(30);
}

beforeEach(() => {
  for (const f of [api.people, api.doc, api.docTree, api.saveDoc, api.embeds, api.searchEntities, api.docComments]) f.mockReset();
  api.people.mockResolvedValue({ items: [{ user_id: 'me', email: 'me@audeniq.com', name: 'me', role: 'OPERATOR' }] });
  api.docTree.mockResolvedValue({ items: [{ ...detail().doc, has_children: false }] });
  api.embeds.mockImplementation(async (refs: string[]) => ({
    items: Object.fromEntries(refs.map(k => [k, k.startsWith('release')
      ? { kind: 'release', id: 'r1', view: 'qc', title: '새벽의 온도', qc: { checks: { total: 10, passed: 9, blocked: 0 }, loudness: { min_lufs: -9.1, max_lufs: -9.1, max_true_peak: -0.8 }, audio: { sample_rates: [44100], channels: [2] }, issues: [] } }
      : { kind: 'task', id: 't1', view: 'summary', seq: 12, title: '<img src=x onerror=alert(1)>', priority: 'URGENT', column: 'Review', assignee_id: null }])),
  }));
  api.searchEntities.mockResolvedValue({ items: [] });
});
afterEach(async () => { await act(async () => root.unmount()); host.remove(); });

describe('문서 화면', () => {
  it('문서를 열면 임베드를 한 번에 불러오고, 값은 글자로만 그린다', async () => {
    api.doc.mockResolvedValue(detail());
    await mount();
    await flush(50);
    expect((host.querySelector('#doc-title') as HTMLInputElement).value).toBe('멜론 장애');
    expect(api.embeds).toHaveBeenCalledTimes(1);
    expect([...api.embeds.mock.calls[0][0]].sort()).toEqual(['release:r1:qc', 'task:t1']);
    expect(host.textContent).toContain('-9.1 LUFS');
    expect(host.textContent).toContain('P0 Critical');
    // 서버 값 속 태그는 실행되지 않고 글자로 보인다
    expect(host.querySelector('img[src="x"]')).toBeNull();
    expect(host.textContent).toContain('<img src=x onerror=alert(1)>');
  });

  it('보기 권한이면 읽기 전용 (서식 줄·공유 없음)', async () => {
    api.doc.mockResolvedValue(detail('VIEWER'));
    await mount();
    expect(host.querySelector('.ProseMirror')?.getAttribute('contenteditable')).toBe('false');
    expect(host.querySelector('.adm-doc-toolbar')).toBeNull();
    expect([...host.querySelectorAll('button')].some(b => b.textContent?.includes('공유'))).toBe(false);
    expect((host.querySelector('#doc-title') as HTMLInputElement).readOnly).toBe(true);
  });

  it('고치면 잠시 뒤 기준 버전과 정리된 내용으로 저장한다', async () => {
    api.doc.mockResolvedValue(detail());
    api.saveDoc.mockImplementation(async (_id: string, b: DocSaveInput) => ({ id: 'd1', version: b.base_version + 1, status: 'DRAFT', saved_at: '2026-10-04T00:00:00Z' }));
    await mount();
    await act(async () => { editor()?.chain().focus('end').insertContent([{ type: 'text', text: ' 추가', marks: [{ type: 'link', attrs: { href: 'javascript:alert(1)' } }] }]).run(); });
    expect(host.querySelector('.adm-doc-save')?.textContent).toContain('저장 대기');
    await flush(1400);
    expect(api.saveDoc).toHaveBeenCalledTimes(1);
    const [id, body] = api.saveDoc.mock.calls[0] as [string, DocSaveInput];
    expect(id).toBe('d1');
    expect(body.base_version).toBe(4);
    expect(JSON.stringify(body.content)).not.toContain('javascript:');
    expect(host.querySelector('.adm-doc-save')?.textContent).toContain('저장됨');
    // 다음 저장은 새 버전 기준
    await act(async () => { editor()?.commands.insertContent('!'); });
    await flush(1400);
    expect((api.saveDoc.mock.calls[1][1] as DocSaveInput).base_version).toBe(5);
  });

  it('다른 사람이 먼저 고쳤으면(409) 덮어쓰지 않고 알린다', async () => {
    api.doc.mockResolvedValue(detail());
    api.saveDoc.mockRejectedValue(new ApiError('conflict', 409, 'CONFLICT'));
    await mount();
    await act(async () => { editor()?.commands.insertContent('x'); });
    await flush(1400);
    expect(host.querySelector('.adm-doc-conflict')?.textContent).toContain('먼저 고쳤어요');
    await act(async () => { editor()?.commands.insertContent('y'); });
    await flush(1400);
    expect(api.saveDoc).toHaveBeenCalledTimes(1);
  });

  it('없거나 볼 수 없는 문서', async () => {
    api.doc.mockRejectedValue(new ApiError('nf', 404, 'NOT_FOUND'));
    await mount();
    expect(host.textContent).toContain('문서를 찾을 수 없어요');
  });

  it("'/' 명령을 키보드로 골라 AUDENIQ 블록 고르기를 연다", async () => {
    api.doc.mockResolvedValue(detail());
    await mount();
    await act(async () => { editor()?.chain().focus('end').insertContent('/qc').run(); });
    await flush(20);
    const menu = document.querySelector('.adm-doc-suggest');
    expect(menu?.textContent).toContain('QC 결과');
    const dom = editor()?.view.dom;
    await act(async () => { dom?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true })); });
    await flush(20);
    expect(document.body.textContent).toContain('발매 고르기');
    expect(api.searchEntities).toHaveBeenCalledWith('release', '');
  });
});
