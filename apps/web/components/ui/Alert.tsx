"use client";

import type { ReactElement, ReactNode } from "react";
import {
  IconAlertTriangle,
  IconCheck,
  IconInfo,
  IconX,
} from "@/components/icons";

export type AlertVariant = "info" | "warning" | "danger" | "success";

export interface AlertProps {
  variant?: AlertVariant;
  title?: string;
  children: ReactNode;
  icon?: ReactNode;
  action?: ReactNode;
  onClose?: () => void;
  className?: string;
}

const VARIANT_CONFIG: Record<
  AlertVariant,
  {
    border: string;
    bg: string;
    text: string;
    icon: ReactElement;
  }
> = {
  info: {
    border: "border-status-telemetry/30",
    bg: "bg-status-telemetry/10",
    text: "text-status-telemetry",
    icon: <IconInfo className="size-4" />,
  },
  warning: {
    border: "border-status-alert/30",
    bg: "bg-status-alert/10",
    text: "text-status-alert",
    icon: <IconAlertTriangle className="size-4" />,
  },
  danger: {
    border: "border-status-danger/30",
    bg: "bg-status-danger/10",
    text: "text-status-danger",
    icon: <IconAlertTriangle className="size-4" />,
  },
  success: {
    border: "border-status-success/30",
    bg: "bg-status-success/10",
    text: "text-status-success",
    icon: <IconCheck className="size-4" />,
  },
};

/**
 * Banner de alerta canônico do Design System Arcane.
 * Implementa role="alert" com cores semânticas e superfícies translúcidas.
 */
export function Alert({
  variant = "info",
  title,
  children,
  icon,
  action,
  onClose,
  className = "",
}: AlertProps) {
  const config = VARIANT_CONFIG[variant];
  const renderedIcon = icon ?? config.icon;

  return (
    <div
      role="alert"
      className={`relative flex items-start gap-3 rounded-xl border p-4 text-xs backdrop-blur-md ${config.border} ${config.bg} ${className}`.trim()}
    >
      {/* Ícone semântico */}
      <span className={`shrink-0 pt-0.5 ${config.text}`}>{renderedIcon}</span>

      {/* Conteúdo */}
      <div className="flex-1 min-w-0">
        {title && (
          <h4 className="font-display font-semibold text-white tracking-tight mb-1 text-xs">
            {title}
          </h4>
        )}
        <div className="text-zinc-300 leading-relaxed font-normal">{children}</div>
      </div>

      {/* Ação ou botão de fechar */}
      {(action || onClose) && (
        <div className="flex items-center gap-2 shrink-0 ml-2">
          {action}
          {onClose && (
            <button
              type="button"
              onClick={onClose}
              aria-label="Fechar alerta"
              className="rounded-lg p-1 text-zinc-400 hover:text-white hover:bg-white/10 transition-colors cursor-pointer"
            >
              <IconX className="size-3.5" />
            </button>
          )}
        </div>
      )}
    </div>
  );
}
