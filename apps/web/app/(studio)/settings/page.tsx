"use client";

import { IconSettings } from "@/components/icons";
import { ComfyTargetsSection } from "@/components/studio/settings/ComfyTargetsSection";

export default function SettingsPage() {
	return (
		<div className="mx-auto flex max-w-4xl flex-col gap-6 p-4 sm:p-6 lg:p-8">
			<div className="flex items-center space-x-3 border-b border-white/5 pb-5">
				<div className="flex size-10 items-center justify-center rounded-xl border border-brand-500/30 bg-brand-500/10 backdrop-blur-sm text-brand-400">
					<IconSettings className="size-5" />
				</div>
				<div>
					<h1 className="font-display text-lg font-bold tracking-tight text-white sm:text-xl">
						Configurações
					</h1>
					<p className="font-mono text-2xs text-zinc-400">
						Integrações com serviços externos
					</p>
				</div>
			</div>
			<ComfyTargetsSection />
		</div>
	);
}
