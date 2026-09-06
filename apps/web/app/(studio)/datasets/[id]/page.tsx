import Link from "next/link";

export default async function DatasetGalleryPage({
  params,
}: {
  params: Promise<{ id: string }>;
}) {
  const { id } = await params;

  return (
    <div className="mx-auto flex max-w-6xl flex-col gap-4 px-4 py-6">
      <Link
        href="/datasets"
        className="w-fit rounded-lg px-2 py-1 text-xs font-medium text-zinc-400 transition-colors hover:bg-zinc-900/60 hover:text-zinc-200"
      >
        ← Datasets
      </Link>
      <div className="flex items-baseline gap-3">
        <h1 className="tracking-display text-base font-semibold text-zinc-100 lg:text-lg">
          Dataset {id}
        </h1>
      </div>
      <div className="glass-card flex flex-col items-center gap-2 rounded-2xl p-12 text-center">
        <p className="text-sm font-medium text-zinc-200">Galeria em construção</p>
        <p className="text-xs text-zinc-500">
          A galeria de imagens chega na fatia 3d (upload, grade de thumbs,
          anotação).
        </p>
      </div>
    </div>
  );
}
