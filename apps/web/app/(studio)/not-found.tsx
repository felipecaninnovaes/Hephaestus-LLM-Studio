import Link from "next/link";
import { IconTarget } from "@/components/icons";
import { Button } from "@/components/ui/Button";

export default function StudioNotFound() {
  return (
    <div className="flex h-full min-h-[60vh] flex-col items-center justify-center p-6 text-center">
      <div className="glass-card max-w-md rounded-2xl border border-white/10 p-8 shadow-2xl backdrop-blur-md space-y-4">
        <div className="mx-auto flex size-12 items-center justify-center rounded-xl border border-brand-500/30 bg-brand-500/15 text-brand-400">
          <IconTarget className="size-6" />
        </div>

        <div className="space-y-1.5">
          <h2 className="font-display text-lg font-bold text-white">
            Página Não Encontrada
          </h2>
          <p className="text-xs text-zinc-400 leading-relaxed">
            O recurso ou módulo solicitado não existe ou foi removido do
            Hephaestus Studio.
          </p>
        </div>

        <div className="pt-2">
          <Link href="/dashboard">
            <Button type="button" variant="primary" size="md">
              Voltar ao Painel
            </Button>
          </Link>
        </div>
      </div>
    </div>
  );
}
