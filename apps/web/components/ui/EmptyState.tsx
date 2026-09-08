import React from "react";
import Button, { type ButtonVariant } from "./Button";

export interface EmptyStateProps {
  icon?: React.ReactNode;
  title: string;
  description?: string;
  actionLabel?: string;
  onAction?: () => void;
  actionVariant?: ButtonVariant;
  children?: React.ReactNode;
  className?: string;
}

export function EmptyState({
  icon,
  title,
  description,
  actionLabel,
  onAction,
  actionVariant = "secondary",
  children,
  className = "",
}: EmptyStateProps) {
  return (
    <div
      className={`glass-card flex flex-col items-center justify-center rounded-2xl p-10 text-center gap-3 ${className}`.trim()}
    >
      {icon && (
        <div className="flex size-11 items-center justify-center rounded-xl border border-zinc-800 bg-zinc-900/80 text-zinc-400 [&_svg]:size-5">
          {icon}
        </div>
      )}
      <div className="max-w-sm space-y-1">
        <h4 className="font-display text-sm font-semibold text-zinc-200">
          {title}
        </h4>
        {description && (
          <p className="text-xs text-zinc-400 leading-relaxed">
            {description}
          </p>
        )}
      </div>
      {actionLabel && onAction && (
        <div className="mt-2">
          <Button variant={actionVariant} size="sm" onClick={onAction}>
            {actionLabel}
          </Button>
        </div>
      )}
      {children}
    </div>
  );
}

export default EmptyState;
