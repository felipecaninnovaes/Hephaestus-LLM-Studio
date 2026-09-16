import React from "react";
import Link from "next/link";

export interface BreadcrumbItem {
  label: string;
  href?: string;
}

export interface BreadcrumbsProps {
  items: BreadcrumbItem[];
  rootLabel?: string;
  rootHref?: string;
  className?: string;
}

export function Breadcrumbs({
  items,
  rootLabel = "Hephaestus Studio",
  rootHref = "/datasets",
  className = "",
}: BreadcrumbsProps) {
  const allItems = [
    { label: rootLabel, href: rootHref },
    ...items,
  ];
  const lastIndex = allItems.length - 1;

  return (
    <nav
      aria-label="Navegação atual"
      className={`flex min-w-0 items-center space-x-2 font-mono text-xs ${className}`.trim()}
    >
      {allItems.map((item, i) => {
        const isFirst = i === 0;
        const isLast = i === lastIndex;

        return (
          <React.Fragment
            // biome-ignore lint/suspicious/noArrayIndexKey: crumb pode repetir label/href; ordem é posicional por natureza
            key={`${item.href ?? item.label}-${i}`}
          >
            {!isFirst && (
              <span
                aria-hidden="true"
                className="shrink-0 text-zinc-600 select-none"
              >
                /
              </span>
            )}
            {isLast ? (
              <span
                title={item.label}
                className="font-display font-semibold tracking-tight text-zinc-200 truncate"
              >
                {item.label}
              </span>
            ) : item.href ? (
              <Link
                href={item.href}
                title={item.label}
                className={`block truncate text-zinc-500 hover:text-zinc-300 transition-colors ${
                  isFirst ? "hidden sm:inline shrink-0" : "max-w-[140px]"
                }`}
              >
                {item.label}
              </Link>
            ) : (
              <span
                title={item.label}
                className={`block truncate text-zinc-500 ${
                  isFirst ? "hidden sm:inline shrink-0" : "max-w-[140px]"
                }`}
              >
                {item.label}
              </span>
            )}
          </React.Fragment>
        );
      })}
    </nav>
  );
}

export default Breadcrumbs;
