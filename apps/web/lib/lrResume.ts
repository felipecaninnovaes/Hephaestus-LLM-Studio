import type { MetricPointWithKey } from "@/types/studio";

/** Como o LR se comporta ao retomar um treino de difusão de um checkpoint. */
export type LrResumeMode = "continue" | "restart";

export const DEFAULT_LR_RESUME_MODE: LrResumeMode = "continue";

type LrScheduler = "cosine" | "linear" | "constant" | "constant_with_warmup";

/**
 * LR em que o checkpoint parou: o último ponto `lr` (maior step, desempate por
 * seq) emitido na época do checkpoint. `epoch` das métricas é absoluto — o
 * checkpoint `epoch_N` fecha a época N. Sem ponto válido ⇒ null.
 */
export function lastLrAtEpoch(
	points: readonly MetricPointWithKey[] | null | undefined,
	epoch: number,
): number | null {
	let best: MetricPointWithKey | null = null;
	for (const p of points ?? []) {
		if (p.key !== "lr" || p.epoch !== epoch) continue;
		if (!Number.isFinite(p.value) || p.value <= 0) continue;
		if (
			!best ||
			p.step > best.step ||
			(p.step === best.step && p.seq > best.seq)
		)
			best = p;
	}
	return best ? best.value : null;
}

/** 1.07e-5 (notação curta, sem zero à esquerda no expoente). */
export function formatLr(lr: number): string {
	return lr.toExponential(2).replace(/\.?0+e/, "e");
}

/** Só retomada (epochOffset > 0) envia o modo; treino novo não envia nada. */
export function lrResumeModeForSubmit(
	epochOffset: number,
	mode: LrResumeMode,
): LrResumeMode | undefined {
	return epochOffset > 0 ? mode : undefined;
}

function epochsLabel(epochs: number): string {
	return `${epochs} ${epochs === 1 ? "época" : "épocas"}`;
}

/** Texto explicativo da curva de LR que a retomada vai executar. */
export function describeLrResume(args: {
	mode: LrResumeMode;
	checkpointLr: number | null;
	requestLr: number | null;
	epochs: number;
	scheduler: LrScheduler;
}): string {
	const { mode, checkpointLr, requestLr, epochs, scheduler } = args;
	const span = epochsLabel(epochs);
	if (mode === "continue" && checkpointLr == null) {
		return `Usa o LR salvo no checkpoint e aplica a curva ${scheduler} sobre ${span}.`;
	}
	const start = mode === "continue" ? checkpointLr : requestLr;
	if (start == null) {
		return `Curva ${scheduler} sobre ${span}, a partir do LR informado.`;
	}
	const lr = formatLr(start);
	const flat = scheduler === "constant" || scheduler === "constant_with_warmup";
	return flat
		? `Começa em ${lr} e se mantém por ${span} (${scheduler}).`
		: `Começa em ${lr} e desce até 0 em ${span} (${scheduler}).`;
}
