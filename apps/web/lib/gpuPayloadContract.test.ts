import { describe, expect, it } from "bun:test";
import { startYoloJob, startDiffusionJob } from "@/lib/jobs";
import { startDiffusionGenerateJob } from "@/lib/playground";
import { startAutolabelJob } from "@/lib/autolabel";
import { startAutotrackerJob } from "@/lib/autotracker";

describe("Submit Payload gpuDevice contract", () => {
	it("omits gpuDevice completely (undefined) when in Automatic mode", async () => {
		const captured: Record<string, unknown>[] = [];
		const originalFetch = globalThis.fetch;

		// @ts-expect-error mock fetch
		globalThis.fetch = async (_url: string, init?: RequestInit) => {
			if (init?.body && typeof init.body === "string") {
				captured.push(JSON.parse(init.body));
			}
			return new Response(JSON.stringify({ jobId: "test-job-id", status: "queued" }), {
				status: 202,
				headers: { "Content-Type": "application/json" },
			});
		};

		try {
			// 1. YOLO
			await startYoloJob({
				datasetId: "550e8400-e29b-41d4-a716-446655440001",
				model: "yolo11m",
				epochs: 10,
				batch: 16,
				imgsz: 640,
				lr0: 0.01,
				optimizer: "AdamW",
				augment: { mosaic: true, mixupFlip: true },
				orchestratorId: null,
				gpuDevice: undefined,
			});

			// 2. Diffusion train
			await startDiffusionJob({
				datasetId: "550e8400-e29b-41d4-a716-446655440001",
				baseModel: "flux-2-klein-4b",
				orchestratorId: null,
				gpuDevice: undefined,
			});

			// 3. Diffusion generate
			await startDiffusionGenerateJob({
				prompt: "Arcane test prompt",
				baseModel: "flux-2-klein-4b",
				orchestratorId: null,
				gpuDevice: undefined,
			});

			// 4. Autolabel
			await startAutolabelJob({
				datasetId: "550e8400-e29b-41d4-a716-446655440001",
				model: "florence-2",
			});

			// 5. Autotracker
			await startAutotrackerJob({
				datasetId: "550e8400-e29b-41d4-a716-446655440001",
				model: "mock",
			});

			expect(captured).toHaveLength(5);
			for (const body of captured) {
				expect(body).not.toHaveProperty("gpuDevice");
				expect(Object.prototype.hasOwnProperty.call(body, "gpuDevice")).toBe(false);
			}
		} finally {
			globalThis.fetch = originalFetch;
		}
	});

	it("includes gpuDevice string (UUID) when specified with orchestratorId", async () => {
		const captured: Record<string, unknown>[] = [];
		const originalFetch = globalThis.fetch;

		// @ts-expect-error mock fetch
		globalThis.fetch = async (_url: string, init?: RequestInit) => {
			if (init?.body && typeof init.body === "string") {
				captured.push(JSON.parse(init.body));
			}
			return new Response(JSON.stringify({ jobId: "test-job-id", status: "queued" }), {
				status: 202,
				headers: { "Content-Type": "application/json" },
			});
		};

		try {
			const orchId = "3889cfcf-10e0-4ef6-950e-e1c0c01b1999";
			const gpuUuid = "GPU-1c1e01c2-4192-8f38-1a8a-33fb78b06f17";

			await startYoloJob({
				datasetId: "550e8400-e29b-41d4-a716-446655440001",
				model: "yolo11m",
				epochs: 10,
				batch: 16,
				imgsz: 640,
				lr0: 0.01,
				optimizer: "AdamW",
				augment: { mosaic: true, mixupFlip: true },
				orchestratorId: orchId,
				gpuDevice: gpuUuid,
			});

			expect(captured).toHaveLength(1);
			expect(captured[0].orchestratorId).toBe(orchId);
			expect(captured[0].gpuDevice).toBe(gpuUuid);
		} finally {
			globalThis.fetch = originalFetch;
		}
	});
});
