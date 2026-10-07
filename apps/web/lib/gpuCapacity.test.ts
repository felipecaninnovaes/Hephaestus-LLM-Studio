import { describe, expect, it } from "bun:test";
import { calculateOomRisk } from "@/hooks/useVramEstimator";
import { resolveTargetGpu } from "@/lib/gpuCapacity";
import type { GpuDeviceTelemetry, Orchestrator } from "@/lib/monitoring";

const gpu = (index: number, name: string, vramTotal: number): GpuDeviceTelemetry => ({
	index,
	uuid: `GPU-${index}`,
	name,
	vramTotal,
	vramUsed: 0,
});

const node = (over: Partial<Orchestrator>): Orchestrator => ({
	id: "n",
	name: "n",
	kind: "remoto",
	endpoint: "x",
	status: "online",
	lastHeartbeat: null,
	measured: true,
	cpu: null,
	ram: null,
	ramTotal: null,
	vramUsed: null,
	vramTotal: null,
	vramTotalGb: null,
	gpus: [],
	jobsActive: 0,
	...over,
});

const docker04 = node({
	id: "docker-04",
	name: "docker-04",
	vramTotal: 18432,
	gpuDevices: [gpu(0, "RTX 3060", 12288), gpu(1, "GTX 1660 SUPER", 6144)],
});

describe("resolveTargetGpu", () => {
	it("GPU escolhida vence a maior do nó", () => {
		expect(resolveTargetGpu([docker04], "docker-04", "GPU-1")).toEqual({
			vramTotalGb: 6,
			label: "GPU 1: GTX 1660 SUPER (6 GB)",
		});
	});

	it("nó sem GPU escolhida usa a maior placa do nó, não a soma", () => {
		expect(resolveTargetGpu([docker04], "docker-04", null)).toEqual({
			vramTotalGb: 12,
			label: "GPU 0: RTX 3060 (12 GB)",
		});
	});

	it("Automático usa a maior placa entre nós online e ignora offline", () => {
		const big = node({
			id: "big",
			status: "offline",
			gpuDevices: [gpu(0, "RTX 4090", 24576)],
		});
		const small = node({ id: "small", gpuDevices: [gpu(0, "RTX 3050", 8192)] });
		expect(resolveTargetGpu([big, small, docker04], null, null)?.vramTotalGb).toBe(12);
		expect(resolveTargetGpu([big], null, null)).toBeNull();
	});

	it("converte MiB→GB com 1 casa", () => {
		const n = node({ gpuDevices: [gpu(0, "RTX 3060", 12000)] });
		expect(resolveTargetGpu([n], null, null)?.vramTotalGb).toBe(11.7);
	});

	it("nó sem gpuDevices mas com vramTotal (MiB) usa o agregado e o nome do nó", () => {
		const n = node({ id: "a", name: "agg", vramTotal: 8192 });
		expect(resolveTargetGpu([n], "a", null)).toEqual({ vramTotalGb: 8, label: "agg (8 GB)" });
	});

	it("sem medição ⇒ null", () => {
		expect(resolveTargetGpu([], null, null)).toBeNull();
		expect(resolveTargetGpu([node({})], null, null)).toBeNull();
		expect(resolveTargetGpu([docker04], "inexistente", null)).toBeNull();
	});

	it("device de 12288 MiB: 4 GB é safe e null é safe", () => {
		const target = resolveTargetGpu([docker04], "docker-04", "GPU-0");
		expect(calculateOomRisk(4, target?.vramTotalGb ?? null)).toBe("safe");
		expect(calculateOomRisk(4, null)).toBe("safe");
		expect(calculateOomRisk(10.4, 6)).toBe("danger");
		expect(calculateOomRisk(10.4, 12)).toBe("warning");
	});
});
