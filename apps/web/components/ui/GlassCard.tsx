import React, { forwardRef, type HTMLAttributes } from "react";

export type GlassLevel = "card" | "menu" | "modal";

export interface GlassCardProps extends HTMLAttributes<HTMLDivElement> {
  level?: GlassLevel;
  interactive?: boolean;
}

const LEVEL_CLASSES: Record<GlassLevel, string> = {
  card: "glass-card rounded-2xl",
  menu: "glass-menu rounded-2xl",
  modal: "glass-modal rounded-2xl",
};

export const GlassCard = forwardRef<HTMLDivElement, GlassCardProps>(
  (
    { level = "card", interactive = false, className = "", children, ...props },
    ref,
  ) => {
    const levelClass = LEVEL_CLASSES[level];
    const interactiveClass = interactive
      ? "cursor-pointer group transition-all hover:border-brand-500/30"
      : "";

    return (
      <div
        ref={ref}
        className={`${levelClass} ${interactiveClass} ${className}`.trim()}
        {...props}
      >
        {children}
      </div>
    );
  },
);

GlassCard.displayName = "GlassCard";

export interface GlassCardHeaderProps extends HTMLAttributes<HTMLDivElement> {}

export const GlassCardHeader = forwardRef<HTMLDivElement, GlassCardHeaderProps>(
  ({ className = "", children, ...props }, ref) => {
    return (
      <div
        ref={ref}
        className={`flex items-start justify-between gap-3 border-b border-white/5 pb-3 ${className}`.trim()}
        {...props}
      >
        {children}
      </div>
    );
  },
);

GlassCardHeader.displayName = "GlassCardHeader";

export interface GlassCardTitleProps
  extends HTMLAttributes<HTMLHeadingElement> {
  as?: "h1" | "h2" | "h3" | "h4" | "p";
}

export const GlassCardTitle = forwardRef<
  HTMLHeadingElement,
  GlassCardTitleProps
>(({ as: Component = "h3", className = "", children, ...props }, ref) => {
  return (
    <Component
      ref={ref}
      className={`font-display text-sm font-semibold tracking-tight text-white ${className}`.trim()}
      {...props}
    >
      {children}
    </Component>
  );
});

GlassCardTitle.displayName = "GlassCardTitle";

export interface GlassCardDescriptionProps
  extends HTMLAttributes<HTMLParagraphElement> {}

export const GlassCardDescription = forwardRef<
  HTMLParagraphElement,
  GlassCardDescriptionProps
>(({ className = "", children, ...props }, ref) => {
  return (
    <p
      ref={ref}
      className={`text-xs text-zinc-400 mt-0.5 leading-relaxed ${className}`.trim()}
      {...props}
    >
      {children}
    </p>
  );
});

GlassCardDescription.displayName = "GlassCardDescription";

export interface GlassCardBodyProps extends HTMLAttributes<HTMLDivElement> {}

export const GlassCardBody = forwardRef<HTMLDivElement, GlassCardBodyProps>(
  ({ className = "", children, ...props }, ref) => {
    return (
      <div ref={ref} className={`space-y-3 ${className}`.trim()} {...props}>
        {children}
      </div>
    );
  },
);

GlassCardBody.displayName = "GlassCardBody";

export interface GlassCardFooterProps extends HTMLAttributes<HTMLDivElement> {}

export const GlassCardFooter = forwardRef<HTMLDivElement, GlassCardFooterProps>(
  ({ className = "", children, ...props }, ref) => {
    return (
      <div
        ref={ref}
        className={`mt-4 pt-3 border-t border-zinc-800/80 flex items-center justify-between gap-2 text-2xs font-mono text-zinc-400 ${className}`.trim()}
        {...props}
      >
        {children}
      </div>
    );
  },
);

GlassCardFooter.displayName = "GlassCardFooter";

export default GlassCard;
