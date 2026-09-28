import { useSyncExternalStore } from 'react';
import { MOCK } from './mode';

type DspName = { code: string; slug: string; name: string };
let names: DspName[] = [];
let version = 0;
const listeners = new Set<() => void>();
const subscribe = (listener: () => void) => {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
};
const snapshot = () => version;
export function setDspNames(items: DspName[]) {
  names = items;
  version++;
  listeners.forEach(listener => listener());
}

const MOCK_NAMES = ['멜론', '지니', 'FLO', '벅스', 'Spotify', 'Apple Music', 'YouTube Music', 'Amazon Music', 'TIDAL', 'Deezer', 'Qobuz'];
const MOCK_SLUGS = ['melon', 'genie', 'flo', 'bugs', 'spotify', 'apple', 'youtube', 'amazon', 'tidal', 'deezer', 'qobuz'];
export function dspLabel(value: string): string {
  if (MOCK) {
    const code = /^D-(\d+)$/.exec(value);
    const index = code ? Number(code[1]) - 1 : MOCK_SLUGS.indexOf(value);
    return MOCK_NAMES[index] ?? value;
  }
  return names.find(d => d.code === value || d.slug === value)?.name ?? value;
}

export function useDspLabel() {
  useSyncExternalStore(subscribe, snapshot, snapshot);
  return dspLabel;
}
