import { Spinner } from "@/components/ui/Spinner";

export default function StudioLoading() {
  return (
    <div className="mx-auto flex max-w-7xl flex-col gap-6 p-4 sm:p-6 lg:p-8 animate-pulse">
      {/* Header skeleton */}
      <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between border-b border-white/5 pb-5">
        <div className="flex items-center space-x-3">
          <div className="size-10 rounded-xl bg-brand-500/10 border border-brand-500/20" />
          <div className="space-y-1.5">
            <div className="h-5 w-48 rounded bg-white/10" />
            <div className="h-3 w-72 rounded bg-white/5" />
          </div>
        </div>
        <div className="flex items-center gap-2">
          <div className="h-8 w-28 rounded-lg bg-white/5 border border-white/5" />
          <div className="h-8 w-28 rounded-lg bg-brand-500/20 border border-brand-500/30" />
        </div>
      </div>

      {/* Grid skeleton */}
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
        {[1, 2, 3, 4, 5, 6, 7, 8].map((i) => (
          <div
            key={i}
            className="glass-card rounded-2xl p-4 border border-white/10 space-y-3"
          >
            <div className="h-32 w-full rounded-xl bg-white/[0.03] border border-white/5 flex items-center justify-center">
              <Spinner className="size-5 text-zinc-600" />
            </div>
            <div className="h-4 w-3/4 rounded bg-white/10" />
            <div className="h-3 w-1/2 rounded bg-white/5" />
            <div className="pt-2 border-t border-white/5 flex justify-between">
              <div className="h-3 w-16 rounded bg-white/5" />
              <div className="h-3 w-12 rounded bg-white/5" />
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
