"use client";

import { useMemo } from "react";
import type uPlot from "uplot";
import { buildComparisonSeries } from "@/lib/compareJobs";
import type { XAxisMode } from "@/lib/metricMath";
import type { Job, MetricPointWithKey } from "@/types/jobs";
import { UPlotChart } from "./UPlotChart";

/** Paleta brand-only por run (The Brand-Only Rule — sem `emerald-*`). */
export const RUN_COLOR_PALETTE = [
	"#8350f2", // brand-500 (violeta)
	"#06b6d4", // status-telemetry (ciano)
	"#f59e0b", // status-alert (âmbar)
	"#c4b5fd", // brand-300 (violeta claro)
] as const;

export function runColor(index: number): string {
	return RUN_COLOR_PALETTE[index % RUN_COLOR_PALETTE.length];
}

export interface ComparisonChartProps {
	jobs: readonly Job[];
	pointsByJob: Record<string, MetricPointWithKey[]>;
	metricKey: string;
	xMode: XAxisMode;
	emaSmoothing: number;
	className?: string;
}

export function ComparisonChart({
	jobs,
	pointsByJob,
	metricKey,
	xMode,
	emaSmoothing,
	className = "",
}: ComparisonChartProps) {
	const jobKindByJob = useMemo(() => {
		const out: Record<string, string | undefined> = {};
		for (const job of jobs) out[job.id] = job.kind;
		return out;
	}, [jobs]);

	const comparison = useMemo(
		() =>
			buildComparisonSeries(
				pointsByJob,
				jobKindByJob,
				metricKey,
				xMode,
				emaSmoothing,
			),
		[pointsByJob, jobKindByJob, metricKey, xMode, emaSmoothing],
	);

	const uPlotData = useMemo<uPlot.AlignedData>(() => {
		if (comparison.xValues.length === 0) return [[]] as uPlot.AlignedData;
		return [
			comparison.xValues,
			...jobs.map((job) => comparison.seriesByJob[job.id] ?? []),
		] as uPlot.AlignedData;
	}, [comparison, jobs]);

	const xRange = useMemo(() => {
		if (comparison.xValues.length === 0) return undefined;
		return {
			min: comparison.xValues[0],
			max: comparison.xValues[comparison.xValues.length - 1],
		};
	}, [comparison.xValues]);

	const options = useMemo<uPlot.Options>(() => {
		const axes: uPlot.Axis[] = [
			{
				scale: "x",
				stroke: "#756d82",
				grid: { stroke: "rgba(131, 80, 242, 0.08)", width: 1 },
				ticks: { stroke: "#3e3749", width: 1 },
				values: (_self, splits) =>
					splits.map((v) =>
						xMode === "epoch" && !Number.isInteger(v)
							? `${Number(v.toFixed(2))}`
							: `${Math.round(v)}`,
					),
			},
			{
				scale: "y",
				stroke: "#9a92a6",
				grid: { stroke: "rgba(131, 80, 242, 0.08)", width: 1 },
				ticks: { stroke: "#3e3749", width: 1 },
				values: (_self, splits) =>
					splits.map((v) =>
						v == null ? "" : v < 0.01 && v > 0 ? v.toExponential(1) : v.toFixed(3),
					),
			},
		];

		const series: uPlot.Series[] = [
			{ label: xMode === "epoch" ? "Época" : "Step Global" },
			...jobs.map((job, i) => ({
				label: `${job.model} #${job.id.slice(0, 8)}`,
				stroke: runColor(i),
				width: 2,
				points: { show: false },
				spanGaps: true,
			})),
		];

		return {
			width: 800,
			height: 360,
			padding: [12, 12, 0, 0],
			scales: { x: { time: false } },
			axes,
			series,
			legend: { show: true },
			cursor: { drag: { x: true, y: false } },
		};
	}, [jobs, xMode]);

	if (jobs.length === 0 || comparison.xValues.length === 0) {
		return (
			<div
				className={`glass-card rounded-xl border border-white/10 p-6 text-center font-mono text-2xs text-zinc-400 ${className}`}
			>
				Sem pontos de métrica <code>{metricKey}</code> para os runs
				selecionados ainda.
			</div>
		);
	}

	return (
		<div
			className={`glass-card rounded-xl border border-white/10 p-4 ${className}`}
		>
			<UPlotChart
				options={options}
				data={uPlotData}
				xRange={xRange}
				resetKey={`${metricKey}:${xMode}`}
				className="w-full"
			/>
		</div>
	);
}
