import type { NextConfig } from "next";

const ADMIN_API_ORIGIN = process.env.ADMIN_API_ORIGIN ?? "http://localhost:7676";

// Defense-in-depth headers for every response. The Console has no known HTML/JS
// injection sink (the markdown renderer is fed only hardcoded strings and never
// parses raw HTML), so these are a backstop. `frame-ancestors 'none'` +
// X-Frame-Options block clickjacking; `object-src`/`base-uri`/`form-action`
// lock down injection levers. script/style keep 'unsafe-inline'/'unsafe-eval'
// because this Next build emits inline bootstrap without nonces — tighten to a
// nonce-based policy if that changes.
const CSP = [
  "default-src 'self'",
  "script-src 'self' 'unsafe-inline' 'unsafe-eval'",
  "style-src 'self' 'unsafe-inline'",
  "img-src 'self' data: blob:",
  "font-src 'self' data:",
  "connect-src 'self'",
  "frame-ancestors 'none'",
  "object-src 'none'",
  "base-uri 'self'",
  "form-action 'self'",
].join("; ");

const SECURITY_HEADERS = [
  { key: "Content-Security-Policy", value: CSP },
  { key: "X-Frame-Options", value: "DENY" },
  { key: "X-Content-Type-Options", value: "nosniff" },
  { key: "Referrer-Policy", value: "no-referrer" },
  { key: "Permissions-Policy", value: "camera=(), microphone=(), geolocation=()" },
];

const nextConfig: NextConfig = {
  output: "standalone", // Phase E packages .next/standalone in node:20-alpine
  async headers() {
    return [{ source: "/:path*", headers: SECURITY_HEADERS }];
  },
  async rewrites() {
    return [
      { source: "/api/:path*", destination: `${ADMIN_API_ORIGIN}/api/:path*` },
      { source: "/login", destination: `${ADMIN_API_ORIGIN}/login` },
      { source: "/callback", destination: `${ADMIN_API_ORIGIN}/callback` },
      { source: "/logout", destination: `${ADMIN_API_ORIGIN}/logout` },
    ];
  },
};

export default nextConfig;
