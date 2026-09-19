"use client";

import { useEffect, useRef } from "react";

const FOCUSABLE_SELECTORS = [
  "button:not([disabled])",
  "[href]",
  "input:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  "[tabindex]:not([tabindex='-1'])",
].join(", ");

interface UseFocusTrapOptions {
  /** Se o foco inicial deve ser forçado imediatamente ao ativar */
  autoFocus?: boolean;
}

/**
 * Hook de Focus Trap acessível para diálogos, modais e gavetas (WAI-ARIA).
 * Mantém o foco de navegação por teclado (Tab/Shift+Tab) estritamente
 * contido no elemento referenciado e restaura o foco anterior ao desmontar/fechar.
 */
export function useFocusTrap<T extends HTMLElement>(
  enabled: boolean,
  options: UseFocusTrapOptions = {},
) {
  const containerRef = useRef<T | null>(null);
  const previousFocusedElement = useRef<HTMLElement | null>(null);
  const { autoFocus = true } = options;

  useEffect(() => {
    if (!enabled || typeof document === "undefined") return;

    // Guarda o elemento ativo anterior para restauração no fechamento
    previousFocusedElement.current =
      document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null;

    const container = containerRef.current;
    if (!container) return;

    // Aplica foco inicial
    if (autoFocus) {
      const focusableElements = container.querySelectorAll<HTMLElement>(
        FOCUSABLE_SELECTORS,
      );
      if (focusableElements.length > 0) {
        // Usa rAF para garantir que o elemento já está pintado e visível
        requestAnimationFrame(() => {
          focusableElements[0]?.focus();
        });
      } else {
        container.setAttribute("tabindex", "-1");
        container.focus();
      }
    }

    function handleKeyDown(event: KeyboardEvent) {
      if (event.key !== "Tab" || !containerRef.current) return;

      const focusables = Array.from(
        containerRef.current.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTORS),
      ).filter((el) => el.offsetParent !== null || el.offsetWidth > 0);

      if (focusables.length === 0) {
        event.preventDefault();
        return;
      }

      const firstElement = focusables[0];
      const lastElement = focusables[focusables.length - 1];

      if (event.shiftKey) {
        if (
          document.activeElement === firstElement ||
          document.activeElement === containerRef.current
        ) {
          event.preventDefault();
          lastElement?.focus();
        }
      } else {
        if (document.activeElement === lastElement) {
          event.preventDefault();
          firstElement?.focus();
        }
      }
    }

    document.addEventListener("keydown", handleKeyDown);

    return () => {
      document.removeEventListener("keydown", handleKeyDown);
      // Restaura o foco para o elemento original
      if (previousFocusedElement.current && document.body.contains(previousFocusedElement.current)) {
        previousFocusedElement.current.focus();
      }
    };
  }, [enabled, autoFocus]);

  return containerRef;
}
