import { NextResponse, type NextRequest } from "next/server";

export function proxy(request: NextRequest) {
  // /login decide por si (chama /me e redireciona se válido) — nunca
  // redirecionar aqui para evitar loop login↔login.
  if (request.nextUrl.pathname.startsWith("/login")) {
    return NextResponse.next();
  }
  // Checar existência do cookie basta; validade é do servidor via /me.
  if (!request.cookies.has("heph_session")) {
    const url = request.nextUrl.clone();
    url.pathname = "/login";
    return NextResponse.redirect(url);
  }
  return NextResponse.next();
}

// /api/* fora do gate: o backend responde o envelope 401 e o rewrite
// precisa repassá-lo intacto (sonda: GET /api/auth/me sem cookie → 401).
export const config = {
  matcher: ["/((?!api|_next/static|_next/image|favicon.ico|.*\\..*).*)"],
};
