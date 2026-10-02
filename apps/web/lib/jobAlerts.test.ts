import { describe, expect, it } from "bun:test";
import {
	alertSeverityLabel,
	applyAlertsSnapshot,
	countActiveAlerts,
	RULE_LABEL,
	sortAlertsByFiredAt,
} from "./jobAlerts";
import type { JobAlert } from "@/types/jobs";

function makeAlert(overrides: Partial<JobAlert> = {}): JobAlert {
	return {
		id: "alert-1",
		jobId: "job-1",
		ruleId: "nan_detected",
		severity: "critical",
		message: "Loss NaN detectado no step 42",
		firedAt: "2026-10-02T10:00:00Z",
		resolvedAt: null,
		details: null,
		...overrides,
	};
}

describe("jobAlerts", () => {
	describe("countActiveAlerts", () => {
		it("retorna 0 para null ou undefined", () => {
			expect(countActiveAlerts(null)).toBe(0);
			expect(countActiveAlerts(undefined)).toBe(0);
		});

		it("retorna 0 quando a lista está vazia (0 alertas → sem badge)", () => {
			expect(countActiveAlerts([])).toBe(0);
		});

		it("retorna 0 quando todos os alertas estão resolvidos", () => {
			const alerts: JobAlert[] = [
				makeAlert({ id: "a1", resolvedAt: "2026-10-02T10:05:00Z" }),
				makeAlert({ id: "a2", resolvedAt: "2026-10-02T10:10:00Z" }),
			];
			expect(countActiveAlerts(alerts)).toBe(0);
		});

		it("com ativos + resolvidos, a contagem só inclui ativos", () => {
			const alerts: JobAlert[] = [
				makeAlert({ id: "a1", resolvedAt: null }),
				makeAlert({ id: "a2", resolvedAt: "2026-10-02T10:05:00Z" }),
				makeAlert({ id: "a3", resolvedAt: null }),
				makeAlert({ id: "a4", resolvedAt: "2026-10-02T10:15:00Z" }),
			];
			expect(countActiveAlerts(alerts)).toBe(2);
		});
	});

	describe("alertSeverityLabel e RULE_LABEL", () => {
		it("retorna o rótulo de severidade correto", () => {
			expect(alertSeverityLabel("critical")).toBe("Crítico");
			expect(alertSeverityLabel("warning")).toBe("Aviso");
		});

		it("mapeia as regras conhecidas", () => {
			expect(RULE_LABEL.nan_detected).toBe("NaN/Inf detectado");
			expect(RULE_LABEL.vram_high).toBe("VRAM elevada");
			expect(RULE_LABEL.disk_high).toBe("Disco elevado");
			expect(RULE_LABEL.telemetry_stale).toBe("Telemetria parada");
		});
	});

	describe("sortAlertsByFiredAt", () => {
		it("ordena do mais recente ao mais antigo pelo firedAt sem mutar", () => {
			const a1 = makeAlert({ id: "a1", firedAt: "2026-10-02T10:00:00Z" });
			const a2 = makeAlert({ id: "a2", firedAt: "2026-10-02T10:20:00Z" });
			const a3 = makeAlert({ id: "a3", firedAt: "2026-10-02T10:10:00Z" });
			const original = [a1, a2, a3];

			const sorted = sortAlertsByFiredAt(original);
			expect(sorted.map((a) => a.id)).toEqual(["a2", "a3", "a1"]);
			// Não mutou
			expect(original.map((a) => a.id)).toEqual(["a1", "a2", "a3"]);
		});

		it("lida com lista nula ou vazia", () => {
			expect(sortAlertsByFiredAt(null)).toEqual([]);
			expect(sortAlertsByFiredAt([])).toEqual([]);
		});
	});

	describe("applyAlertsSnapshot", () => {
		it("snapshot substitui completamente a lista anterior (idempotente)", () => {
			const previous: JobAlert[] = [
				makeAlert({ id: "a1", message: "antigo 1" }),
				makeAlert({ id: "a2", message: "antigo 2" }),
			];
			const incoming: JobAlert[] = [
				makeAlert({ id: "a3", message: "novo 3" }),
			];

			const next = applyAlertsSnapshot(previous, incoming);
			expect(next).toEqual(incoming);
			expect(next).not.toBe(incoming); // nova referência
			expect(next).not.toContainEqual(previous[0]);
		});

		it("substitui quando a lista anterior era nula", () => {
			const incoming: JobAlert[] = [makeAlert({ id: "a1" })];
			expect(applyAlertsSnapshot(null, incoming)).toEqual(incoming);
		});

		it("snapshot vazio zera a lista de alertas", () => {
			const previous: JobAlert[] = [makeAlert({ id: "a1" })];
			expect(applyAlertsSnapshot(previous, [])).toEqual([]);
		});
	});
});
