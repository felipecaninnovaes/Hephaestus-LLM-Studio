"use client";

import type { ReactNode } from "react";
import { IconAlertTriangle } from "@/components/icons";

export interface FormFieldProps {
  label?: string;
  htmlFor?: string;
  hint?: string;
  error?: string | null;
  required?: boolean;
  children: ReactNode;
  className?: string;
  rightLabelAction?: ReactNode;
}

/**
 * Contêiner e chrome de formulário canônico do Design System Arcane.
 * Provê label monoespaçado em tracking-caps, indicador required e mensagens de erro acessíveis (role="alert").
 */
export function FormField({
  label,
  htmlFor,
  hint,
  error,
  required = false,
  children,
  className = "",
  rightLabelAction,
}: FormFieldProps) {
  const errorId = htmlFor ? `${htmlFor}-error` : undefined;
  const hintId = htmlFor ? `${htmlFor}-hint` : undefined;

  return (
    <div className={`flex flex-col gap-1.5 ${className}`.trim()}>
      {(label || rightLabelAction) && (
        <div className="flex items-center justify-between text-left">
          {label && (
            <label
              htmlFor={htmlFor}
              className="font-mono text-3xs font-medium uppercase tracking-caps text-zinc-400 select-none flex items-center"
            >
              <span>{label}</span>
              {required && (
                <span
                  className="text-status-danger ml-1 select-none"
                  aria-hidden="true"
                >
                  *
                </span>
              )}
            </label>
          )}
          {rightLabelAction && <div>{rightLabelAction}</div>}
        </div>
      )}

      {/* Controle do formulário (Input, Select, Slider, etc.) */}
      <div className="relative">{children}</div>

      {/* Mensagem de Erro com role="alert" */}
      {error && (
        <p
          id={errorId}
          role="alert"
          className="text-2xs text-status-danger font-mono flex items-center gap-1.5 mt-0.5"
        >
          <IconAlertTriangle className="size-3 shrink-0" />
          <span>{error}</span>
        </p>
      )}

      {/* Dica / Hint auxiliar se não houver erro ativo */}
      {!error && hint && (
        <p id={hintId} className="text-2xs text-zinc-500 leading-relaxed mt-0.5">
          {hint}
        </p>
      )}
    </div>
  );
}
