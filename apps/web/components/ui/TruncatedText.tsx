import React, { forwardRef, type HTMLAttributes } from "react";

export interface TruncatedTextProps extends HTMLAttributes<HTMLElement> {
  text: string;
  lines?: 1 | 2 | 3 | 4;
  as?: "span" | "p" | "div" | "h1" | "h2" | "h3" | "h4";
}

const LINE_CLASSES: Record<number, string> = {
  1: "truncate block",
  2: "line-clamp-2",
  3: "line-clamp-3",
  4: "line-clamp-4",
};

/**
 * The Truncamento Honesto Rule:
 * Garante que qualquer texto truncado na interface sempre exponha
 * a propriedade `title` nativa com o conteúdo integral acessível.
 */
export const TruncatedText = forwardRef<HTMLElement, TruncatedTextProps>(
  ({ text, lines = 1, as: Component = "span", className = "", children, ...props }, ref) => {
    const lineClass = LINE_CLASSES[lines] || "truncate";

    const DynamicComponent = Component as any;

    return (
      <DynamicComponent
        ref={ref}
        title={text}
        className={`${lineClass} ${className}`.trim()}
        {...props}
      >
        {children || text}
      </DynamicComponent>
    );
  },
);

TruncatedText.displayName = "TruncatedText";
export default TruncatedText;
