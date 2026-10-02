import { describe, expect, it } from "bun:test";
import {
	buildComparisonSeries,
	buildHyperparamRows,
	flattenParams,
	isValidJobUuid,
	parseCompareJobIds,
} from "./compareJobs";
import type { Job, MetricPointWithKey } from "@/types/jobs";

function makePoint(
	step: number,
	loss: number,
	overrides: Partial<MetricPointWithKey> = {},
): MetricPointWithKey {
	return {
		seq: step,
		key: "loss",
		step,
		value: loss,
		epoch: 1,
		ts: "2026-10-02T10:00:00Z",
		...overrides,
	};
}

function makeJob(id: string, params: Record<string, unknown> = {}): Job {
	return {
		id,
		datasetId: "ds-1",
		model: "flux-dev",
		kind: "flux_lora",
		status: "completed",
		progress: 100,
		currentStep: 100,
		totalSteps: 100,
		currentEpoch: 1,
		totalEpochs: 1,
		params,
		metrics: null,
		telemetry: null,
		createdAt: "2026-10-02T10:00:00Z",
		updatedAt: "2026-10-02T11:00:00Z",
		startedAt: "2026-10-02T10:00:05Z",
		endedAt: "2026-10-02T11:00:00Z",
		exitCode: 0,
		error: null,
		engineState: null,
	};
}

