"use client";

import {
  cloneElement,
  isValidElement,
  type ReactElement,
  type ReactNode,
  useEffect,
  useId,
  useRef,
  useState,
} from "react";

export interface TooltipProps {
  content: ReactNode;
  children: ReactElement;
  side?: "top" | "bottom" | "left" | "right";
  delay?: number;
  className?: string;
  disabled?: boolean;
}

interface TriggerProps {
  onPointerEnter?: (e: React.PointerEvent) => void;
  onPointerLeave?: (e: React.PointerEvent) => void;
  onFocus?: (e: React.FocusEvent) => void;
  onBlur?: (e: React.FocusEvent) => void;
  "aria-describedby"?: string;
}

/**
 * Tooltip óptico do Design System Arcane.
 * Substitui o atributo nativo title="..." por uma prévia acessível (role="tooltip"),
 * responsiva a hover e foco de teclado (WCAG 2.2 AA).
 */
export function Tooltip({
  content,
  children,
  side = "top",
  delay = 200,
  className = "",
  disabled = false,
}: TooltipProps) {
  const [open, setOpen] = useState(false);
  const timerRef = useRef<NodeJS.Timeout | null>(null);
  const id = useId();

  useEffect(() => {
    return () => {
      clearTimeout(timerRef.current ?? undefined);
    };
  }, []);

  if (!isValidElement(children)) {
    return null;
  }

  const sideClasses = {
    top: "bottom-full left-1/2 -translate-x-1/2 mb-2",
    bottom: "top-full left-1/2 -translate-x-1/2 mt-2",
    left: "right-full top-1/2 -translate-y-1/2 mr-2",
    right: "left-full top-1/2 -translate-y-1/2 ml-2",
  }[side];

  const childProps = children.props as TriggerProps;

  const trigger = cloneElement(children as ReactElement<TriggerProps>, {
    onPointerEnter: (e: React.PointerEvent) => {
      childProps.onPointerEnter?.(e);
      if (disabled || !content) return;
      timerRef.current = setTimeout(() => setOpen(true), delay);
    },
    onPointerLeave: (e: React.PointerEvent) => {
      childProps.onPointerLeave?.(e);
      clearTimeout(timerRef.current ?? undefined);
      timerRef.current = null;
      setOpen(false);
    },
    onFocus: (e: React.FocusEvent) => {
      childProps.onFocus?.(e);
      if (disabled || !content) return;
      timerRef.current = setTimeout(() => setOpen(true), delay);
    },
    onBlur: (e: React.FocusEvent) => {
      childProps.onBlur?.(e);
      clearTimeout(timerRef.current ?? undefined);
      timerRef.current = null;
      setOpen(false);
    },
    "aria-describedby": open ? id : childProps["aria-describedby"],
  });

  return (
    <div className="relative inline-flex items-center">
      {trigger}

      {open && (
        <div
          id={id}
          role="tooltip"
          className={`absolute ${sideClasses} z-popover pointer-events-none whitespace-nowrap rounded-lg border border-brand-500/25 bg-zinc-950/95 px-2.5 py-1 font-mono text-2xs text-zinc-200 shadow-2xl backdrop-blur-xl animate-toast-in ${className}`.trim()}
        >
          {content}
        </div>
      )}
    </div>
  );
}
