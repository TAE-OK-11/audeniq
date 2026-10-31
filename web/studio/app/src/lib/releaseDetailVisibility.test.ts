import { describe, expect, it } from 'vitest';
import { releaseDetailVisibility } from './releaseDetailVisibility';

describe('release detail stage visibility', () => {
  it('shows only information for an empty draft', () => {
    expect(releaseDetailVisibility('draft', 0, false)).toEqual({ overview: true, stats: false, delivery: false, manage: false });
  });
  it('preserves pre-submission splits input and actual documents', () => {
    expect(releaseDetailVisibility('draft', 1, true)).toEqual({ overview: true, stats: false, delivery: true, manage: true });
  });
  it.each(['ready', 'review', 'needs', 'scheduled', 'rejected', 'cancelled'])('retains submitted tabs for %s', status => {
    expect(releaseDetailVisibility(status, 0, false)).toEqual({ overview: true, stats: false, delivery: true, manage: true });
  });
  it('has no separate publishing tab (it is a section of 배급·계약)', () => {
    expect(releaseDetailVisibility('review', 2, false)).not.toHaveProperty('publishing');
  });
  it.each(['live', 'closed'])('opens reports after release (%s)', status => {
    expect(releaseDetailVisibility(status, 0, false).stats).toBe(true);
  });
  it('never shows the change history to users', () => {
    expect(releaseDetailVisibility('live', 3, true)).not.toHaveProperty('history');
  });
});
