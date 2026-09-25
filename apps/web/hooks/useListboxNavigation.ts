"use client";

import { useCallback, useEffect, useState, type Dispatch, type KeyboardEvent, type RefObject, type SetStateAction } from "react";

export interface ListboxItem {
  disabled?: boolean;
}

export interface UseListboxNavigationOptions<TItem extends ListboxItem> {
  items: readonly TItem[];
  isOpen: boolean;
  setIsOpen: (open: boolean) => void;
  optionsListRef: RefObject<HTMLElement | null>;
  triggerRef?: RefObject<HTMLElement | null>;
  onSelect: (item: TItem, index: number) => void;
  selectedIndex?: number;
  disabled?: boolean;
  loading?: boolean;
}

export interface UseListboxNavigationReturn {
  highlightedIndex: number;
  setHighlightedIndex: Dispatch<SetStateAction<number>>;
  handleKeyDown: (e: KeyboardEvent) => void;
}

/**
 * Hook reutilizável para navegação de listbox acessível por teclado (WAI-ARIA).
 * Suporta ArrowDown/Up cíclico com pulo de itens desabilitados, Home/End,
 * Enter/Space para seleção, Escape para fechamento com foco no gatilho
 * e scrollIntoView automático para o item destacado.
 */
export function useListboxNavigation<TItem extends ListboxItem>({
  items,
  isOpen,
  setIsOpen,
  optionsListRef,
  triggerRef,
  onSelect,
  selectedIndex,
  disabled = false,
  loading = false,
}: UseListboxNavigationOptions<TItem>): UseListboxNavigationReturn {
  const [highlightedIndex, setHighlightedIndex] = useState<number>(-1);

  // Inicializa índice ao abrir ou reseta ao fechar
  useEffect(() => {
    if (isOpen) {
      if (selectedIndex !== undefined && selectedIndex >= 0 && selectedIndex < items.length) {
        setHighlightedIndex(selectedIndex);
      } else {
        const firstEnabled = items.findIndex((item) => !item.disabled);
        setHighlightedIndex(firstEnabled >= 0 ? firstEnabled : 0);
      }
    } else {
      setHighlightedIndex(-1);
    }
  }, [isOpen, selectedIndex, items]);

  // Scroll automático do item em destaque para dentro da viewport do listbox
  useEffect(() => {
    if (isOpen && highlightedIndex >= 0 && optionsListRef.current) {
      const el = optionsListRef.current.children[highlightedIndex] as HTMLElement | undefined;
      if (el && typeof el.scrollIntoView === "function") {
        el.scrollIntoView({ block: "nearest" });
      }
    }
  }, [highlightedIndex, isOpen, optionsListRef]);

  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      if (disabled || loading) return;

      if (!isOpen) {
        if (
          e.key === "ArrowDown" ||
          e.key === "ArrowUp" ||
          e.key === "Enter" ||
          e.key === " "
        ) {
          e.preventDefault();
          setIsOpen(true);
        }
        return;
      }

      switch (e.key) {
        case "Escape": {
          e.preventDefault();
          setIsOpen(false);
          triggerRef?.current?.focus();
          break;
        }

        case "ArrowDown": {
          e.preventDefault();
          if (items.length === 0) break;
          let nextIdx = (highlightedIndex + 1) % items.length;
          let attempts = 0;
          while (items[nextIdx]?.disabled && attempts < items.length) {
            nextIdx = (nextIdx + 1) % items.length;
            attempts++;
          }
          setHighlightedIndex(nextIdx);
          break;
        }

        case "ArrowUp": {
          e.preventDefault();
          if (items.length === 0) break;
          let prevIdx = (highlightedIndex - 1 + items.length) % items.length;
          let attempts = 0;
          while (items[prevIdx]?.disabled && attempts < items.length) {
            prevIdx = (prevIdx - 1 + items.length) % items.length;
            attempts++;
          }
          setHighlightedIndex(prevIdx);
          break;
        }

        case "Home": {
          e.preventDefault();
          if (items.length === 0) break;
          const firstEnabled = items.findIndex((item) => !item.disabled);
          if (firstEnabled >= 0) {
            setHighlightedIndex(firstEnabled);
          }
          break;
        }

        case "End": {
          e.preventDefault();
          if (items.length === 0) break;
          for (let i = items.length - 1; i >= 0; i--) {
            if (!items[i]?.disabled) {
              setHighlightedIndex(i);
              break;
            }
          }
          break;
        }

        case "Enter":
        case " ": {
          const target = e.target as HTMLElement | null;
          if (e.key === " " && target?.tagName === "INPUT") {
            return;
          }
          e.preventDefault();
          const targetOpt = items[highlightedIndex];
          if (targetOpt && !targetOpt.disabled) {
            onSelect(targetOpt, highlightedIndex);
          }
          break;
        }

        case "Tab": {
          setIsOpen(false);
          break;
        }
      }
    },
    [
      disabled,
      loading,
      isOpen,
      items,
      highlightedIndex,
      onSelect,
      setIsOpen,
      triggerRef,
    ],
  );

  return {
    highlightedIndex,
    setHighlightedIndex,
    handleKeyDown,
  };
}
