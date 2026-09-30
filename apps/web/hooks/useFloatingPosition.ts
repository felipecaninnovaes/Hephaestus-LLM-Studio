"use client";

import { useCallback, useEffect, useState, type RefObject } from "react";

export type VerticalPlacement = "bottom" | "top";
export type HorizontalPlacement = "left" | "right";
export type Alignment = "left" | "right" | "auto";
export type FloatingWidth = "trigger" | "auto" | "fixed" | string;

export interface FloatingCoords {
  top?: number;
  bottom?: number;
  left?: number;
  right?: number;
  width?: number;
}

export interface UseFloatingPositionOptions {
  anchorRef: RefObject<HTMLElement | null>;
  isOpen: boolean;
  align?: Alignment;
  menuWidth?: FloatingWidth;
  gap?: number;
  minMenuHeight?: number;
}

export interface UseFloatingPositionReturn {
  coords: FloatingCoords | null;
  placement: VerticalPlacement;
  horizontalPlacement: HorizontalPlacement;
  updatePlacement: () => void;
}

/**
 * Hook reutilizável para posicionamento de elementos flutuantes portaled (dropdowns, selects, menus).
 * Calcula flip vertical (top/bottom), alinhamento horizontal (left/right/auto) e mantém
 * as coordenadas sincronizadas via listeners de resize e scroll.
 */
export function useFloatingPosition({
  anchorRef,
  isOpen,
  align = "auto",
  menuWidth = "trigger",
  gap = 6,
  minMenuHeight = 200,
}: UseFloatingPositionOptions): UseFloatingPositionReturn {
  const [placement, setPlacement] = useState<VerticalPlacement>("bottom");
  const [horizontalPlacement, setHorizontalPlacement] = useState<HorizontalPlacement>("left");
  const [coords, setCoords] = useState<FloatingCoords | null>(null);

  const updatePlacement = useCallback(() => {
    if (typeof window === "undefined") return;
    const anchor = anchorRef.current;
    if (!anchor) return;

    const rect = anchor.getBoundingClientRect();

    let vertical: VerticalPlacement;
    const spaceBelow = window.innerHeight - rect.bottom;
    if (spaceBelow < minMenuHeight && rect.top > minMenuHeight) {
      vertical = "top";
    } else {
      vertical = "bottom";
    }
    setPlacement(vertical);

    let horizontal: HorizontalPlacement;
    if (align === "right") {
      horizontal = "right";
    } else if (align === "left") {
      horizontal = "left";
    } else {
      // Auto: if trigger is close to the right edge of the viewport, align to right
      const spaceRight = window.innerWidth - rect.right;
      horizontal = spaceRight < 240 ? "right" : "left";
    }
    setHorizontalPlacement(horizontal);

    setCoords({
      ...(vertical === "bottom"
        ? { top: rect.bottom + gap }
        : { bottom: window.innerHeight - rect.top + gap }),
      ...(horizontal === "left"
        ? { left: rect.left }
        : { right: window.innerWidth - rect.right }),
      ...(menuWidth === "trigger" || menuWidth === "fixed"
        ? { width: rect.width }
        : {}),
    });
  }, [anchorRef, align, menuWidth, gap, minMenuHeight]);

  useEffect(() => {
    if (!isOpen) {
      setCoords(null);
      return;
    }

    updatePlacement();

    const handleReposition = () => {
      updatePlacement();
    };

    window.addEventListener("resize", handleReposition);
    document.addEventListener("scroll", handleReposition, true);

    return () => {
      window.removeEventListener("resize", handleReposition);
      document.removeEventListener("scroll", handleReposition, true);
    };
  }, [isOpen, updatePlacement]);

  return {
    coords,
    placement,
    horizontalPlacement,
    updatePlacement,
  };
}
