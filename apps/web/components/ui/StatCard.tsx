import React from "react";

export interface StatCardProps extends React.HTMLAttributes<HTMLDivElement> {
  label: string;
  value: string | number;
  subtext?: React.ReactNode;
  icon?: React.ReactNode;
  iconColor?: string;
  badge?: React.ReactNode;
  onClick?: () => void;
  className?: string;
}

export function StatCard({
  label,
  value,
  subtext,
  icon,
  iconColor = "text-brand-400",
  badge,
  onClick,
  className = "",
  ...props
}: StatCardProps) {
  return (
    <div
      onClick={onClick}
      role={onClick ? "button" : undefined}
      tabIndex={onClick ? 0 : undefined}
      onKeyDown={
        onClick
          ? (e) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                onClick();
              }
            }
          : undefined
      }
      className={`glass-card group rounded-xl p-4 transition-[border-color,box-shadow] hover:border-brand-500/30 ${
        onClick
          ? "cursor-pointer active:scale-[0.99] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-500/70"
          : ""
      } ${className}`.trim()}
      {...props}
    >
      <div className="flex items-center justify-between gap-2">
        <div className="flex items-center space-x-2 text-zinc-300 min-w-0">
          {icon && (
            <span className={`shrink-0 ${iconColor} [&_svg]:size-4`}>
              {icon}
            </span>
          )}
          <span className="font-mono text-[11px] font-semibold tracking-[0.08em] uppercase truncate">
            {label}
          </span>
        </div>
        {badge && <div className="shrink-0">{badge}</div>}
      </div>
      <div className="mt-2 font-mono text-2xl sm:text-3xl font-bold tracking-tight text-white tabular-nums">
        {value}
      </div>
      {subtext && (
        <div className="mt-1 text-xs text-zinc-400 truncate">
          {subtext}
        </div>
      )}
    </div>
  );
}

export default StatCard;
