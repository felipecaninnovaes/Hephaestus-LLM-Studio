import { forwardRef, type HTMLAttributes, type TdHTMLAttributes, type ThHTMLAttributes } from "react";

export interface TableProps extends HTMLAttributes<HTMLTableElement> {
  containerClassName?: string;
}

/**
 * Tabela canônica do Design System Arcane.
 * Encapsulada em container de vidro óptico (.glass-card) com scroll horizontal suave.
 */
export const Table = forwardRef<HTMLTableElement, TableProps>(
  ({ className = "", containerClassName = "", children, ...props }, ref) => (
    <div
      className={`w-full overflow-x-auto rounded-2xl border border-zinc-800/80 bg-zinc-950/80 shadow-lg backdrop-blur-xl ${containerClassName}`.trim()}
    >
      <table
        ref={ref}
        className={`w-full text-left text-xs font-mono border-collapse ${className}`.trim()}
        {...props}
      >
        {children}
      </table>
    </div>
  ),
);
Table.displayName = "Table";

export const TableHeader = forwardRef<
  HTMLTableSectionElement,
  HTMLAttributes<HTMLTableSectionElement>
>(({ className = "", ...props }, ref) => (
  <thead
    ref={ref}
    className={`border-b border-zinc-800/80 bg-zinc-900/50 backdrop-blur-md text-zinc-400 select-none ${className}`.trim()}
    {...props}
  />
));
TableHeader.displayName = "TableHeader";

export const TableBody = forwardRef<
  HTMLTableSectionElement,
  HTMLAttributes<HTMLTableSectionElement>
>(({ className = "", ...props }, ref) => (
  <tbody
    ref={ref}
    className={`divide-y divide-zinc-800/40 text-zinc-300 ${className}`.trim()}
    {...props}
  />
));
TableBody.displayName = "TableBody";

export const TableFooter = forwardRef<
  HTMLTableSectionElement,
  HTMLAttributes<HTMLTableSectionElement>
>(({ className = "", ...props }, ref) => (
  <tfoot
    ref={ref}
    className={`border-t border-zinc-800/80 bg-zinc-900/50 font-medium text-zinc-300 ${className}`.trim()}
    {...props}
  />
));
TableFooter.displayName = "TableFooter";

export const TableRow = forwardRef<
  HTMLTableRowElement,
  HTMLAttributes<HTMLTableRowElement>
>(({ className = "", ...props }, ref) => (
  <tr
    ref={ref}
    className={`transition-colors hover:bg-white/[0.03] data-[state=selected]:bg-brand-500/10 ${className}`.trim()}
    {...props}
  />
));
TableRow.displayName = "TableRow";

export const TableHead = forwardRef<
  HTMLTableCellElement,
  ThHTMLAttributes<HTMLTableCellElement>
>(({ className = "", ...props }, ref) => (
  <th
    ref={ref}
    className={`px-4 py-3 font-mono text-3xs font-medium uppercase tracking-caps text-zinc-400 select-none [&:has([role=checkbox])]:pr-0 ${className}`.trim()}
    {...props}
  />
));
TableHead.displayName = "TableHead";

export const TableCell = forwardRef<
  HTMLTableCellElement,
  TdHTMLAttributes<HTMLTableCellElement>
>(({ className = "", ...props }, ref) => (
  <td
    ref={ref}
    className={`px-4 py-3 align-middle text-zinc-300 [&:has([role=checkbox])]:pr-0 ${className}`.trim()}
    {...props}
  />
));
TableCell.displayName = "TableCell";

export const TableCaption = forwardRef<
  HTMLTableCaptionElement,
  HTMLAttributes<HTMLTableCaptionElement>
>(({ className = "", ...props }, ref) => (
  <caption
    ref={ref}
    className={`mt-4 text-2xs text-zinc-500 font-mono ${className}`.trim()}
    {...props}
  />
));
TableCaption.displayName = "TableCaption";
