import type { MetricPointWithKey } from "@/types/jobs";

/**
 * Ordena e deduplica pontos por `seq`.
 * Garante idempotência e ordem estrita crescente de sequência.
 */
export function dedupeMetricPoints(
	points: MetricPointWithKey[],
): MetricPointWithKey[] {
	if (points.length <= 1) return points.slice();

	// Ordena por seq crescente
	const sorted = points.slice().sort((a, b) => {
		if (a.seq !== b.seq) return a.seq < b.seq ? -1 : 1;
		return a.ts.localeCompare(b.ts);
	});

	const deduped: MetricPointWithKey[] = [];
	const seenSeqs = new Set<string>();

	for (const pt of sorted) {
		const key = `${pt.seq}:${pt.key}`;
		if (!seenSeqs.has(key)) {
			seenSeqs.add(key);
			deduped.push(pt);
		}
	}

	return deduped;
}

/**
 * Merge de novo lote de pontos com os já existentes, deduplicando por seq + key.
 */
export function mergeMetricPoints(
	existing: MetricPointWithKey[],
	incoming: MetricPointWithKey[],
): MetricPointWithKey[] {
	if (!incoming || incoming.length === 0) return existing;
	if (!existing || existing.length === 0) return dedupeMetricPoints(incoming);

	return dedupeMetricPoints([...existing, ...incoming]);
}

/**
 * Deriva um step global monotônico crescente para jobs onde o step reinicia por época.
 * Mapeia cada par único (epoch, step) para um índice numérico contínuo.
 * Se o step nos pontos já for cumulativo/monotônico contínuo, preserva o próprio step.
 */
export function computeGlobalSteps(
	points: MetricPointWithKey[],
): Map<string, number> {
	const map = new Map<string, number>();
	if (points.length === 0) return map;

	// Coleta pares únicos de (epoch, step) com menor timestamp e menor seq
	const uniqueSteps = new Map<
		string,
		{ epoch: number | null; step: number; minTs: number; firstSeq: number }
	>();

	for (const pt of points) {
		const key = `${pt.epoch ?? "null"}:${pt.step}`;
		const parsedTs = Date.parse(pt.ts) || 0;
		const existing = uniqueSteps.get(key);
		if (!existing) {
			uniqueSteps.set(key, {
				epoch: pt.epoch,
				step: pt.step,
				minTs: parsedTs,
				firstSeq: Number(pt.seq),
			});
		} else {
			if (parsedTs < existing.minTs) existing.minTs = parsedTs;
			if (Number(pt.seq) < existing.firstSeq) {
				existing.firstSeq = Number(pt.seq);
			}
		}
	}

	// Ordena por epoch (nulls first), step, e firstSeq
	const sorted = Array.from(uniqueSteps.values()).sort((a, b) => {
		const epA = a.epoch ?? -1;
		const epB = b.epoch ?? -1;
		if (epA !== epB) return epA - epB;
		if (a.step !== b.step) return a.step - b.step;
		return a.firstSeq - b.firstSeq;
	});

	// Verifica se os steps já são estritamente crescentes sem reiniciar
	let isAlreadyStrictlyIncreasing = true;
	let lastStep = -Infinity;
	for (const item of sorted) {
		if (item.step <= lastStep) {
			isAlreadyStrictlyIncreasing = false;
			break;
		}
		lastStep = item.step;
	}

	sorted.forEach((item, index) => {
		const key = `${item.epoch ?? "null"}:${item.step}`;
		if (isAlreadyStrictlyIncreasing && item.step > 0) {
			map.set(key, item.step);
		} else {
			map.set(key, index + 1);
		}
	});

	return map;
}

/**
 * Calcula Média Móvel Exponencial (EMA) sobre uma série de números (podendo conter nulls).
 * alpha = 1 - smoothing (onde smoothing varia de 0 a 0.99).
 */
export function calculateEMA(
	values: (number | null | undefined)[],
	smoothing: number,
): (number | null)[] {
	if (smoothing <= 0) {
		return values.map((v) => (v != null && Number.isFinite(v) ? v : null));
	}

	const clampedSmoothing = Math.min(Math.max(smoothing, 0), 0.999);
	const alpha = 1 - clampedSmoothing;
	const result: (number | null)[] = new Array(values.length);

	let ema: number | null = null;

	for (let i = 0; i < values.length; i++) {
		const v = values[i];
		if (v == null || !Number.isFinite(v)) {
			result[i] = null;
			continue;
		}

		if (ema === null) {
			ema = v;
		} else {
			ema = alpha * v + (1 - alpha) * ema;
		}
		result[i] = ema;
	}

	return result;
}

export type XAxisMode = "global_step" | "epoch" | "ts";

