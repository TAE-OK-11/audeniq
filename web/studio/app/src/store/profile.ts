// 아티스트 프로필 공유 스토어 — Profile(편집), Dashboard(인사말), SignatureModal(서명자 기본값)이 공유
import { createStore } from '../lib/store';

export interface ProfileInfo {
  name: string;
  email: string;
  bio: string;
  country: string;
}

const INITIAL: ProfileInfo = {
  name: '서린',
  email: 'artist.demo@example.com',
  bio: '도시의 풍경과 하루의 감정을 음악으로 기록합니다.',
  country: 'KR',
};

const store = createStore<ProfileInfo>(INITIAL, {
  persist: 'profile',
  revive: (raw, fallback) => (raw && typeof raw === 'object' ? { ...fallback, ...(raw as Partial<ProfileInfo>) } : fallback),
});

export const useProfile = store.use;
export const getProfileSnapshot = store.get;
export function setProfile(p: ProfileInfo): void {
  store.set(p);
}
export function patchProfile(p: Partial<ProfileInfo>): void {
  store.set(prev => ({ ...prev, ...p }));
}
