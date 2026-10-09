"use client";

import { Button } from "@/components/ui/Button";
import { showToast } from "@/components/ui/Toast";
import { copyToClipboard } from "@/lib/clipboard";

const ENV_LINE = "HEPHAESTUS_COMFY_TOKEN=<token-secreto>";

/** Passos curtos para instalar o custom node no ComfyUI remoto. */
export function ComfyInstallHint() {
	async function copyEnv() {
		const ok = await copyToClipboard(ENV_LINE);
		showToast(
			ok ? "Copiado." : "Não foi possível copiar.",
			ok ? "success" : "error",
		);
	}

	return (
		<div className="rounded-xl border border-white/10 bg-white/[0.02] p-4">
			<h3 className="font-mono text-2xs font-semibold uppercase tracking-caps text-zinc-300">
				Instalar o custom node no ComfyUI
			</h3>
			<ol className="mt-3 space-y-2 text-xs leading-relaxed text-zinc-300">
				<Step n={1}>
					Copie a pasta{" "}
					<code className="rounded bg-black/40 px-1 font-mono text-2xs text-zinc-100">
						integrations/comfyui-hephaestus
					</code>{" "}
					para{" "}
					<code className="rounded bg-black/40 px-1 font-mono text-2xs text-zinc-100">
						ComfyUI/custom_nodes/
					</code>
					.
				</Step>
				<Step n={2}>
					Defina a variável de ambiente{" "}
					<code className="rounded bg-black/40 px-1 font-mono text-2xs text-zinc-100">
						{ENV_LINE}
					</code>{" "}
					<Button
						type="button"
						variant="ghost"
						size="sm"
						className="ml-1 h-6 px-2 align-middle"
						onClick={() => void copyEnv()}
					>
						Copiar
					</Button>
				</Step>
				<Step n={3}>
					Reinicie o ComfyUI e cadastre abaixo a URL e esse mesmo token.
				</Step>
			</ol>
		</div>
	);
}

function Step({ n, children }: { n: number; children: React.ReactNode }) {
	return (
		<li className="flex gap-2.5">
			<span className="mt-0.5 flex size-5 shrink-0 items-center justify-center rounded-full border border-brand-500/30 bg-brand-500/10 font-mono text-3xs text-brand-400">
				{n}
			</span>
			<span className="min-w-0">{children}</span>
		</li>
	);
}
