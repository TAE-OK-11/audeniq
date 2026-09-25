// 아티스트 프로필 공유 스토어 — 라이브 db.profile 대응
// Profile(편집)과 SignatureModal(서명자 기본값)이 같은 상태를 공유한다.
import { useSyncExternalStore } from 'react';

export interface ProfileInfo {
  name: string;
  email: string;
  bio: string;
  country: string;
}

// mock 초기값 (디자인 테스트용)
let profile: ProfileInfo = {
  name: '서린',
  email: 'artist.demo@example.com',
  bio: '도시의 풍경과 하루의 감정을 음악으로 기록합니다.',
  country: 'KR',
};

const listeners = new Set<() => void>();
function emit() { listeners.forEach(l => l()); }
function subscribe(l: () => void): () => void {
  listeners.add(l);
  return () => { listeners.delete(l); };
}
function getProfile(): ProfileInfo { return profile; }

export function useProfile(): ProfileInfo {
  return useSyncExternalStore(subscribe, getProfile);
}

export function getProfileSnapshot(): ProfileInfo {
  return profile;
}

export function setProfile(p: ProfileInfo): void {
  profile = p;
  emit();
}
