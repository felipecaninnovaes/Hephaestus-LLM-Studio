"use client";

import Link from "next/link";
import { useSearchParams } from "next/navigation";
import { Suspense, useEffect, useMemo, useState } from "react";
import { IconChevronDown, IconLayers } from "@/components/icons";
import {
	ComparisonChart,
	ComparisonTable,
	runColor,
} from "@/components/studio/charts";
import { Select } from "@/components/ui/Select";
import { Slider } from "@/components/ui/Slider";
import { Spinner } from "@/components/ui/Spinner";
import { useJobsMetricsCompare } from "@/hooks/useJobsMetricsCompare";
import { getJob } from "@/lib/jobs";
import type { XAxisMode } from "@/lib/metricMath";
import type { Job } from "@/types/jobs";

const COMPARE_MIN_JOBS = 2;
const COMPARE_MAX_JOBS = 4;

const DIFFUSION_KEYS = ["loss", "lr", "grad_norm"];
const YOLO_KEYS = ["box_loss", "cls_loss", "dfl_loss", "mAP50", "mAP50-95"];

function CompareContent() {
	const searchParams = useSearchParams();
	const idsParam = searchParams.get("ids") ?? "";
	const jobIds = useMemo(
		() =>
			Array.from(
				new Set(
					idsParam
						.split(",")
						.map((s) => s.trim())
						.filter(Boolean),
				),
			).slice(0, COMPARE_MAX_JOBS),
		[idsParam],
	);

	const [jobs, setJobs] = useState<Job[]>([]);
	const [loadError, setLoadError] = useState<string | null>(null);
	const [loadingJobs, setLoadingJobs] = useState(true);
	const [xMode, setXMode] = useState<XAxisMode>("global_step");
	const [emaSmoothing, setEmaSmoothing] = useState(0.6);
	const [metricKey, setMetricKey] = useState<string | null>(null);

	useEffect(() => {
		if (jobIds.length === 0) {
			setJobs([]);
			setLoadingJobs(false);
			return;
		}
		const controller = new AbortController();
		setLoadingJobs(true);
		Promise.all(
			jobIds.map((id) =>
				getJob(id).catch(() => null),
			),
		).then((results) => {
			if (controller.signal.aborted) return;
			const loaded = results.filter((j): j is Job => j != null);
			setJobs(loaded);
			setLoadError(
				loaded.length < jobIds.length
					? `${jobIds.length - loaded.length} job(s) não encontrado(s).`
					: null,
			);
			setLoadingJobs(false);
		});
		return () => controller.abort();
	}, [jobIds]);

	const isYoloGroup = jobs.length > 0 && jobs.every((j) => j.kind.startsWith("yolo"));
	const requestedKeys = isYoloGroup ? YOLO_KEYS : DIFFUSION_KEYS;

	const { seriesByJob, isLoading: metricsLoading } = useJobsMetricsCompare(
		jobs.map((j) => j.id),
		{ keys: requestedKeys, maxPoints: 2500, enabled: jobs.length > 0 },
	);

	const pointsByJob = useMemo(() => {
		const out: Record<string, typeof seriesByJob[string]["points"]> = {};
		for (const job of jobs) out[job.id] = seriesByJob[job.id]?.points ?? [];
		return out;
	}, [jobs, seriesByJob]);

	const defaultMetricKey = isYoloGroup ? "box_loss" : "loss";
	const activeMetricKey = metricKey ?? defaultMetricKey;

	const metricOptions = requestedKeys.map((k) => ({ value: k, label: k }));

	return (
		<div className="mx-auto max-w-[1600px] w-full px-4 py-5 md:px-6 lg:px-8 space-y-6">
			<div className="flex flex-col gap-3 border-b border-white/10 pb-4 sm:flex-row sm:items-center sm:justify-between">
				<div>
					<Link
						href="/jobs"
						className="inline-flex items-center gap-1 font-mono text-2xs text-zinc-400 hover:text-zinc-200 transition"
					>
						<IconChevronDown className="size-3 rotate-90" />
						Voltar às Execuções
					</Link>
					<div className="mt-1.5 flex items-center gap-2.5">
						<span className="flex size-7 items-center justify-center rounded-lg border border-brand-500/30 bg-brand-500/15 text-brand-400 backdrop-blur-sm">
							<IconLayers className="size-4" />
						</span>
						<h1 className="font-display text-lg font-bold text-white tracking-tight">
							Comparação de Runs
						</h1>
						<span className="rounded-full border border-white/10 bg-white/5 px-2 py-0.5 font-mono text-2xs text-zinc-400 backdrop-blur-sm">
							{jobs.length} job{jobs.length === 1 ? "" : "s"}
						</span>
					</div>
				</div>
			</div>

			{loadingJobs ? (
				<div className="flex items-center justify-center gap-2 py-16 font-mono text-xs text-zinc-400">
					<Spinner className="size-4" />
					Carregando jobs…
				</div>
			) : jobIds.length < COMPARE_MIN_JOBS ? (
				<div className="glass-card rounded-xl border border-white/10 p-8 text-center font-mono text-xs text-zinc-400">
					Selecione pelo menos {COMPARE_MIN_JOBS} jobs em{" "}
					<Link href="/jobs" className="text-brand-400 underline underline-offset-2">
						/jobs
					</Link>{" "}
					para comparar.
				</div>
			) : (
				<div className="space-y-5">
					{loadError && (
						<div className="rounded-lg border border-status-alert/30 bg-status-alert/10 px-3 py-2 font-mono text-2xs text-amber-300">
							{loadError}
						</div>
					)}

					{/* Legenda de runs */}
					<div className="flex flex-wrap gap-3">
						{jobs.map((job, i) => (
							<span
								key={job.id}
								className="inline-flex items-center gap-1.5 rounded-full border border-white/10 bg-white/[0.04] px-2.5 py-1 font-mono text-2xs text-zinc-300"
							>
								<span
									className="size-2 shrink-0 rounded-full"
									style={{ backgroundColor: runColor(i) }}
									aria-hidden="true"
								/>
								{job.model} #{job.id.slice(0, 8)}
							</span>
						))}
					</div>

					{/* Controles do gráfico */}
					<div className="glass-card flex flex-wrap items-center gap-4 rounded-xl border border-white/10 p-3.5">
						<div className="w-48">
							<Select
								label="Métrica"
								value={activeMetricKey}
								onChange={(v) => setMetricKey(v)}
								options={metricOptions}
								size="sm"
								fontMono
							/>
						</div>
						<div className="flex items-center rounded-lg border border-white/10 bg-black/40 p-0.5">
							{(["global_step", "epoch"] as const).map((mode) => (
								<button
									key={mode}
									type="button"
									onClick={() => setXMode(mode)}
									className={`min-h-[32px] px-2.5 py-1 font-mono text-2xs rounded-md transition cursor-pointer focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-500/70 ${
										xMode === mode
											? "bg-zinc-800 text-zinc-100 shadow-sm font-semibold"
											: "text-zinc-400 hover:text-zinc-200"
									}`}
								>
									{mode === "epoch" ? "Época" : "Step Global"}
								</button>
							))}
						</div>
						<div className="w-56">
							<Slider
								label="Suavização EMA"
								value={emaSmoothing}
								onChange={setEmaSmoothing}
								min={0}
								max={0.95}
								step={0.05}
								formatValue={(v) => v.toFixed(2)}
							/>
						</div>
						{metricsLoading && (
							<span className="flex items-center gap-1.5 font-mono text-2xs text-zinc-400">
								<Spinner className="size-3" />
								Carregando métricas…
							</span>
						)}
					</div>

					<ComparisonChart
						jobs={jobs}
						pointsByJob={pointsByJob}
						metricKey={activeMetricKey}
						xMode={xMode}
						emaSmoothing={emaSmoothing}
					/>

					<div>
						<h2 className="mb-2 font-mono text-2xs font-semibold uppercase tracking-caps text-zinc-400">
							Hiperparâmetros
						</h2>
						<ComparisonTable jobs={jobs} />
					</div>
				</div>
			)}
		</div>
	);
}

export default function ComparePage() {
	return (
		<Suspense
			fallback={
				<div className="flex items-center justify-center gap-2 py-16 font-mono text-xs text-zinc-400">
					<Spinner className="size-4" />
					Carregando…
				</div>
			}
		>
			<CompareContent />
		</Suspense>
	);
}
