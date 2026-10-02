import type { JobAlert, JobAlertRuleId } from "@/types/jobs";

export const RULE_LABEL: Record<JobAlertRuleId, string> = {
	nan_detected: "NaN/Inf detectado",
	vram_high: "VRAM elevada",
	disk_high: "Disco elevado",
	telemetry_stale: "Telemetria parada",
};

/** Alertas ativos = sem `resolvedAt`. */
export function countActiveAlerts(
	alerts: readonly JobAlert[] | null | undefined,
): number {
	if (!alerts) return 0;
	return alerts.filter((a) => !a.resolvedAt).length;
}

/** Rótulo legível para a severidade do alerta. */
export function alertSeverityLabel(severity: JobAlert["severity"]): string {
	return severity === "critical" ? "Crítico" : "Aviso";
}

/**
 * Ordena os alertas do mais recente ao mais antigo pelo `firedAt`.
 * Não muta o array original.
 */
export function sortAlertsByFiredAt(
	alerts: readonly JobAlert[] | null | undefined,
): JobAlert[] {
	if (!alerts || alerts.length === 0) return [];
	return [...alerts].sort(
		(a, b) => new Date(b.firedAt).getTime() - new Date(a.firedAt).getTime(),
	);
}

/**
 * Aplica um novo snapshot completo de alertas, substituindo a lista anterior.
 * Idempotente — reflete o evento SSE 'alerts' da fatia 3c.
 */
export function applyAlertsSnapshot(
	_previous: readonly JobAlert[] | null | undefined,
	incoming: readonly JobAlert[],
): JobAlert[] {
	return [...incoming];
}
