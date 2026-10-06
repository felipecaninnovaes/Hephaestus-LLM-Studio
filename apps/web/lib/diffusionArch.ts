/* Helpers de família/arch de difusão (fonte única das regras 4B vs 9B).
   Flux.2 Klein = família compartilhada (samplers, defaults base); regras
   exclusivas de 4B (destilado, text encoder custom, checkpoint custom) usam
   `isFlux2Klein4b` explicitamente. O alias legado "flux" equivale a 4B. */

export const FLUX2_KLEIN_4B = "flux-2-klein-4b";
export const FLUX2_KLEIN_9B = "flux-2-klein-9b";

export function isFlux2Klein(arch: string | null | undefined): boolean {
  return arch === FLUX2_KLEIN_4B || arch === FLUX2_KLEIN_9B || arch === "flux";
}

export function isFlux2Klein4b(arch: string | null | undefined): boolean {
  return arch === FLUX2_KLEIN_4B || arch === "flux";
}

export function isFlux2Klein9b(arch: string | null | undefined): boolean {
  return arch === FLUX2_KLEIN_9B;
}

/** Normaliza alias legado ("flux") p/ o arch canônico; demais passam. */
export function normalizeDiffusionArch<T extends string | null | undefined>(arch: T): T | typeof FLUX2_KLEIN_4B {
  return arch === "flux" ? FLUX2_KLEIN_4B : arch;
}

/** LoRA é compatível com o arch efetivo? arch nulo (legado) sempre passa (regra do manager). */
export function isLoraCompatible(
  loraArch: string | null | undefined,
  effectiveArch: string | null | undefined,
): boolean {
  if (!loraArch || !effectiveArch) return true;
  return normalizeDiffusionArch(loraArch) === normalizeDiffusionArch(effectiveArch);
}
