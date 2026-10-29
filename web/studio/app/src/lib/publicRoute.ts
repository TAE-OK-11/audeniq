/** 공개 페이지(스마트링크·아티스트 페이지, 예전 /l/·/a/)인지 — main.tsx가 공개 앱과 스튜디오 앱 중 무엇을 받을지 고른다 */
export const isPublicPath = (path: string) => /^\/(link\/(release|artist)\/|l\/|a\/)/.test(path);
