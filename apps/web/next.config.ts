import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  // Dev: aceita acesso via LAN (ex.: http://10.15.10.3:3000) — sem isso o
  // Turbopack rejeita o Socket HMR de origem externa (`ERR_INVALID_HTTP_RESPONSE`)
  // e a hidratação não completa: os handlers de /login nunca registram e o
  // submit vira um GET nativo (a senha sai na URL!). Produção não usa este campo.
  allowedDevOrigins: ["10.15.10.3"],
  async rewrites() {
    const apiBase =
      process.env.API_INTERNAL_URL ?? "http://localhost:8080";
    return [
      {
        source: "/api/:path*",
        destination: `${apiBase}/api/:path*`,
      },
    ];
  },
};

export default nextConfig;
