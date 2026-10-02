import type { HTMLAttributes } from "react";
import { ThermalBadge } from "@/components/ui/ThermalBadge";
import type { components } from "@/types/api-generated";

export type GpuDeviceTelemetry = components["schemas"]["GpuDeviceTelemetry"];

export interface MultiGpuRackProps extends HTMLAttributes<HTMLDivElement> {
	devices?: GpuDeviceTelemetry[] | null;
	/** Se true, renderiza layout mais compacto para painéis laterais / toolbars */
	compact?: boolean;
}

export function MultiGpuRack({
	devices,
	compact: _compact = false,
	className = "",
	...props
}: MultiGpuRackProps) {
	if (!devices || devices.length === 0) {
		return null;
	}

	const rackTitle = `RACK MULTI-GPU (${devices.length} ${devices.length === 1 ? "PLACA" : "PLACAS"})`;

	return (
		<section
			className={`flex flex-col gap-2.5 ${className}`.trim()}
			aria-label={`Sensores de ${devices.length} GPUs físicas`}
			{...props}
		>
			<div className="flex items-center justify-between text-3xs font-mono text-zinc-400">
				<span className="uppercase tracking-caps font-semibold text-zinc-300">
					{rackTitle}
				</span>
			</div>

			<div className="flex flex-col gap-2">
				{devices.map((dev) => {
					const vramUsedMib = dev.vramUsed ?? 0;
					const vramTotalMib = dev.vramTotal ?? 0;
					const vramUsedGb = (vramUsedMib / 1024).toFixed(1);
					const vramTotalGb = (vramTotalMib / 1024).toFixed(1);
					const vramPct =
						vramTotalMib > 0
							? Math.min(100, Math.max(0, (vramUsedMib / vramTotalMib) * 100))
							: 0;

					const utilStr =
						dev.gpuUtilizationPct != null &&
						!Number.isNaN(dev.gpuUtilizationPct)
							? `${dev.gpuUtilizationPct.toFixed(0)}%`
							: "—";

					const powerStr =
						dev.powerWatts != null && !Number.isNaN(dev.powerWatts)
							? `${dev.powerWatts.toFixed(0)}W`
							: "—";

					return (
						<div
							key={dev.uuid || `gpu-${dev.index}`}
							className="rounded-xl border border-white/10 bg-zinc-950/70 p-3 backdrop-blur-sm transition-colors hover:border-brand-500/25"
						>
							{/* Top row: Index, Model name & UUID */}
							<div className="flex flex-wrap items-center justify-between gap-1.5 pb-2">
								<div className="flex min-w-0 items-center gap-2">
									<span className="inline-flex items-center justify-center rounded-sm bg-brand-500/20 px-1.5 py-0.5 font-mono text-3xs font-bold text-brand-300 border border-brand-500/30">
										{`GPU ${dev.index}`}
									</span>
									<span
										className="truncate font-sans text-xs font-semibold text-zinc-100"
										title={dev.name}
									>
										{dev.name}
									</span>
								</div>

								<div className="flex items-center gap-1.5 font-mono text-3xs">
									<span
										className="hidden sm:inline-block text-zinc-500 max-w-[120px] truncate"
										title={dev.uuid}
									>
										{dev.uuid}
									</span>
									<ThermalBadge temperatureC={dev.temperatureC} />
								</div>
							</div>

							{/* VRAM Progress bar */}
							<div className="space-y-1">
								<div className="flex items-center justify-between font-mono text-3xs text-zinc-400">
									<span className="text-zinc-500">VRAM</span>
									<span className="tabular-nums text-zinc-200">
										<span className="font-semibold text-white">
											{vramUsedGb}
										</span>
										<span className="text-zinc-500"> / </span>
										<span>{`${vramTotalGb} GB`}</span>
										<span className="text-zinc-500 ml-1.5">{`(${vramPct.toFixed(0)}%)`}</span>
									</span>
								</div>

								<div
									role="progressbar"
									aria-valuenow={Math.round(vramPct)}
									aria-valuemin={0}
									aria-valuemax={100}
									aria-label={`Uso de VRAM da GPU ${dev.index}: ${vramUsedGb} de ${vramTotalGb} GB`}
									className="h-1.5 w-full overflow-hidden rounded-full bg-zinc-900 border border-white/5"
								>
									<div
										className="h-full rounded-full bg-brand-500 transition-all duration-300"
										style={{ width: `${vramPct}%` }}
									/>
								</div>
							</div>

							{/* Sensor stats: Util % and Power W */}
							<div className="mt-2.5 flex items-center justify-between border-t border-white/5 pt-2 font-mono text-3xs text-zinc-400">
								<div className="flex items-center gap-1">
									<span className="text-zinc-500">UTIL:</span>
									<span
										title={`Utilização: ${utilStr}`}
										className="tabular-nums font-medium text-zinc-200"
									>
										{utilStr}
									</span>
								</div>

								<div className="flex items-center gap-1">
									<span className="text-zinc-500">POWER:</span>
									<span
										title={`Consumo de energia: ${powerStr}`}
										className="tabular-nums font-medium text-zinc-200"
									>
										{powerStr}
									</span>
								</div>
							</div>
						</div>
					);
				})}
			</div>
		</section>
	);
}

export default MultiGpuRack;
