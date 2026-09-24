// SEO uses one primary hostname. Change this and the absolute URLs in the
// landing HTML / sitemap / robots together if AUDENIQ chooses another domain.
export const PRIMARY_HOST = "audeniq.com";
const PRIMARY_ORIGIN = `https://${PRIMARY_HOST}`;

export function resolveAssetPath(hostname, pathname, searchParams = new URLSearchParams()) {
  const host = String(hostname || "").toLowerCase().replace(/\.$/, "");
  const path = pathname || "/";

  // Static files must bypass the host router, even on STUDIO.
  if (path.startsWith("/assets/") || path === "/favicon.ico" || path === "/robots.txt" || path === "/sitemap.xml") {
    return path;
  }

  // Host-based routing for the dedicated STUDIO domain.
  if (host === "studio" || host.startsWith("studio.")) return "/studio/index.html";

  // Path-based STUDIO preview on workers.dev / localhost. Never indexed.
  if (searchParams.get("site") === "studio" || /^\/studio(?:\/|$)/.test(path)) {
    return "/studio/index.html";
  }
  return "/index.html";
}

export function isPublicCanonicalHost(hostname) {
  return String(hostname || "").toLowerCase().replace(/\.$/, "") === PRIMARY_HOST;
}

function withSecurityHeaders(response, { isStudio = false, isCanonical = false } = {}) {
  const headers = new Headers(response.headers);
  headers.set("X-Content-Type-Options", "nosniff");
  headers.set("Referrer-Policy", "strict-origin-when-cross-origin");
  headers.set("Permissions-Policy", "camera=(), microphone=(), geolocation=(), payment=()");
  headers.set("Content-Security-Policy", "frame-ancestors 'none'; base-uri 'self'; object-src 'none'");
  headers.set("Cross-Origin-Opener-Policy", "same-origin");
  headers.set("Cache-Control", "no-cache");
  headers.set("X-AUDENIQ-Site", isStudio ? "studio" : "landing");
  // The HTML meta tag alone is insufficient for workers.dev and other preview hosts.
  // Likewise STUDIO should never appear in search, irrespective of its hostname.
  if (isStudio || !isCanonical || response.status >= 400) {
    headers.set("X-Robots-Tag", "noindex, nofollow, noarchive");
  }
  return new Response(response.body, { status: response.status, statusText: response.statusText, headers });
}

function plainText(body, contentType = "text/plain; charset=utf-8", status = 200) {
  return new Response(body, { status, headers: { "Content-Type": contentType } });
}

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    const path = url.pathname;
    const host = url.hostname.toLowerCase().replace(/\.$/, "");
    const isCanonical = isPublicCanonicalHost(host);
    const isStudioHost = host === "studio" || host.startsWith("studio.");

    // Only the apex domain should be indexed. Preserve the path and query in redirects.
    if (host === `www.${PRIMARY_HOST}`) {
      const destination = new URL(url.pathname + url.search + url.hash, PRIMARY_ORIGIN);
      return Response.redirect(destination, 308);
    }

    // Robots is host-aware: don't accidentally invite indexing the preview or STUDIO.
    if (path === "/robots.txt" && !isCanonical) {
      return withSecurityHeaders(plainText("User-agent: *\nDisallow: /\n"), { isCanonical: false, isStudio: isStudioHost });
    }
    if (path === "/sitemap.xml" && !isCanonical) {
      return withSecurityHeaders(plainText("Not found", "text/plain; charset=utf-8", 404), { isCanonical: false, isStudio: isStudioHost });
    }
    if (path === "/health") {
      return withSecurityHeaders(Response.json({ ok: true, service: "AUDENIQ Web", host }), { isCanonical: false, isStudio: isStudioHost });
    }

    // The landing page is a single document with fragment sections, not a catch-all
    // that should answer 200 to every arbitrary URL (which causes soft-404 SEO issues).
    const isStudioPreview = url.searchParams.get("site") === "studio" || /^\/studio(?:\/|$)/.test(path);
    if (!isStudioHost && !isStudioPreview && path === "/index.html") {
      return Response.redirect(new URL("/", url), 308);
    }
    const sharedFile = path.startsWith("/assets/") || path === "/favicon.ico" || path === "/robots.txt" || path === "/sitemap.xml";
    if (!isStudioHost && !isStudioPreview && !sharedFile && path !== "/") {
      return withSecurityHeaders(plainText("Not found", "text/plain; charset=utf-8", 404), { isCanonical });
    }

    const assetPath = resolveAssetPath(host, path, url.searchParams);
    const assetUrl = new URL(assetPath, url);
    const assetRequest = new Request(assetUrl, request);
    const response = await env.ASSETS.fetch(assetRequest);
    const isStudio = assetPath.startsWith("/studio/") || isStudioHost;
    return withSecurityHeaders(response, { isStudio, isCanonical });
  }
};
