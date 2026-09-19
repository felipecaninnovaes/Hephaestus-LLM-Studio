import type { NextConfig } from "next";

const devOrigins = process.env.DEV_ALLOWED_ORIGIN
  ? [process.env.DEV_ALLOWED_ORIGIN, "10.15.10.3"]
  : ["10.15.10.3"];

const nextConfig: NextConfig = {
  output: "standalone",
  // Dev: aceita acesso via LAN (ex.: http://10.15.10.3:3000 ou DEV_ALLOWED_ORIGIN) — sem isso o
  // Turbopack rejeita o Socket HMR de origem externa (`ERR_INVALID_HTTP_RESPONSE`)
  // e a hidratação não completa: os handlers de /login nunca registram e o
  // submit vira um GET nativo (a senha sai na URL!). Produção não usa este campo.
  allowedDevOrigins: devOrigins,
  experimental: {
    proxyClientMaxBodySize: "8200mb",
    // Complete de upload chunked (md5 + PUT S3 de até 8 GiB) excede o default de 30s do proxy dev.
    proxyTimeout: 900_000,
  },
  async rewrites() {
    const apiBase =
      process.env.API_INTERNAL_URL ?? "http://localhost:8080";
    return [
      {
        source: "/api/:path*",
        destination: `${apiBase}/api/:path*`,
      },
      {
        source: "/health",
        destination: `${apiBase}/health`,
      },
    ];
  },
  async redirects() {
    return [
      {
        source: "/orchestrators",
        destination: "/environments",
        permanent: true,
      },
      {
        source: "/orquestradores",
        destination: "/environments",
        permanent: true,
      },
    ];
  },
};

export default nextConfig;
