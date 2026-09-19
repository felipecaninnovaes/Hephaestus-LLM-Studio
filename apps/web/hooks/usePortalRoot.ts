"use client";

import { useEffect, useState } from "react";

/**
 * usePortalRoot — retorna `document.body` somente após mount (null no SSR),
 * para overlays de tela cheia escaparem do stacking context do StudioShell.
 *
 * Motivo: o wrapper de conteúdo (`relative z-10`) e a sidebar (`lg:z-30`)
 * criam contextos de empilhamento fixos na raiz do layout. Um overlay
 * `fixed` renderizado dentro deles tem seu `z-index` resolvido LOCALMENTE
 * — badges `z-10`/`z-20` dos cards e a própria sidebar ficam por cima do
 * modal, independentemente do valor canônico (`z-modal` = 400). Portalar
 * para `document.body` coloca o overlay no contexto raiz, onde a escala
 * canônica de `--z-index-*` (globals.css) governa.
 */
export function usePortalRoot(): HTMLElement | null {
  const [root, setRoot] = useState<HTMLElement | null>(null);

  useEffect(() => {
    setRoot(document.body);
  }, []);

  return root;
}