export interface PivotSeriesResult {
	xValues: number[];
	seriesKeys: string[];
	seriesData: Record<string, (number | null)[]>;
	xIndexMap: { epoch: number | null; step: number; ts: number }[];
}

export interface PivotOptions {
	jobKind?: string;
	allowedKeys?: string[];
}

/**
 * Agrupa pontos brutos de métricas em matriz alinhada para o uPlot.
 * - Filtra estritamente chaves de treino permitidas
 * - Elimina séries cujos valores são todos 0 ou ausentes
 * - uPlot exige que data[0] seja o eixo X em ordem estritamente crescente
 */
export function pivotMetricPoints(
	points: MetricPointWithKey[],
	xMode: XAxisMode = "global_step",
	options: PivotOptions = {},
): PivotSeriesResult {
	const deduped = dedupeMetricPoints(points);
	if (deduped.length === 0) {
		return {
			xValues: [],
			seriesKeys: [],
			seriesData: {},
			xIndexMap: [],
		};
	}

	// 1. Determina chaves permitidas por tipo de job
	const isYolo = options.jobKind?.startsWith("yolo");
	const defaultAllowed = isYolo
		? [
				"box_loss",
				"cls_loss",
				"dfl_loss",
				"mAP50",
				"mAP50-95",
				"map50",
				"map5095",
			]
		: ["loss", "lr", "grad_norm", "gradNorm"];

	const allowedSet = new Set<string>(options.allowedKeys ?? defaultAllowed);

	// Filtra pontos permitidos
	const validPoints = deduped.filter((pt) => allowedSet.has(pt.key));
	if (validPoints.length === 0) {
		return {
			xValues: [],
			seriesKeys: [],
			seriesData: {},
			xIndexMap: [],
		};
	}

	const globalStepMap = computeGlobalSteps(validPoints);

	interface SampleBin {
		xVal: number;
		epoch: number | null;
		step: number;
		ts: number;
		values: Map<string, number>;
	}

	const binsMap = new Map<string, SampleBin>();
	const allKeysSet = new Set<string>();

	for (const pt of validPoints) {
		allKeysSet.add(pt.key);

		const stepKey = `${pt.epoch ?? "null"}:${pt.step}`;
		const tsSec = Math.floor((Date.parse(pt.ts) || 0) / 1000);

		let binKey = stepKey;
		let xVal = 0;

		if (xMode === "epoch") {
			const ep = pt.epoch ?? 0;
			xVal = ep;
			binKey = `${ep}:${pt.step}`;
		} else if (xMode === "ts") {
			xVal = tsSec;
			binKey = `${tsSec}:${pt.step}`;
		} else {
			xVal = globalStepMap.get(stepKey) ?? (pt.epoch ?? 0) * 10000 + pt.step;
			binKey = stepKey;
		}

		let bin = binsMap.get(binKey);
		if (!bin) {
			bin = {
				xVal,
				epoch: pt.epoch,
				step: pt.step,
				ts: tsSec,
				values: new Map(),
			};
			binsMap.set(binKey, bin);
		}
		bin.values.set(pt.key, pt.value);
	}

	// Ordena os bins por xVal crescente
	const sortedBins = Array.from(binsMap.values()).sort((a, b) => {
		if (a.xVal !== b.xVal) return a.xVal - b.xVal;
		const epA = a.epoch ?? 0;
		const epB = b.epoch ?? 0;
		if (epA !== epB) return epA - epB;
		return a.step - b.step;
	});

	// Garante monotonicidade estrita exigida por uPlot
	const xValues: number[] = [];
	const xIndexMap: { epoch: number | null; step: number; ts: number }[] = [];

	let lastX = -Infinity;
	for (let i = 0; i < sortedBins.length; i++) {
		const bin = sortedBins[i];
		let x = bin.xVal;
		if (x <= lastX) {
			x = lastX + (xMode === "ts" ? 1 : 0.001);
		}
		lastX = x;
		xValues.push(x);
		xIndexMap.push({
			epoch: bin.epoch,
			step: bin.step,
			ts: bin.ts,
		});
	}

	// 2. Filtra séries cujos valores são TODOS zero (ex.: métricas YOLO legado em job de difusão)
	const filteredKeys: string[] = [];
	const seriesData: Record<string, (number | null)[]> = {};

	for (const key of Array.from(allKeysSet)) {
		const rawValues = sortedBins.map((bin) => bin.values.get(key) ?? null);
		const hasNonZero = rawValues.some(
			(v) => v !== null && Number.isFinite(v) && v !== 0,
		);
		if (hasNonZero) {
			filteredKeys.push(key);
			seriesData[key] = rawValues;
		}
	}

	return {
		xValues,
		seriesKeys: filteredKeys,
		seriesData,
		xIndexMap,
	};
}
