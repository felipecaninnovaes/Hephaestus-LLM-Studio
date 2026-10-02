import {
	calculateEMA,
	pivotMetricPoints,
	type XAxisMode,
} from "@/lib/metricMath";
import type { Job, MetricPointWithKey } from "@/types/jobs";

export const COMPARE_MIN_JOBS = 2;
export const COMPARE_MAX_JOBS = 4;

const UUID_REGEX =
	/^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;

/** Verifica se a string é um UUID válido (RFC 4122). */
export function isValidJobUuid(id: string): boolean {
	return UUID_REGEX.test(id.trim());
}

export type CompareIdsValidationResult =
	| { valid: true; jobIds: string[] }
	| {
			valid: false;
			reason:
				| "too_few"
				| "too_many"
				| "invalid_uuid"
				| "duplicate_ids";
			message: string;
	  };

/**
 * Valida o parâmetro `?ids=` da página de comparação de runs (fatia 4c).
 * Regras:
 * - Aceita 2 a 4 UUIDs válidos e únicos.
 * - Rejeita menos de 2 (<2 ou 1).
 * - Rejeita mais de 4 (5+).
 * - Rejeita tokens que não sejam UUIDs válidos.
 * - Rejeita IDs duplicados.
 */
export function parseCompareJobIds(
	idsParam: string | null | undefined,
): CompareIdsValidationResult {
	if (!idsParam) {
		return {
			valid: false,
			reason: "too_few",
			message: `Selecione pelo menos ${COMPARE_MIN_JOBS} jobs para comparar.`,
		};
	}

	const tokens = idsParam
		.split(",")
		.map((s) => s.trim())
		.filter(Boolean);

	// Checagem de UUIDs inválidos
	const invalidToken = tokens.find((t) => !isValidJobUuid(t));
	if (invalidToken !== undefined) {
		return {
			valid: false,
			reason: "invalid_uuid",
			message: `ID inválido: "${invalidToken}" não é um UUID válido.`,
		};
	}

	// Checagem de duplicatas
	const unique = new Set(tokens);
	if (unique.size !== tokens.length) {
		return {
			valid: false,
			reason: "duplicate_ids",
			message: "IDs duplicados encontrados na seleção de comparação.",
		};
	}

	if (tokens.length < COMPARE_MIN_JOBS) {
		return {
			valid: false,
			reason: "too_few",
			message: `Selecione pelo menos ${COMPARE_MIN_JOBS} jobs para comparar.`,
		};
	}

	if (tokens.length > COMPARE_MAX_JOBS) {
		return {
			valid: false,
			reason: "too_many",
			message: `Máximo de ${COMPARE_MAX_JOBS} jobs permitidos para comparação.`,
		};
	}

	return { valid: true, jobIds: tokens };
}
export interface ComparisonSeriesResult {
	/** Grade de X compartilhada (união ordenada de todos os runs). */
	xValues: number[];
	/** Valor (já suavizado por EMA se aplicável) por job, alinhado a `xValues` — `null` = sem amostra naquele ponto (uPlot conecta via `spanGaps`). */
	seriesByJob: Record<string, (number | null)[]>;
	/** Chaves de métrica presentes em pelo menos um dos jobs (para o seletor de métrica). */
	availableKeys: string[];
}

/**
 * Funde as séries de métricas de múltiplos jobs (fatia 4c) numa única matriz
 * uPlot-compatível: cada run mantém sua própria trajetória de step/época
 * (pivotada individualmente por `pivotMetricPoints`), projetada sobre uma
 * grade X compartilhada (união ordenada dos pontos de todos os runs).
 * EMA é aplicada por run ANTES da projeção, com o mesmo `smoothing`
 * (compartilhado) para todos — "EMA compartilhado" do §3.4 da spec.
 */
export function buildComparisonSeries(
	pointsByJob: Record<string, MetricPointWithKey[]>,
	jobKindByJob: Record<string, string | undefined>,
	metricKey: string,
	xMode: XAxisMode,
	emaSmoothing: number,
): ComparisonSeriesResult {
	const jobIds = Object.keys(pointsByJob);
	const perJob: Record<
		string,
		{ xValues: number[]; values: (number | null)[] }
	> = {};
	const keySet = new Set<string>();

	for (const jobId of jobIds) {
		const pivoted = pivotMetricPoints(pointsByJob[jobId] ?? [], xMode, {
			jobKind: jobKindByJob[jobId],
		});
		for (const k of pivoted.seriesKeys) keySet.add(k);
		const raw = pivoted.seriesData[metricKey] ?? [];
		const smoothed = emaSmoothing > 0 ? calculateEMA(raw, emaSmoothing) : raw;
		perJob[jobId] = { xValues: pivoted.xValues, values: smoothed };
	}

	const xSet = new Set<number>();
	for (const jobId of jobIds) {
		for (const x of perJob[jobId].xValues) xSet.add(x);
	}
	const xValues = Array.from(xSet).sort((a, b) => a - b);

	const seriesByJob: Record<string, (number | null)[]> = {};
	for (const jobId of jobIds) {
		const { xValues: jx, values: jv } = perJob[jobId];
		const map = new Map<number, number>();
		for (let i = 0; i < jx.length; i++) {
			const v = jv[i];
			if (v != null) map.set(jx[i], v);
		}
		seriesByJob[jobId] = xValues.map((x) => map.get(x) ?? null);
	}

	return {
		xValues,
		seriesByJob,
		availableKeys: Array.from(keySet).sort(),
	};
}

/** Achata `job.params` em pares `chave.aninhada -> valor formatado`, sem perder nenhum campo. */
export function flattenParams(
	params: Record<string, unknown> | null | undefined,
	prefix = "",
): Record<string, string> {
	const out: Record<string, string> = {};
	if (!params) return out;
	for (const [k, v] of Object.entries(params)) {
		const key = prefix ? `${prefix}.${k}` : k;
		if (v !== null && typeof v === "object" && !Array.isArray(v)) {
			Object.assign(out, flattenParams(v as Record<string, unknown>, key));
		} else if (Array.isArray(v)) {
			out[key] = JSON.stringify(v);
		} else if (v == null) {
			out[key] = "—";
		} else {
			out[key] = String(v);
		}
	}
	return out;
}

export interface HyperparamRow {
	key: string;
	valuesByJob: Record<string, string>;
	differs: boolean;
}

/**
 * Tabela lado a lado dos hiperparâmetros (`job.params` achatado) de cada job,
 * marcando linhas cujo valor diverge entre os runs selecionados (fatia 4c).
 */
export function buildHyperparamRows(jobs: readonly Job[]): HyperparamRow[] {
	const flatByJob: Record<string, Record<string, string>> = {};
	const allKeys = new Set<string>();
	for (const job of jobs) {
		const flat = flattenParams(
			job.params as Record<string, unknown> | null | undefined,
		);
		flatByJob[job.id] = flat;
		for (const k of Object.keys(flat)) allKeys.add(k);
	}

	const rows: HyperparamRow[] = [];
	for (const key of Array.from(allKeys).sort()) {
		const valuesByJob: Record<string, string> = {};
		for (const job of jobs) {
			valuesByJob[job.id] = flatByJob[job.id][key] ?? "—";
		}
		const uniqueValues = new Set(Object.values(valuesByJob));
		rows.push({ key, valuesByJob, differs: uniqueValues.size > 1 });
	}
	return rows;
}