describe("compareJobs", () => {
	describe("validação de ?ids (parseCompareJobIds / isValidJobUuid)", () => {
		const uuid1 = "11111111-1111-4111-8111-111111111111";
		const uuid2 = "22222222-2222-4222-8222-222222222222";
		const uuid3 = "33333333-3333-4333-8333-333333333333";
		const uuid4 = "44444444-4444-4444-8444-444444444444";
		const uuid5 = "55555555-5555-4555-8555-555555555555";

		it("valida UUID v4/v1 corretamente", () => {
			expect(isValidJobUuid(uuid1)).toBe(true);
			expect(isValidJobUuid("not-a-uuid")).toBe(false);
			expect(isValidJobUuid("")).toBe(false);
			expect(isValidJobUuid("12345")).toBe(false);
		});

		it("aceita 2, 3 e 4 UUIDs válidos e únicos", () => {
			const res2 = parseCompareJobIds(`${uuid1},${uuid2}`);
			expect(res2.valid).toBe(true);
			if (res2.valid) {
				expect(res2.jobIds).toEqual([uuid1, uuid2]);
			}

			const res3 = parseCompareJobIds(`${uuid1},${uuid2},${uuid3}`);
			expect(res3.valid).toBe(true);
			if (res3.valid) {
				expect(res3.jobIds).toEqual([uuid1, uuid2, uuid3]);
			}

			const res4 = parseCompareJobIds(`${uuid1},${uuid2},${uuid3},${uuid4}`);
			expect(res4.valid).toBe(true);
			if (res4.valid) {
				expect(res4.jobIds).toEqual([uuid1, uuid2, uuid3, uuid4]);
			}
		});

		it("rejeita 0 ou 1 UUID (too_few)", () => {
			const res0 = parseCompareJobIds("");
			expect(res0.valid).toBe(false);
			if (!res0.valid) expect(res0.reason).toBe("too_few");

			const res1 = parseCompareJobIds(uuid1);
			expect(res1.valid).toBe(false);
			if (!res1.valid) expect(res1.reason).toBe("too_few");
		});

		it("rejeita 5 ou mais UUIDs (too_many)", () => {
			const res5 = parseCompareJobIds(
				`${uuid1},${uuid2},${uuid3},${uuid4},${uuid5}`,
			);
			expect(res5.valid).toBe(false);
			if (!res5.valid) expect(res5.reason).toBe("too_many");
		});

		it("rejeita tokens que não são UUIDs válidos (invalid_uuid)", () => {
			const res = parseCompareJobIds(`${uuid1},job-invalido-123`);
			expect(res.valid).toBe(false);
			if (!res.valid) {
				expect(res.reason).toBe("invalid_uuid");
				expect(res.message).toContain("job-invalido-123");
			}
		});

		it("rejeita UUIDs duplicados (duplicate_ids)", () => {
			const res = parseCompareJobIds(`${uuid1},${uuid1}`);
			expect(res.valid).toBe(false);
			if (!res.valid) expect(res.reason).toBe("duplicate_ids");
		});
	});

	describe("buildComparisonSeries: grade X compartilhada e gaps", () => {
		it("cria grade X compartilhada quando os runs têm steps diferentes e preenche gaps com null (nunca interpolação falsa)", () => {
			// Job A tem steps [10, 20, 40]
			// Job B tem steps [15, 20, 30]
			// Grade compartilhada esperada: [10, 15, 20, 30, 40]
			const pointsByJob: Record<string, MetricPointWithKey[]> = {
				jobA: [
					makePoint(10, 0.5),
					makePoint(20, 0.4),
					makePoint(40, 0.2),
				],
				jobB: [
					makePoint(15, 0.8),
					makePoint(20, 0.6),
					makePoint(30, 0.3),
				],
			};

			const result = buildComparisonSeries(
				pointsByJob,
				{ jobA: "flux_lora", jobB: "flux_lora" },
				"loss",
				"global_step",
				0, // sem smoothing
			);

			expect(result.xValues).toEqual([10, 15, 20, 30, 40]);

			// Job A tem valor em 10, 20, 40; em 15 e 30 deve ser estritamente null (gap)
			expect(result.seriesByJob.jobA).toEqual([0.5, null, 0.4, null, 0.2]);

			// Job B tem valor em 15, 20, 30; em 10 e 40 deve ser estritamente null (gap)
			expect(result.seriesByJob.jobB).toEqual([null, 0.8, 0.6, 0.3, null]);
		});

		it("aplica EMA por run com o mesmo smoothing antes de projetar na grade X", () => {
			const pointsByJob: Record<string, MetricPointWithKey[]> = {
				jobA: [
					makePoint(10, 1.0),
					makePoint(20, 0.5),
					makePoint(30, 0.2),
				],
				jobB: [
					makePoint(10, 2.0),
					makePoint(20, 1.0),
					makePoint(30, 0.4),
				],
			};

			const smoothing = 0.5;
			const result = buildComparisonSeries(
				pointsByJob,
				{ jobA: "flux_lora", jobB: "flux_lora" },
				"loss",
				"global_step",
				smoothing,
			);

			// Ambos os runs são suavizados individualmente com smoothing 0.5:
			// EMA_0 = val_0
			// EMA_t = smoothing * EMA_{t-1} + (1 - smoothing) * val_t
			// Job A:
			// s0 = 1.0
			// s1 = 0.5 * 1.0 + 0.5 * 0.5 = 0.75
			// s2 = 0.5 * 0.75 + 0.5 * 0.2 = 0.475
			const aVals = result.seriesByJob.jobA;
			expect(aVals[0]).toBeCloseTo(1.0);
			expect(aVals[1]).toBeCloseTo(0.75);
			expect(aVals[2]).toBeCloseTo(0.475);

			// Job B:
			// s0 = 2.0
			// s1 = 0.5 * 2.0 + 0.5 * 1.0 = 1.5
			// s2 = 0.5 * 1.5 + 0.5 * 0.4 = 0.95
			const bVals = result.seriesByJob.jobB;
			expect(bVals[0]).toBeCloseTo(2.0);
			expect(bVals[1]).toBeCloseTo(1.5);
			expect(bVals[2]).toBeCloseTo(0.95);
		});
	});

	describe("achatamento recursivo de params e marcação de divergências", () => {
		it("flattenParams achata objetos recursivamente sem perder campos", () => {
			const params = {
				lr: 0.0001,
				optimizer: {
					type: "adamw8bit",
					betas: [0.9, 0.999],
					nested: {
						eps: 1e-8,
					},
				},
				seed: null,
			};

			const flat = flattenParams(params);
			expect(flat["lr"]).toBe("0.0001");
			expect(flat["optimizer.type"]).toBe("adamw8bit");
			expect(flat["optimizer.betas"]).toBe("[0.9,0.999]");
			expect(flat["optimizer.nested.eps"]).toBe("1e-8");
			expect(flat["seed"]).toBe("—");
		});

		it("buildHyperparamRows marca como differs: true as linhas divergentes e false as idênticas", () => {
			const job1 = makeJob("job-1", {
				lr: 0.0001,
				batch_size: 4,
				optimizer: { type: "adamw" },
			});
			const job2 = makeJob("job-2", {
				lr: 0.0002, // diferente
				batch_size: 4, // igual
				optimizer: { type: "adamw" }, // igual
			});

			const rows = buildHyperparamRows([job1, job2]);

			const lrRow = rows.find((r) => r.key === "lr");
			expect(lrRow).toBeDefined();
			expect(lrRow?.differs).toBe(true);
			expect(lrRow?.valuesByJob["job-1"]).toBe("0.0001");
			expect(lrRow?.valuesByJob["job-2"]).toBe("0.0002");

			const bsRow = rows.find((r) => r.key === "batch_size");
			expect(bsRow).toBeDefined();
			expect(bsRow?.differs).toBe(false);

			const optRow = rows.find((r) => r.key === "optimizer.type");
			expect(optRow).toBeDefined();
			expect(optRow?.differs).toBe(false);
		});

		it("marca como divergente quando uma chave existe em um job mas falta em outro", () => {
			const job1 = makeJob("job-1", { lora_rank: 16 });
			const job2 = makeJob("job-2", {}); // sem lora_rank

			const rows = buildHyperparamRows([job1, job2]);
			const rankRow = rows.find((r) => r.key === "lora_rank");
			expect(rankRow).toBeDefined();
			expect(rankRow?.differs).toBe(true);
			expect(rankRow?.valuesByJob["job-1"]).toBe("16");
			expect(rankRow?.valuesByJob["job-2"]).toBe("—");
		});
	});
});
