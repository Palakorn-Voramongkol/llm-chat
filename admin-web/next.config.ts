import type { NextConfig } from "next";

const ADMIN_API_ORIGIN = process.env.ADMIN_API_ORIGIN ?? "http://localhost:7676";

const nextConfig: NextConfig = {
  output: "standalone", // Phase E packages .next/standalone in node:20-alpine
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
