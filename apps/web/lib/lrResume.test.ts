import { describe, expect, it } from "bun:test";
import type { MetricPointWithKey } from "@/types/studio";
import { buildDiffusionJobBody } from "./jobs";
import {
	describeLrResume,
	formatLr,
	lastLrAtEpoch,
	lrResumeModeForSubmit,
} from "./lrResume";

let seq = 0;
function pt(
	epoch: number | null,
	step: number,
	value: number,
	key = "lr",
): MetricPointWithKey {
	seq += 1;
	return { seq, epoch, step, key, value, ts: "2026-10-10T00:00:00Z" };
}

describe("lastLrAtEpoch", () => {
	it("picks the last lr point (highest step) of the checkpoint epoch", () => {
		const points = [
			pt(3, 30, 3e-5),
			pt(4, 40, 2e-5),
			pt(4, 48, 1.07e-5),
			pt(4, 44, 1.5e-5),
			pt(4, 48, 9e-6, "loss"),
			pt(5, 50, 5e-6),
		];
		expect(lastLrAtEpoch(points, 4)).toBe(1.07e-5);
	});

	it("is order independent and ignores points of other epochs", () => {
		const points = [pt(5, 50, 5e-6), pt(4, 48, 1e-5), pt(4, 40, 2e-5)];
		expect(lastLrAtEpoch(points, 4)).toBe(1e-5);
	});

	it("returns null without data for that epoch", () => {
		expect(lastLrAtEpoch([], 4)).toBeNull();
		expect(lastLrAtEpoch(null, 4)).toBeNull();
		expect(lastLrAtEpoch([pt(3, 30, 1e-5), pt(null, 1, 1e-5)], 4)).toBeNull();
		expect(lastLrAtEpoch([pt(4, 40, Number.NaN)], 4)).toBeNull();
	});
});

describe("describeLrResume", () => {
	const base = { requestLr: 8e-5, epochs: 6, scheduler: "cosine" as const };

	it("continue with checkpoint LR", () => {
		expect(
			describeLrResume({ ...base, mode: "continue", checkpointLr: 1.07e-5 }),
		).toBe("Começa em 1.07e-5 e desce até 0 em 6 épocas (cosine).");
	});

	it("continue without data is neutral", () => {
		const text = describeLrResume({
			...base,
			mode: "continue",
			checkpointLr: null,
		});
		expect(text).toContain("LR salvo no checkpoint");
		expect(text).not.toContain("e-");
	});

	it("restart uses the request LR", () => {
		expect(
			describeLrResume({ ...base, mode: "restart", checkpointLr: 1.07e-5 }),
		).toBe("Começa em 8e-5 e desce até 0 em 6 épocas (cosine).");
	});
});

describe("formatLr", () => {
	it("uses short scientific notation", () => {
		expect(formatLr(1.07e-5)).toBe("1.07e-5");
		expect(formatLr(1e-4)).toBe("1e-4");
	});
});

describe("submit body", () => {
	const common = { datasetId: "ds", baseModel: "flux" as const, epochs: 6 };

	it("resume sends epochOffset and lrResumeMode", () => {
		for (const mode of ["continue", "restart"] as const) {
			const body = buildDiffusionJobBody({
				...common,
				epochOffset: 4,
				lrResumeMode: lrResumeModeForSubmit(4, mode),
			});
			expect(body.epochOffset).toBe(4);
			expect(body.lrResumeMode).toBe(mode);
		}
	});

	it("new training does not send lrResumeMode", () => {
		const body = buildDiffusionJobBody({
			...common,
			epochOffset: undefined,
			lrResumeMode: lrResumeModeForSubmit(0, "continue"),
		});
		expect("lrResumeMode" in body).toBe(false);
		// Mesmo que alguém passe o modo direto, sem retomada ele é descartado.
		const forced = buildDiffusionJobBody({
			...common,
			lrResumeMode: "restart",
		});
		expect("lrResumeMode" in forced).toBe(false);
	});
});
