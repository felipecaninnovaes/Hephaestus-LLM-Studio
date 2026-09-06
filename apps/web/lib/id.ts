/**
 * Gera um id único client-side.
 *
 * `crypto.randomUUID()` só existe em secure contexts (HTTPS ou localhost).
 * O Studio também é acessado por IP da rede local em HTTP puro — origem
 * non-secure, onde a API é `undefined` (provado: http://10.15.10.3 lança
 * `TypeError: crypto.randomUUID is not a function`). Já
 * `crypto.getRandomValues` está disponível em qualquer contexto.
 */
export function newId(): string {
  if (
    typeof crypto !== "undefined" &&
    typeof crypto.randomUUID === "function"
  ) {
    return crypto.randomUUID();
  }
  // UUID v4 via getRandomValues (RFC 4122 §4.4).
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  bytes[6] = (bytes[6] & 0x0f) | 0x40; // versão 4
  bytes[8] = (bytes[8] & 0x3f) | 0x80; // variante RFC
  const hex = Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join(
    "",
  );
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}
