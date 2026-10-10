import { describe, expect, it } from "bun:test";
import type { Job, JobArtifact } from "@/types/studio";
import {
	buildDiffusionRerun,
	buildDiffusionResume,
	diffusionEpochOffset,
	paramsToPreset,
} from "./paramsToPreset";

function makeJob(partial: Partial<Job> = {}): Job {
	return {
		id: "job-123",
		engine: "diffusion",
		status: "done",
		createdAt: "2026-10-04T12:00:00Z",
		datasetId: "ds-alpha",
		model: "flux",
		epoch: 6,
		params: {
			baseModel: "flux",
			triggerWord: "OHWX",
			epochs: 10,
			learningRate: 0.00005,
			lrScheduler: "cosine",
			lrWarmupSteps: 400,
			rank: 16,
			alpha: 16,
			weights: "weights-original-id",
			outputName: "lora-v1",
		},
		...partial,
	};
}

function makeArtifact(partial: Partial<JobArtifact> = {}): JobArtifact {
	return {
		id: "art-checkpoint-6",
		jobId: "job-123",
		kind: "checkpoint",
		path: "runs/job-123/weights/epoch_6/checkpoint.safetensors",
		size: 1024 * 1024 * 50,
		createdAt: "2026-10-04T12:30:00Z",
		...partial,
	};
}

describe("paramsToPreset", () => {
	it("converts job params to studio preset correctly", () => {
		const job = makeJob();
		const preset = paramsToPreset(job);
		expect(preset).toBeDefined();
		expect(preset?.epochs).toBe(10);
		expect(preset?.learningRate).toBe("0.00005");
		expect(preset?.weights).toBe("weights-original-id");
		expect(preset?.outputName).toBe("lora-v1");
	});
});

describe("diffusionEpochOffset", () => {
	it("extracts epoch from artifact path", () => {
		const job = makeJob();
		const art = makeArtifact({
			path: "checkpoints/epoch_6/adapter.safetensors",
		});
		expect(diffusionEpochOffset(job, art)).toBe(6);
	});

	it("falls back to job.epoch if artifact path has no epoch_N", () => {
		const job = makeJob({ epoch: 8 });
		const art = makeArtifact({ path: "checkpoints/final.safetensors" });
		expect(diffusionEpochOffset(job, art)).toBe(8);
	});
});

describe("buildDiffusionResume", () => {
	it("calculates remaining epochs when epochOffset < originalEpochs", () => {
		const job = makeJob(); // 10 epochs
		const art = makeArtifact({
			path: "runs/weights/epoch_6/checkpoint.safetensors",
		});
		const payload = buildDiffusionResume(job, art);

		expect(payload.epochOffset).toBe(6);
		expect(payload.datasetId).toBe("ds-alpha");
		expect(payload.resumeCheckpoint).toEqual({
			id: "art-checkpoint-6",
			name: "checkpoint.safetensors",
			epoch: 6,
			sourceJobId: "job-123",
		});
		// Invariant: epochs = max(1, originalEpochs - epochOffset) -> 10 - 6 = 4
		expect(payload.initialPreset?.epochs).toBe(4);
		// Cleared so source weights/name are not overwritten
		expect(payload.initialPreset?.weights).toBeUndefined();
		expect(payload.initialPreset?.outputName).toBeUndefined();
		// Preserves original peak LR and schedule
		expect(payload.initialPreset?.learningRate).toBe("0.00005");
		expect(payload.initialPreset?.lrScheduler).toBe("cosine");
		expect(payload.initialPreset?.lrWarmupSteps).toBe(400);
	});

	it("clamps epochs to 1 when epochOffset >= originalEpochs", () => {
		const job = makeJob({
			params: {
				...(makeJob().params as Record<string, unknown>),
				epochs: 10,
			},
		});
		// Exactly equal: 10 - 10 = 0 -> clamp to 1
		const artEqual = makeArtifact({
			path: "runs/weights/epoch_10/checkpoint.safetensors",
		});
		const payloadEqual = buildDiffusionResume(job, artEqual);
		expect(payloadEqual.epochOffset).toBe(10);
		expect(payloadEqual.initialPreset?.epochs).toBe(1);

		// Greater than: 10 - 12 = -2 -> clamp to 1
		const artGreater = makeArtifact({
			path: "runs/weights/epoch_12/checkpoint.safetensors",
		});
		const payloadGreater = buildDiffusionResume(job, artGreater);
		expect(payloadGreater.epochOffset).toBe(12);
		expect(payloadGreater.initialPreset?.epochs).toBe(1);
	});

	it("handles chained resume correctly across intermediate checkpoints", () => {
		// Original 10 epochs -> resumed at 6 with epochs 4 (epochOffset: 6, epochs: 4)
		// That job reaches epoch_8 checkpoint and is resumed again.
		// totalEpochs = priorOffset (6) + epochs (4) = 10
		// remaining = max(1, 10 - 8) = 2 epochs.
		const chainedJobCamel = makeJob({
			params: {
				...(makeJob().params as Record<string, unknown>),
				epochOffset: 6,
				epochs: 4,
			},
		});
		const art8 = makeArtifact({
			path: "runs/weights/epoch_008/checkpoint.safetensors",
		});
		const payloadCamel = buildDiffusionResume(chainedJobCamel, art8);
		expect(payloadCamel.epochOffset).toBe(8);
		expect(payloadCamel.initialPreset?.epochs).toBe(2);

		// Also supports legacy snake_case epoch_offset
		const chainedJobSnake = makeJob({
			params: {
				...(makeJob().params as Record<string, unknown>),
				epoch_offset: 6,
				epochs: 4,
			},
		});
		const payloadSnake = buildDiffusionResume(chainedJobSnake, art8);
		expect(payloadSnake.epochOffset).toBe(8);
		expect(payloadSnake.initialPreset?.epochs).toBe(2);
	});

	it("handles job without params gracefully", () => {
		const job = makeJob({ params: undefined });
		const art = makeArtifact({ path: "runs/epoch_3/model.safetensors" });
		const payload = buildDiffusionResume(job, art);
		expect(payload.epochOffset).toBe(3);
		expect(payload.initialPreset).toBeUndefined();
	});
});

describe("buildDiffusionRerun", () => {
	it("keeps original epochs and resets epochOffset to 0", () => {
		const job = makeJob();
		const rerun = buildDiffusionRerun(job);
		expect(rerun.epochOffset).toBe(0);
		expect(rerun.initialPreset?.epochs).toBe(10);
		expect(rerun.resumeCheckpoint).toBeUndefined();
	});
});
