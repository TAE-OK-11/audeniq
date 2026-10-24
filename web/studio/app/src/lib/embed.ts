// 블로그·홈페이지에 붙여 넣는 스마트링크 카드 — iframe 없이 HTML 하나(스타일은 줄 안에).
// 스튜디오는 보안상 다른 사이트 안에 열리지 않으므로, 커버·제목·버튼만 그리고 누르면 스마트링크로 간다.

const esc = (s: string) => s.replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]!));

export interface EmbedInput { url: string; title: string; artist: string; cover?: string; released: boolean }

export function embedHtml({ url, title, artist, cover, released }: EmbedInput): string {
  const img = cover
    ? `<img src="${esc(cover)}" alt="" width="64" height="64" style="width:64px;height:64px;border-radius:12px;object-fit:cover;flex:none">`
    : `<span style="width:64px;height:64px;border-radius:12px;flex:none;background:linear-gradient(135deg,#9DB4FF,#3B63F3)"></span>`;
  return `<a href="${esc(url)}" target="_blank" rel="noopener" style="display:flex;align-items:center;gap:14px;max-width:420px;padding:12px;border-radius:18px;background:#F4F7FE;color:#191F28;text-decoration:none;font-family:Pretendard,-apple-system,BlinkMacSystemFont,'Apple SD Gothic Neo',sans-serif">`
    + img
    + `<span style="display:block;min-width:0;flex:1"><strong style="display:block;font-size:16px;line-height:1.35;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">${esc(title)}</strong>`
    + `<span style="display:block;margin-top:2px;font-size:13px;color:#5B6475;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">${esc(artist)}</span></span>`
    + `<span style="flex:none;padding:8px 14px;border-radius:999px;background:#3B63F3;color:#fff;font-size:13px;font-weight:700">${released ? '듣기' : '미리 보기'}</span></a>`;
}
