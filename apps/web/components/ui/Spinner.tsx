import type React from "react";

export type SpinnerTone = "brand" | "current" | "white";

export interface SpinnerProps extends React.HTMLAttributes<HTMLSpanElement> {
  tone?: SpinnerTone;
}

const TONE_CLASSES: Record<SpinnerTone, string> = {
  brand: "border-brand-500/30 border-t-brand-400",
  current: "border-current/30 border-t-current",
  white: "border-white/30 border-t-white",
};

/**
 * Indicador circular de carregamento canônico do Arcane UI.
 * Tamanho via className (ex.: `size-4`, `size-6`, `size-8`);
 * tom semântico via `tone` (padrão: brand).
 */
export function Spinner({
  tone = "brand",
  className = "",
  ...props
}: SpinnerProps) {
  return (
    <span
      aria-hidden="true"
      className={`inline-block shrink-0 animate-spin rounded-full border-2 ${TONE_CLASSES[tone]} ${className}`.trim()}
      {...props}
    />
  );
}

export default Spinner;
