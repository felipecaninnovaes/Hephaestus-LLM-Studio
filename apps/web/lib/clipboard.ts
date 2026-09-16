/** Copia texto p/ área de transferência com fallback p/ contexto não-seguro
 *  (HTTP em LAN): `navigator.clipboard` só existe em secure context, fora
 *  disso cai no truque do textarea oculto + `document.execCommand("copy")`.
 *  Best-effort: nunca lança — retorna false se nenhum caminho funcionou. */
export async function copyToClipboard(text: string): Promise<boolean> {
  try {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(text);
      return true;
    }
  } catch {
    // Secure context negado (permissão/iframe): tenta o fallback.
  }
  try {
    const ta = document.createElement("textarea");
    ta.value = text;
    ta.setAttribute("readonly", "");
    ta.style.position = "fixed";
    ta.style.top = "-9999px";
    ta.style.opacity = "0";
    document.body.appendChild(ta);
    ta.select();
    ta.setSelectionRange(0, text.length);
    const ok = document.execCommand("copy");
    ta.remove();
    return ok;
  } catch {
    return false;
  }
}
