import { describe, expect, it } from "bun:test";
import {
	isFlux2Klein,
	isFlux2Klein4b,
	isFlux2Klein9b,
	isLoraCompatible,
} from "@/lib/diffusionArch";
import { estimateDiffusionVramGb } from "@/components/studio/diffusion/estimateDiffusionVram";
import { geracaoFormFromGeneration, loadGeracaoForm, GERACAO_FORM_KEY } from "@/lib/geracao-storage";
import type { Generation } from "@/types/studio";

describe("diffusionArch", () => {
	it("família Flux.2 inclui 4B, 9B e alias flux; 4B-only exclui 9B", () => {
		expect(isFlux2Klein("flux-2-klein-9b")).toBe(true);
		expect(isFlux2Klein("flux")).toBe(true);
		expect(isFlux2Klein("sdxl")).toBe(false);
		expect(isFlux2Klein4b("flux-2-klein-9b")).toBe(false);
		expect(isFlux2Klein4b("flux")).toBe(true);
		expect(isFlux2Klein9b("flux-2-klein-9b")).toBe(true);
	});

	it("LoRA: arch nulo passa; arch diferente é incompatível", () => {
		expect(isLoraCompatible(null, "flux-2-klein-9b")).toBe(true);
		expect(isLoraCompatible("flux-2-klein-4b", "flux-2-klein-9b")).toBe(false);
		expect(isLoraCompatible("flux-2-klein-9b", "flux-2-klein-9b")).toBe(true);
	});
});

describe("estimateDiffusionVramGb 9B", () => {
	it("4-bit default ≈ 12 GB e tiers crescentes", () => {
		const q = (x: "2bit" | "4bit" | "6bit" | "8bit" | "none") =>
			estimateDiffusionVramGb("flux-2-klein-9b", 1, 16, 768, "paged_adamw8bit", "bf16", x);
		expect(q("4bit")).toBeGreaterThanOrEqual(11.5);
		expect(q("4bit")).toBeLessThanOrEqual(12);
		expect(q("6bit")).toBeLessThanOrEqual(20);
		expect(q("6bit")).toBeGreaterThan(q("4bit"));
		expect(q("none")).toBeLessThanOrEqual(28);
		expect(q("none")).toBeGreaterThan(q("8bit"));
	});
});

describe("geracao-storage 9B", () => {
	it("form persistido 9B força distilled=false e encoder vazio", () => {
		const store: Record<string, string> = {};
		// @ts-expect-error mock localStorage
		globalThis.window = { localStorage: { getItem: (k: string) => store[k] ?? null, setItem: () => {}, removeItem: () => {} } };
		// @ts-expect-error mock localStorage
		globalThis.localStorage = globalThis.window.localStorage;
		store[GERACAO_FORM_KEY] = JSON.stringify({
			version: 1,
			state: { baseModel: "flux-2-klein-9b", distilled: true, textEncoderModelId: "abc" },
		});
		const form = loadGeracaoForm();
		expect(form?.baseModel).toBe("flux-2-klein-9b");
		expect(form?.distilled).toBe(false);
		expect(form?.textEncoderModelId).toBe("");
	});

	it("reaplicar geração 9B mantém base, sampler flux válido e distilled=false", () => {
		const gen = {
			prompt: "x",
			params: { baseModel: "flux-2-klein-9b", sampler: "dpmpp_2m", distilled: true },
		} as unknown as Generation;
		const { form, warnings } = geracaoFormFromGeneration(gen);
		expect(form.baseModel).toBe("flux-2-klein-9b");
		expect(form.distilled).toBe(false);
		expect(form.sampler).toBe("default");
		expect(warnings.length).toBeGreaterThan(0);
	});
});
