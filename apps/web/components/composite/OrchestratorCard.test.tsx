import { describe, expect, it } from "bun:test";
import { createElement } from "react";
import { renderToString } from "react-dom/server";
import type { Orchestrator } from "@/lib/monitoring";
import { OrchestratorCard } from "./OrchestratorCard";

describe("OrchestratorCard integration with MultiGpuRack", () => {
	const legacyCpuNode: Orchestrator = {
		id: "orch-cpu-1",
		name: "cpu-orchestrator",
		kind: "local",
		endpoint: "http://127.0.0.1:9090",
		status: "online",
		lastHeartbeat: new Date().toISOString(),
		measured: true,
		cpu: 25.5,
		ram: 8 * 1024 * 1024 * 1024,
		ramTotal: 32 * 1024 * 1024 * 1024,
		vramUsed: null,
		vramTotal: null,
		vramTotalGb: null,
		gpus: [],
		gpuDevices: null,
		jobsActive: 0,
	};

	const multiGpuNode: Orchestrator = {
		id: "orch-gpu-docker-04",
		name: "docker-04",
		kind: "remoto",
		endpoint: "http://10.15.50.114:9090",
		status: "online",
		lastHeartbeat: new Date().toISOString(),
		measured: true,
		cpu: 45.2,
		ram: 16 * 1024 * 1024 * 1024,
		ramTotal: 32 * 1024 * 1024 * 1024,
		vramUsed: 5120,
		vramTotal: 18432,
		vramTotalGb: 12,
		gpus: ["NVIDIA GeForce RTX 3060", "NVIDIA GeForce GTX 1660 SUPER"],
		gpuDevices: [
			{
				index: 0,
				uuid: "GPU-1c1e01c2-4192-8f38-1a8a-33fb78b06f17",
				name: "NVIDIA GeForce RTX 3060",
				vramTotal: 12288,
				vramUsed: 4096,
				powerWatts: 115,
				gpuUtilizationPct: 78,
				temperatureC: 66,
			},
			{
				index: 1,
				uuid: "GPU-c83cc056-07f7-d31e-cc98-7486ddac0296",
				name: "NVIDIA GeForce GTX 1660 SUPER",
				vramTotal: 6144,
				vramUsed: 1024,
				powerWatts: null,
				gpuUtilizationPct: null,
				temperatureC: null,
			},
		],
		jobsActive: 1,
	};

	it("renders legacy CPU/mock node without crashing and without multi-GPU rack", () => {
		const html = renderToString(
			createElement(OrchestratorCard, { node: legacyCpuNode }),
		);
		expect(html).toContain("cpu-orchestrator");
		expect(html).toContain("sem GPU");
		expect(html).not.toContain("RACK MULTI-GPU");
	});

	it("renders multi-GPU node with MultiGpuRack inside OrchestratorCard", () => {
		const html = renderToString(
			createElement(OrchestratorCard, { node: multiGpuNode }),
		);
		expect(html).toContain("docker-04");
		expect(html).toContain("RACK MULTI-GPU (2 PLACAS)");
		expect(html).toContain("NVIDIA GeForce RTX 3060");
		expect(html).toContain("NVIDIA GeForce GTX 1660 SUPER");
		expect(html).toContain("GPU-1c1e01c2-4192-8f38-1a8a-33fb78b06f17");
		expect(html).toContain("GPU-c83cc056-07f7-d31e-cc98-7486ddac0296");
		expect(html).toContain("66°C");
	});
});
