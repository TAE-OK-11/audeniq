import { describe, expect, it } from 'vitest';
import { startVitals } from './vitals';

describe('startVitals', () => {
  it('수집 주소도 점검 표시도 없으면 아무것도 하지 않는다', () => {
    localStorage.removeItem('aq.perf');
    expect(startVitals()).toBeNull();
  });
});
