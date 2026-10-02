"use client";

import { useState } from "react";
import { IconAlertTriangle, IconCheck } from "@/components/icons";
import { Badge } from "@/components/ui/Badge";
import { Modal } from "@/components/ui/Modal";
import { formatRelativeTime } from "@/lib/format";
import type { JobAlert, JobAlertRuleId } from "@/types/jobs";

const RULE_LABEL: Record<JobAlertRuleId, string> = {
	nan_detected: "NaN/Inf detectado",
	vram_high: "VRAM elevada",
	disk_high: "Disco elevado",
	telemetry_stale: "Telemetria parada",
};

/** Alertas ativos = sem `resolvedAt`. */
export function countActiveAlerts(alerts: JobAlert[] | null | undefined): number {
	if (!alerts) return 0;
	return alerts.filter((a) => !a.resolvedAt).length;
}

export interface JobAlertsBadgeProps {
	alerts: JobAlert[] | null | undefined;
	onOpen: () => void;
	className?: string;
}

/**
 * Badge compacto com a contagem de alertas ATIVOS do job (fatia 3c-UI).
 * Só renderiza quando há pelo menos 1 alerta ativo — card/detalhe silencioso
 * no caso comum (sem alertas).
 */
export function JobAlertsBadge({
	alerts,
	onOpen,
	className = "",
}: JobAlertsBadgeProps) {
	const activeCount = countActiveAlerts(alerts);
	if (activeCount === 0) return null;

	return (
		<button
			type="button"
			onClick={(e) => {
				e.stopPropagation();
				onOpen();
			}}
			title={`${activeCount} alerta${activeCount > 1 ? "s" : ""} ativo${activeCount > 1 ? "s" : ""} — clique para ver`}
			className={`relative z-10 inline-flex min-h-[28px] items-center gap-1 rounded-full border border-status-danger/40 bg-status-danger/10 px-2 py-0.5 font-mono text-2xs font-semibold text-rose-300 transition hover:bg-status-danger/20 active:scale-[0.97] cursor-pointer ${className}`}
		>
			<IconAlertTriangle className="size-3" />
			{activeCount}
		</button>
	);
}

export interface JobAlertsModalProps {
	open: boolean;
	onClose: () => void;
	alerts: JobAlert[] | null | undefined;
	isLoading?: boolean;
}

/** Lista completa (ativos + resolvidos) de alertas de um job (fatia 3c-UI). */
export function JobAlertsModal({
	open,
	onClose,
	alerts,
	isLoading = false,
}: JobAlertsModalProps) {
	const items = alerts ?? [];
	const sorted = [...items].sort(
		(a, b) => new Date(b.firedAt).getTime() - new Date(a.firedAt).getTime(),
	);

	return (
		<Modal
			open={open}
			onClose={onClose}
			title="Alertas do Job"
			icon={<IconAlertTriangle className="size-4" />}
			maxWidth="md"
		>
			{isLoading ? (
				<p className="font-mono text-2xs text-zinc-400">Carregando…</p>
			) : sorted.length === 0 ? (
				<div className="flex flex-col items-center gap-2 py-6 text-center">
					<IconCheck className="size-6 text-status-success" />
					<p className="font-mono text-2xs text-zinc-400">
						Nenhum alerta registrado para este job.
					</p>
				</div>
			) : (
				<ul className="space-y-2">
					{sorted.map((alert) => {
						const active = !alert.resolvedAt;
						return (
							<li
								key={alert.id}
								className={`rounded-lg border p-3 ${
									active
										? "border-status-danger/30 bg-status-danger/[0.06]"
										: "border-white/10 bg-white/[0.03]"
								}`}
							>
								<div className="flex flex-wrap items-center gap-2">
									<Badge variant={alert.severity === "critical" ? "danger" : "alert"}>
										{alert.severity === "critical" ? "Crítico" : "Aviso"}
									</Badge>
									<span className="font-mono text-2xs font-semibold text-zinc-200">
										{RULE_LABEL[alert.ruleId] ?? alert.ruleId}
									</span>
									{active ? (
										<span className="font-mono text-3xs uppercase tracking-caps text-rose-300">
											Ativo
										</span>
									) : (
										<span className="font-mono text-3xs uppercase tracking-caps text-status-success">
											Resolvido
										</span>
									)}
								</div>
								<p className="mt-1.5 text-xs text-zinc-300">{alert.message}</p>
								<div className="mt-1.5 flex flex-wrap gap-3 font-mono text-3xs text-zinc-500">
									<span title={alert.firedAt}>
										Disparado {formatRelativeTime(alert.firedAt)}
									</span>
									{alert.resolvedAt && (
										<span title={alert.resolvedAt}>
											Resolvido {formatRelativeTime(alert.resolvedAt)}
										</span>
									)}
								</div>
							</li>
						);
					})}
				</ul>
			)}
		</Modal>
	);
}
