import { describe, expect, it, beforeEach, afterEach } from "bun:test";
import {
	subscribeTelemetry,
	getTelemetrySnapshot,
	resetTelemetryStoreForTest,
} from "./telemetryStore";
import type { Telemetry } from "@/types/studio";

describe("telemetryStore", () => {
	const mockTelemetry: Telemetry = {
		node: "local-gpu",
		gpus: ["RTX 4090"],
		vramTotal: 24576,
		vramUsed: 4096,
		vramReserved: 6144,
		vramFree: 18432,
		cpu: 12.5,
		ramTotal: 65536,
		ramUsed: 16384,
		jobsActive: 1,
		jobsQueued: 0,
		jobsTotal: 10,
	};

	let originalFetch: typeof globalThis.fetch;
	let fetchCalls = 0;

	beforeEach(() => {
		resetTelemetryStoreForTest();
		fetchCalls = 0;
		originalFetch = globalThis.fetch;
		globalThis.fetch = async (input: RequestInfo | URL) => {
			const url = typeof input === "string" ? input : input.toString();
			if (url.includes("/api/telemetry")) {
				fetchCalls++;
				return new Response(JSON.stringify(mockTelemetry), {
					status: 200,
					headers: { "Content-Type": "application/json" },
				});
			}
			return new Response("Not found", { status: 404 });
		};
	});

	afterEach(() => {
		resetTelemetryStoreForTest();
		globalThis.fetch = originalFetch;
	});

	it("does not poll when there are no subscribers", async () => {
		const { promise, resolve } = Promise.withResolvers<void>();
		setTimeout(resolve, 50);
		await promise;
		expect(fetchCalls).toBe(0);
	});

	it("polls immediately upon first subscription and stores snapshot", async () => {
		const { promise, resolve } = Promise.withResolvers<void>();
		const unsub = subscribeTelemetry(() => {
			resolve();
		}, 100);

		await promise;
		expect(fetchCalls).toBe(1);
		expect(getTelemetrySnapshot()).toEqual(mockTelemetry);

		const { promise: nextTickPromise, resolve: nextTickResolve } = Promise.withResolvers<void>();
		setTimeout(nextTickResolve, 120);
		await nextTickPromise;
		expect(fetchCalls).toBe(2);

		unsub();
	});

	it("uses the shortest interval among all active subscribers", async () => {
		const { promise: firstSubPromise, resolve: firstSubResolve } = Promise.withResolvers<void>();
		const unsub1 = subscribeTelemetry(() => {
			firstSubResolve();
		}, 200);

		await firstSubPromise;
		expect(fetchCalls).toBe(1);

		// Add subscriber with shorter interval (60ms)
		const unsub2 = subscribeTelemetry(() => {}, 60);

		const { promise: shortWaitPromise, resolve: shortWaitResolve } = Promise.withResolvers<void>();
		setTimeout(shortWaitResolve, 80);
		await shortWaitPromise;

		expect(fetchCalls).toBeGreaterThanOrEqual(2);

		unsub2();
		unsub1();
	});

	it("stops polling when all subscribers unsubscribe", async () => {
		const { promise: firstSubPromise, resolve: firstSubResolve } = Promise.withResolvers<void>();
		const unsub = subscribeTelemetry(() => {
			firstSubResolve();
		}, 50);

		await firstSubPromise;
		expect(fetchCalls).toBe(1);

		unsub();
		const callsAfterUnsub = fetchCalls;

		const { promise: waitPromise, resolve: waitResolve } = Promise.withResolvers<void>();
		setTimeout(waitResolve, 120);
		await waitPromise;

		expect(fetchCalls).toBe(callsAfterUnsub);
	});
});
