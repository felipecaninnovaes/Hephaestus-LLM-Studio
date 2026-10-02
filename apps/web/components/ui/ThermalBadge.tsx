import { forwardRef, type HTMLAttributes } from "react";

export interface ThermalBadgeProps extends HTMLAttributes<HTMLSpanElement> {
	temperatureC?: number | null;
}

/**
 * Escala térmica canônica do Hephaestus LLM Studio:
 * - < 70 °C: neutro / brand suave (normal)
 * - 70 a 80 °C: alerta intermediário (âmbar / status-alert #f59e0b)
 * - > 80 °C: status-alert pulsante / crítico (status-alert / border-status-alert)
 * - null/undefined: valor ausente ("—", sem escala de alerta)
 */
export const ThermalBadge = forwardRef<HTMLSpanElement, ThermalBadgeProps>(
	({ temperatureC, className = "", ...props }, ref) => {
		if (temperatureC == null || Number.isNaN(temperatureC)) {
			return (
				<span
					ref={ref}
					role="status"
					className={`inline-flex items-center gap-1 rounded-sm border border-white/10 bg-zinc-900/60 px-1.5 py-0.5 font-mono text-3xs font-medium text-zinc-400 ${className}`.trim()}
					aria-label="Temperatura indisponível"
					{...props}
				>
					<span className="text-zinc-500">TEMP</span>
					<span className="text-zinc-300 tabular-nums">—</span>
				</span>
			);
		}

		const temp = Math.round(temperatureC);
		let colorClasses = "border-brand-500/20 bg-brand-500/10 text-brand-300";
		let ariaStatus = "normal";

		if (temp > 80) {
			colorClasses =
				"border-status-alert/50 bg-status-alert/20 text-status-alert shadow-[0_0_8px_rgba(245,158,11,0.25)]";
			ariaStatus = "crítica";
		} else if (temp >= 70) {
			colorClasses =
				"border-status-alert/40 bg-status-alert/10 text-status-alert";
			ariaStatus = "alta";
		}

		return (
			<span
				ref={ref}
				role="status"
				className={`inline-flex items-center gap-1 rounded-sm border px-1.5 py-0.5 font-mono text-3xs font-medium backdrop-blur-sm ${colorClasses} ${className}`.trim()}
				aria-label={`Temperatura ${temp} graus Celsius, faixa ${ariaStatus}`}
				{...props}
			>
				<span className="text-2xs opacity-75">TEMP</span>
				<span className="tabular-nums font-semibold">{`${temp}°C`}</span>
			</span>
		);
	},
);

ThermalBadge.displayName = "ThermalBadge";
export default ThermalBadge;
