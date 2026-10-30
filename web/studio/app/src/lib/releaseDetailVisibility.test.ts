import { describe, expect, it } from 'vitest';
import { releaseDetailVisibility } from './releaseDetailVisibility';

describe('release detail stage visibility', () => {
  it('shows only information for an empty draft', () => {
    expect(releaseDetailVisibility('draft', 0, 0, false)).toEqual({ overview: true, stats: false, delivery: false, publishing: false, manage: false, history: false });
  });
  it('preserves pre-submission splits input and actual records/documents', () => {
    expect(releaseDetailVisibility('draft', 1, 1, true)).toEqual({ overview: true, stats: false, delivery: true, publishing: false, manage: true, history: true });
  });
  it.each(['ready', 'review', 'needs', 'scheduled', 'rejected', 'cancelled'])('retains submitted tabs for %s', status => {
    expect(releaseDetailVisibility(status, 0, 0, false)).toEqual({ overview: true, stats: false, delivery: true, publishing: false, manage: true, history: true });
  });
  it('opens publishing once a submitted release has tracks', () => {
    expect(releaseDetailVisibility('review', 2, 0, false).publishing).toBe(true);
  });
  it.each(['live', 'closed'])('opens reports after release (%s)', status => {
    expect(releaseDetailVisibility(status, 0, 0, false).stats).toBe(true);
  });
});
