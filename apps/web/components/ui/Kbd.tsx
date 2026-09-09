import React from "react";

export interface KbdProps extends React.HTMLAttributes<HTMLElement> {
  children: React.ReactNode;
  size?: "sm" | "md";
}

export function Kbd({
  children,
  size = "sm",
  className = "",
  ...props
}: KbdProps) {
  const sizeClasses =
    size === "sm" ? "px-1 py-0.5 text-[10px]" : "px-1.5 py-0.5 text-xs";

  return (
    <kbd
      className={`inline-flex items-center justify-center rounded border border-white/10 bg-black/40 font-mono text-zinc-300 backdrop-blur-sm shadow-sm select-none ${sizeClasses} ${className}`.trim()}
      {...props}
    >
      {children}
    </kbd>
  );
}

export default Kbd;
