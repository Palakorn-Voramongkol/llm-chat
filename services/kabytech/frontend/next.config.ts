import type { NextConfig } from "next";

const KABY_BACKEND_ORIGIN = process.env.KABY_BACKEND_ORIGIN ?? "http://localhost:7670";

const nextConfig: NextConfig = {
  output: "standalone",
  async headers() {
    return [{
      source: "/:path*",
      headers: [
        { key: "X-Frame-Options", value: "DENY" },
        { key: "X-Content-Type-Options", value: "nosniff" },
        { key: "Referrer-Policy", value: "no-referrer" },
      ],
    }];
  },
  async rewrites() {
    return [
      { source: "/api/:path*", destination: `${KABY_BACKEND_ORIGIN}/api/:path*` },
      // /login is now a Next page (custom login UI); the authorize start is
      // /api/login/start (proxied via /api/:path*). Do NOT proxy /login.
      { source: "/callback", destination: `${KABY_BACKEND_ORIGIN}/callback` },
      { source: "/logout", destination: `${KABY_BACKEND_ORIGIN}/logout` },
    ];
  },
};

export default nextConfig;
