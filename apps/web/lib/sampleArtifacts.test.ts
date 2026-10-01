import { describe, expect, it } from "bun:test";
import {
	buildSampleTimeline,
	formatSampleLabel,
	isSampleArtifact,
	parseSampleEpoch,
	parseSamplePromptKey,
} from "./sampleArtifacts";
import type { JobArtifact } from "@/types/jobs";

function art(path: string, kind = "sample", id = path): JobArtifact {
	return { id, kind, path, md5: "deadbeef", bytes: 1024 };
}

describe("sampleArtifacts - parseSampleEpoch", () => {
	it("parses the baseline epoch 000 emitted by trainer-difusao/loop.py", () => {
		expect(parseSampleEpoch("samples/sample_epoch_000.png")).toBe(0);
	});

	it("parses a mid-training epoch with zero-padded digits", () => {
		expect(parseSampleEpoch("samples/sample_epoch_015.png")).toBe(15);
	});

	it("parses large epoch numbers without padding assumptions", () => {
		expect(parseSampleEpoch("samples/sample_epoch_123.png")).toBe(123);
	});

	it("returns null for paths without an epoch segment", () => {
		expect(parseSampleEpoch("samples/preview.png")).toBeNull();
	});
});

describe("sampleArtifacts - parseSamplePromptKey", () => {
	it("defaults to 'default' for the real single-sample-per-epoch naming", () => {
		expect(parseSamplePromptKey("samples/sample_epoch_004.png")).toBe("default");
	});

	it("extracts a prompt suffix when present (future multi-prompt naming)", () => {
		expect(parseSamplePromptKey("samples/sample_epoch_004_catA.png")).toBe("catA");
	});

	it("defaults to 'default' for untagged paths", () => {
		expect(parseSamplePromptKey("samples/preview.png")).toBe("default");
	});
});

describe("sampleArtifacts - formatSampleLabel", () => {
	it("labels epoch 0 as Baseline", () => {
		expect(formatSampleLabel("samples/sample_epoch_000.png")).toBe("Baseline (Época 0)");
	});

	it("labels other epochs as Época N", () => {
		expect(formatSampleLabel("samples/sample_epoch_007.png")).toBe("Época 7");
	});

	it("falls back to the filename stem when no epoch is present", () => {
		expect(formatSampleLabel("samples/preview.png")).toBe("preview");
	});
});

describe("sampleArtifacts - isSampleArtifact", () => {
	it("accepts kind sample", () => {
		expect(isSampleArtifact(art("outputs/foo.bin", "sample"))).toBe(true);
	});

	it("accepts samples/ prefix regardless of kind", () => {
		expect(isSampleArtifact(art("samples/sample_epoch_000.png", "other"))).toBe(true);
	});

	it("rejects model and metrics kinds even with png extension", () => {
		expect(isSampleArtifact(art("outputs/lora.png", "model"))).toBe(false);
		expect(isSampleArtifact(art("outputs/loss.png", "metrics"))).toBe(false);
	});
});

describe("sampleArtifacts - buildSampleTimeline", () => {
	it("orders baseline first then ascending epochs, from the real trainer naming", () => {
		const artifacts = [
			art("samples/sample_epoch_010.png"),
			art("samples/sample_epoch_000.png"),
			art("samples/sample_epoch_005.png"),
		];
		const { steps } = buildSampleTimeline(artifacts);
		expect(steps.map((s) => s.epoch)).toEqual([0, 5, 10]);
		expect(steps[0].isBaseline).toBe(true);
		expect(steps[0].label).toBe("Baseline (Época 0)");
		expect(steps[2].label).toBe("Época 10");
	});

	it("groups same-epoch samples by prompt key", () => {
		const artifacts = [
			art("samples/sample_epoch_002_catA.png", "sample", "a"),
			art("samples/sample_epoch_002_catB.png", "sample", "b"),
		];
		const { steps } = buildSampleTimeline(artifacts);
		expect(steps).toHaveLength(1);
		expect(steps[0].groups.map((g) => g.promptKey)).toEqual(["catA", "catB"]);
	});

	it("collects artifacts without a parsable epoch into untagged", () => {
		const artifacts = [art("samples/sample_epoch_000.png"), art("samples/weird.png")];
		const { steps, untagged } = buildSampleTimeline(artifacts);
		expect(steps).toHaveLength(1);
		expect(untagged).toHaveLength(1);
		expect(untagged[0].path).toBe("samples/weird.png");
	});

	it("excludes non-sample artifacts like checkpoints and metrics", () => {
		const artifacts = [
			art("samples/sample_epoch_000.png", "sample"),
			art("checkpoints/lora.safetensors", "model"),
			art("metrics/loss.jsonl", "metrics"),
		];
		const { steps } = buildSampleTimeline(artifacts);
		expect(steps).toHaveLength(1);
	});
});
