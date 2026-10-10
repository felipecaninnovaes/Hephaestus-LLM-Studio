"use client";

import { SegmentedControl } from "@/components/ui/SegmentedControl";
import { describeLrResume, type LrResumeMode } from "@/lib/lrResume";

export interface DiffusionLrResumeControlProps {
	mode: LrResumeMode;
	onModeChange: (mode: LrResumeMode) => void;
	checkpointLr: number | null;
	requestLr: number | null;
	epochs: number;
	scheduler: "cosine" | "linear" | "constant" | "constant_with_warmup";
	disabled?: boolean;
}

/** Controle "LR na retomada" — só renderizado em modo retomada (epochOffset > 0). */
export function DiffusionLrResumeControl({
	mode,
	onModeChange,
	checkpointLr,
	requestLr,
	epochs,
	scheduler,
	disabled,
}: DiffusionLrResumeControlProps) {
	return (
		<div
			className="space-y-2 rounded-xl border border-sky-500/20 bg-sky-500/[0.04] px-3.5 py-3"
			data-testid="lr-resume-control"
		>
			<span className="tracking-caps block font-mono text-2xs font-medium uppercase text-zinc-300">
				LR na retomada
			</span>
			<SegmentedControl<LrResumeMode>
				ariaLabel="LR na retomada"
				disabled={disabled}
				value={mode}
				onChange={onModeChange}
				options={[
					{ id: "continue", label: "Continuar de onde parou" },
					{ id: "restart", label: "Reiniciar com o LR acima" },
				]}
			/>
			<p
				className="font-mono text-2xs text-zinc-400"
				data-testid="lr-resume-summary"
			>
				{describeLrResume({ mode, checkpointLr, requestLr, epochs, scheduler })}
				{mode === "continue" &&
					" O campo Learning Rate não é usado neste modo."}
			</p>
		</div>
	);
}
