import Link from "next/link";
import { IconMoreVertical } from "@/components/icons";
import { formatPercent, formatRelativeTime } from "@/lib/format";
import { TYPE_LABELS, type Dataset } from "@/types/studio";

export default function DatasetTable({
  datasets,
  onContextMenu,
}: {
  datasets: Dataset[];
  onContextMenu?: (dataset: Dataset, x: number, y: number) => void;
}) {
  return (
    <div className="overflow-x-auto">
      <table className="w-full text-xs">
        <thead>
          <tr className="tracking-caps text-left font-mono uppercase text-zinc-500">
            <th className="px-3 py-2 font-medium">Nome</th>
            <th className="px-3 py-2 font-medium">Formato-Tarefa</th>
            <th className="px-3 py-2 font-medium">Imagens</th>
            <th className="px-3 py-2 font-medium">Progresso</th>
            <th className="px-3 py-2 font-medium">Origem Storage</th>
            <th className="px-3 py-2 font-medium">Modificado</th>
            <th className="px-3 py-2 font-medium">Ações</th>
          </tr>
        </thead>
        <tbody>
          {datasets.map((d) => {
            const pct = formatPercent(d.labeledCount, d.imagesCount);
            return (
              <tr
                key={d.id}
                onContextMenu={
                  onContextMenu
                    ? (e) => {
                        e.preventDefault();
                        onContextMenu(d, e.clientX, e.clientY);
                      }
                    : undefined
                }
                className="border-t border-zinc-800/80 transition-colors hover:bg-brand-500/5"
              >
                <td className="max-w-56 px-3 py-2.5">
                  <Link href={`/datasets/${d.id}`} className="block min-w-0">
                    <span className="block truncate text-[13px] font-medium text-zinc-100" title={d.title}>
                      {d.title}
                    </span>
                    <span className="block truncate font-mono text-[11px] text-zinc-500" title={d.slug}>
                      {d.slug}
                    </span>
                  </Link>
                </td>
                <td className="whitespace-nowrap px-3 py-2.5 text-zinc-300">
                  <Link href={`/datasets/${d.id}`} className="block" title={TYPE_LABELS[d.type]}>
                    {TYPE_LABELS[d.type]}
                  </Link>
                </td>
                <td className="px-3 py-2.5 font-mono text-zinc-200">
                  <Link href={`/datasets/${d.id}`} className="block">
                    {d.imagesCount}
                  </Link>
                </td>
                <td className="px-3 py-2.5">
                  <Link href={`/datasets/${d.id}`} className="block min-w-28" title={`${pct} rotulado`}>
                    <span className="mb-1 block h-1 overflow-hidden rounded-full bg-zinc-800">
                      <span
                        className="block h-full rounded-full bg-[#34d399]"
                        style={{
                          width:
                            d.imagesCount > 0
                              ? `${Math.min(100, Math.round((d.labeledCount / d.imagesCount) * 100))}%`
                              : "0%",
                        }}
                      />
                    </span>
                    <span className="font-mono text-[11px] text-zinc-400">
                      {pct}
                    </span>
                  </Link>
                </td>
                <td className="max-w-40 px-3 py-2.5">
                  <Link
                    href={`/datasets/${d.id}`}
                    className="block truncate font-mono text-zinc-400"
                    title={d.source ?? "—"}
                  >
                    {d.source ?? "—"}
                  </Link>
                </td>
                <td className="whitespace-nowrap px-3 py-2.5 font-mono text-zinc-400">
                  <Link href={`/datasets/${d.id}`} className="block" title={formatRelativeTime(d.lastModified)}>
                    {formatRelativeTime(d.lastModified)}
                  </Link>
                </td>
                <td className="px-3 py-2.5">
                  <button
                    type="button"
                    aria-label={`Ações do dataset ${d.title}`}
                    title={`Ações do dataset ${d.title}`}
                    onClick={(e) => {
                      e.stopPropagation();
                      if (!onContextMenu) return;
                      const rect = e.currentTarget.getBoundingClientRect();
                      onContextMenu(d, rect.left, rect.bottom);
                    }}
                    className="inline-flex size-9 items-center justify-center rounded-lg border border-transparent bg-transparent p-0 text-zinc-400 transition hover:bg-white/[0.06] hover:text-white active:scale-[0.985] focus-visible:ring-2 focus-visible:ring-brand-500/70 focus-visible:ring-offset-2 focus-visible:ring-offset-transparent [&_svg]:size-4 disabled:pointer-events-none disabled:opacity-55"
                  >
                    <IconMoreVertical className="h-4 w-4" />
                  </button>
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
