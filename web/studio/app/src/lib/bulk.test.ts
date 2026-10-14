import { describe, expect, it } from 'vitest';
import { applyEdit, createPayload, groups, payloadFromDetail, parseCsv, readRows, rowsFromDetail, toCsv, validate, TEMPLATE_ROWS } from './bulk';
import type { ReleaseDetail } from '../api/types';

describe('bulk CSV', () => {
  it('parses quotes, commas, newlines and BOM; writes what it reads', () => {
    expect(parseCsv('﻿a,b\r\n"x, y","say ""hi""\nthere"\n')).toEqual([['a', 'b'], ['x, y', 'say "hi"\nthere']]);
    const csv = toCsv(TEMPLATE_ROWS);
    const { rows, issues } = readRows(csv);
    expect(issues).toEqual([]);
    expect(rows[1].version).toBe('Inst.');
    expect(rows[0].release_title).toBe('여름의 끝');
  });

  it('neutralises spreadsheet formulas on the way out and back', () => {
    const csv = toCsv([{ ...TEMPLATE_ROWS[0], track_title: '=HYPERLINK("x")' }]);
    expect(csv).toContain(`"'=HYPERLINK(""x"")"`);
    expect(readRows(csv).rows[0].track_title).toBe('=HYPERLINK("x")');
  });

  it('rejects unknown and missing columns', () => {
    const { issues } = readRows('발매명,모르는열\nx,y\n');
    expect(issues.map(i => i.message).join(' ')).toContain('알 수 없는 열');
    expect(issues.some(i => i.column === 'artist')).toBe(true);
  });
});

describe('bulk validation', () => {
  it('accepts the template and flags bad cells by row and column', () => {
    expect(validate(TEMPLATE_ROWS, 'create', '2026-10-14')).toEqual([]);
    const bad = TEMPLATE_ROWS.map(r => ({ ...r }));
    bad[0].isrc = 'KR-000-26-00000';
    bad[1].release_date = '2027-01-16';
    bad[2].release_date = '2026-10-20';
    bad[2].type = 'mixtape';
    const issues = validate(bad, 'create', '2026-10-14');
    const at = (row: number, column: string) => issues.some(i => i.row === row && i.column === column);
    expect(at(1, 'isrc')).toBe(true);
    expect(at(2, 'release_date')).toBe(true); // differs within the release
    expect(at(3, 'release_date')).toBe(true); // too soon
    expect(at(3, 'type')).toBe(true);
  });

  it('needs release ids to edit and none to create; duplicate track numbers', () => {
    expect(validate([{ ...TEMPLATE_ROWS[2] }], 'edit')[0].column).toBe('release_id');
    const dup = [TEMPLATE_ROWS[0], { ...TEMPLATE_ROWS[1], track_no: '1' }];
    expect(validate(dup, 'create', '2026-10-14').some(i => i.column === 'track_no')).toBe(true);
  });
});

describe('bulk payloads', () => {
  it('groups rows into releases with tracks in order and fills rights lines', () => {
    const gs = groups([TEMPLATE_ROWS[1], TEMPLATE_ROWS[0], TEMPLATE_ROWS[2]]);
    expect(gs).toHaveLength(2);
    const p = createPayload(gs[0]);
    expect(p.tracks.map(t => t.title)).toEqual(['여름의 끝', '밤바다']);
    expect(p.type).toBe('ep');
    expect(p.phonogram).toBe('2027 한결 뮤직');
    expect(createPayload(gs[1]).type).toBe('single');
    expect(p.genre).toBe('Indie Pop');
    expect(createPayload({ key: 'x', rows: [{ row: { ...TEMPLATE_ROWS[2], genre: '시티팝' }, line: 1 }] }).genreCustom).toBe('시티팝');
  });

  it('exports a draft and applies edited rows, keeping server track ids', () => {
    const d = {
      id: 'r1', title: '첫눈', status: 'draft', release_date: '2027-02-01', created_at: '', track_count: 1, tracks: [],
      draft: { artist: '서린', type: 'single', genre: 'Ballad', label: 'L', upc: '', notes: '', coverName: '', territories: ['WORLD'],
        platforms: [], ownership: '서린', phonogram: '2027 서린', copyright: '2027 서린', rightsChecks: {},
        draftTracks: [{ id: 't1', serverId: 's1', title: '첫눈', version: '', isrc: '', composers: '서린', lyricists: '', arrangers: '',
          performers: '', producer: '', lyrics: '가사', audioName: 'a.wav', audioSize: 1, explicit: false, duration: '3:00', assetId: 'as1' }] },
    } as unknown as ReleaseDetail;
    const rows = rowsFromDetail(d);
    expect(rows[0].release_id).toBe('r1');
    rows[0].track_title = '첫눈 (겨울)';
    rows.push({ ...rows[0], track_no: '2', track_title: '둘째 곡' });
    const g = groups(rows)[0];
    const p = applyEdit(payloadFromDetail(d), g);
    expect(p.tracks[0]).toMatchObject({ serverId: 's1', assetId: 'as1', lyrics: '가사', title: '첫눈 (겨울)' });
    expect(p.tracks[1].title).toBe('둘째 곡');
  });
});
