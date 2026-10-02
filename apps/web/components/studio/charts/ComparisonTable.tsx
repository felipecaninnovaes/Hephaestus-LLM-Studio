"use client";

import { useMemo } from "react";
import {
	Table,
	TableBody,
	TableCell,
	TableHead,
	TableHeader,
	TableRow,
} from "@/components/ui/Table";
import { buildHyperparamRows } from "@/lib/compareJobs";
import type { Job } from "@/types/jobs";
import { runColor } from "./ComparisonChart";

export interface ComparisonTableProps {
	jobs: readonly Job[];
	className?: string;
}

/**
 * Tabela de hiperparâmetros lado a lado (`job.params` achatado) destacando
 * linhas cujo valor diverge entre os runs selecionados (fatia 4c).
 */
export function ComparisonTable({ jobs, className = "" }: ComparisonTableProps) {
	const rows = useMemo(() => buildHyperparamRows(jobs), [jobs]);

	if (rows.length === 0) {
		return (
			<div
				className={`glass-card rounded-xl border border-white/10 p-4 text-center font-mono text-2xs text-zinc-400 ${className}`}
			>
				Nenhum parâmetro registrado para os runs selecionados.
			</div>
		);
	}

	return (
		<div className={className}>
			<Table containerClassName="max-h-[480px] overflow-y-auto">
				<TableHeader className="sticky top-0 z-10 bg-zinc-900/95 backdrop-blur-sm">
					<TableRow>
						<TableHead className="font-mono text-2xs uppercase tracking-caps text-zinc-400">
							Parâmetro
						</TableHead>
						{jobs.map((job, i) => (
							<TableHead key={job.id} className="font-mono text-2xs text-zinc-300">
								<span className="flex items-center gap-1.5">
									<span
										className="size-2 shrink-0 rounded-full"
										style={{ backgroundColor: runColor(i) }}
										aria-hidden="true"
									/>
									<span className="truncate" title={`${job.model} #${job.id}`}>
										{job.model} #{job.id.slice(0, 8)}
									</span>
								</span>
							</TableHead>
						))}
					</TableRow>
				</TableHeader>
				<TableBody>
					{rows.map((row) => (
						<TableRow
							key={row.key}
							className={row.differs ? "bg-status-alert/[0.06]" : undefined}
						>
							<TableCell
								className="font-mono text-2xs text-zinc-400"
								title={row.key}
							>
								{row.key}
							</TableCell>
							{jobs.map((job) => (
								<TableCell
									key={job.id}
									className={`font-mono text-2xs ${
										row.differs ? "font-semibold text-amber-200" : "text-zinc-200"
									}`}
								>
									{row.valuesByJob[job.id]}
								</TableCell>
							))}
						</TableRow>
					))}
				</TableBody>
			</Table>
		</div>
	);
}
