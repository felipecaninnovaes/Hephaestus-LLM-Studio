"use client";

import { useCallback, useId, useMemo, useRef, useState } from "react";
import type uPlot from "uplot";
import { IconActivity, IconTrendingUp } from "@/components/icons";
import {
	calculateEMA,
	pivotMetricPoints,
	type XAxisMode,
} from "@/lib/metricMath";
import type { MetricPointWithKey } from "@/types/jobs";
import { UPlotChart } from "./UPlotChart";

export interface ConvergenceChartProps {
	/** Pontos brutos de métricas com key (fatia 1a/4a) */
	points?: MetricPointWithKey[];
	/** Retrocompatibilidade com array legado pivotado por época */
	metrics?: Array<{
		epoch: number;
		step?: number;
		loss?: number;
		lr?: number;
		boxLoss?: number;
		clsLoss?: number;
		dflLoss?: number;
		map50?: number;
		map5095?: number;
		gradNorm?: number;
		phase?: string;
	}>;
	jobKind?: string;
	totalEpochs?: number;
	isJobActive?: boolean;
	className?: string;
	onRenderMeasurement?: (durationMs: number) => void;
}

// Cores do tema Dark Arcane (DESIGN.md)
const SERIES_PALETTE: Record<
	string,
	{ stroke: string; label: string; axis: "y" | "right" }
> = {
	loss: { stroke: "#a78bfa", label: "Loss", axis: "y" },
	box_loss: { stroke: "#a78bfa", label: "Box Loss", axis: "y" },
	boxLoss: { stroke: "#a78bfa", label: "Box Loss", axis: "y" },
	cls_loss: { stroke: "#c084fc", label: "Cls Loss", axis: "y" },
	clsLoss: { stroke: "#c084fc", label: "Cls Loss", axis: "y" },
	dfl_loss: { stroke: "#e879f9", label: "DFL Loss", axis: "y" },
	dflLoss: { stroke: "#e879f9", label: "DFL Loss", axis: "y" },
	map50: { stroke: "#34d399", label: "mAP50", axis: "right" },
	mAP50: { stroke: "#34d399", label: "mAP50", axis: "right" },
	map5095: { stroke: "#2dd4bf", label: "mAP50-95", axis: "right" },
	"mAP50-95": { stroke: "#2dd4bf", label: "mAP50-95", axis: "right" },
	grad_norm: { stroke: "#f59e0b", label: "Grad Norm", axis: "y" },
	gradNorm: { stroke: "#f59e0b", label: "Grad Norm", axis: "y" },
	lr: { stroke: "#38bdf8", label: "LR", axis: "right" },
};

function formatNum(v: number | null | undefined): string {
	if (v == null || !Number.isFinite(v)) return "—";
	if (Math.abs(v) < 0.001 && v !== 0) return v.toExponential(2);
	return v.toFixed(4);
}

export function ConvergenceChart({
	points: rawPoints,
	metrics: legacyMetrics,
	jobKind,
	isJobActive = false,
	className = "",
	onRenderMeasurement,
}: ConvergenceChartProps) {
	const chartId = useId();

	// 1. Normaliza dados de entrada: se não recebeu points brutos mas recebeu legacyMetrics, converte
	const allPoints: MetricPointWithKey[] = useMemo(() => {
		if (rawPoints && rawPoints.length > 0) return rawPoints;
		if (!legacyMetrics || legacyMetrics.length === 0) return [];

		const isYolo = jobKind?.startsWith("yolo");
		const converted: MetricPointWithKey[] = [];
		let seqCounter = 1;

		for (const m of legacyMetrics) {
			const ep = m.epoch;
			const st = m.step ?? ep;
			const ts = new Date().toISOString();

			if (isYolo) {
				if (
					m.boxLoss != null &&
					Number.isFinite(m.boxLoss) &&
					m.boxLoss !== 0
				) {
					converted.push({
						seq: seqCounter++,
						epoch: ep,
						step: st,
						key: "box_loss",
						value: m.boxLoss,
						ts,
					});
				}
				if (
					m.clsLoss != null &&
					Number.isFinite(m.clsLoss) &&
					m.clsLoss !== 0
				) {
					converted.push({
						seq: seqCounter++,
						epoch: ep,
						step: st,
						key: "cls_loss",
						value: m.clsLoss,
						ts,
					});
				}
				if (
					m.dflLoss != null &&
					Number.isFinite(m.dflLoss) &&
					m.dflLoss !== 0
				) {
					converted.push({
						seq: seqCounter++,
						epoch: ep,
						step: st,
						key: "dfl_loss",
						value: m.dflLoss,
						ts,
					});
				}
				if (m.map50 != null && Number.isFinite(m.map50) && m.map50 !== 0) {
					converted.push({
						seq: seqCounter++,
						epoch: ep,
						step: st,
						key: "mAP50",
						value: m.map50,
						ts,
					});
				}
				if (
					m.map5095 != null &&
					Number.isFinite(m.map5095) &&
					m.map5095 !== 0
				) {
					converted.push({
						seq: seqCounter++,
						epoch: ep,
						step: st,
						key: "mAP50-95",
						value: m.map5095,
						ts,
					});
				}
			} else {
				// Difusão
				if (m.loss != null && Number.isFinite(m.loss) && m.loss !== 0) {
					converted.push({
						seq: seqCounter++,
						epoch: ep,
						step: st,
						key: "loss",
						value: m.loss,
						ts,
					});
				}
				if (m.lr != null && Number.isFinite(m.lr) && m.lr !== 0) {
					converted.push({
						seq: seqCounter++,
						epoch: ep,
						step: st,
						key: "lr",
						value: m.lr,
						ts,
					});
				}
				if (
					m.gradNorm != null &&
					Number.isFinite(m.gradNorm) &&
					m.gradNorm !== 0
				) {
					converted.push({
						seq: seqCounter++,
						epoch: ep,
						step: st,
						key: "grad_norm",
						value: m.gradNorm,
						ts,
					});
				}
			}
		}
		return converted;
	}, [rawPoints, legacyMetrics, jobKind]);

	// Controles do usuário
	const [xMode, setXMode] = useState<XAxisMode>("global_step");
	const [emaSmoothing, setEmaSmoothing] = useState<number>(0.6);
	const [logScaleY, setLogScaleY] = useState<boolean>(false);
	const [hoveredIdx, setHoveredIdx] = useState<number | null>(null);

	// Estado para toggle individual de séries (visibilidade)
	const [hiddenSeries, setHiddenSeries] = useState<Record<string, boolean>>({});

	const uPlotInstanceRef = useRef<uPlot | null>(null);

	// Pivota pontos na matriz de uPlot com filtro rígido
	const pivoted = useMemo(() => {
		return pivotMetricPoints(allPoints, xMode, { jobKind });
	}, [allPoints, xMode, jobKind]);

	const activeKeys = pivoted.seriesKeys;

	// Verifica se há métricas no eixo secundário (LR para difusão ou mAP para YOLO)
	const hasRightAxis = useMemo(() => {
		return activeKeys.some((k) => SERIES_PALETTE[k]?.axis === "right");
	}, [activeKeys]);


	// Matriz de dados para o uPlot:
	// Para cada chave ativa K:
	//  - série K_raw (bruta esmaecida)
	//  - série K_ema (suavizada com EMA) se emaSmoothing > 0
	const uPlotData = useMemo(() => {
		if (pivoted.xValues.length === 0) {
			return [[]] as uPlot.AlignedData;
		}

		const dataRows: (number | null)[][] = [pivoted.xValues];

		for (const key of activeKeys) {
			const rawValues = pivoted.seriesData[key] ?? [];
			// Se EMA > 0 e a chave não for LR (LR já é suave e em escala logarítmica)
			if (emaSmoothing > 0 && key !== "lr") {
				dataRows.push(rawValues); // raw
				dataRows.push(calculateEMA(rawValues, emaSmoothing)); // ema
			} else {
				dataRows.push(rawValues);
			}
		}

		return dataRows as uPlot.AlignedData;
	}, [pivoted, activeKeys, emaSmoothing]);

	// Construção das Options do uPlot
	const uPlotOptions = useMemo<uPlot.Options>(() => {
		const axes: uPlot.Axis[] = [
			// Eixo X
			{
				scale: "x",
				stroke: "#756d82",
				grid: { stroke: "rgba(131, 80, 242, 0.08)", width: 1 },
				ticks: { stroke: "#3e3749", width: 1 },
				values: (_self, splits) => {
					return splits.map((val) => {
						if (xMode === "ts") {
							const d = new Date(val * 1000);
							return d.toLocaleTimeString([], {
								hour: "2-digit",
								minute: "2-digit",
								second: "2-digit",
							});
						}
						if (xMode === "epoch") {
							return `${Math.round(val)}`;
						}
						return `${Math.round(val)}`;
					});
				},
			},
			// Eixo Y Principal
			{
				scale: "y",
				stroke: "#9a92a6",
				grid: { stroke: "rgba(131, 80, 242, 0.08)", width: 1 },
				ticks: { stroke: "#3e3749", width: 1 },
				values: (_self, splits) =>
					splits.map((v) =>
						v < 0.01 && v > 0 ? v.toExponential(1) : v.toFixed(3),
					),
			},
		];

		if (hasRightAxis) {
			axes.push({
				scale: "right",
				side: 1, // Right
				stroke: "#38bdf8",
				grid: { show: false },
				ticks: { stroke: "#38bdf8", width: 1 },
				values: (_self, splits) =>
					splits.map((v) =>
						v < 0.01 && v > 0 ? v.toExponential(1) : v.toFixed(2),
					),
			});
		}

		const seriesOpts: uPlot.Series[] = [
			// Eixo X
			{
				label:
					xMode === "epoch"
						? "Época"
						: xMode === "ts"
							? "Tempo"
							: "Step Global",
			},
		];

		for (const key of activeKeys) {
			const meta = SERIES_PALETTE[key] || {
				stroke: "#9a92a6",
				label: key,
				axis: "y" as const,
			};
			const isHidden = !!hiddenSeries[key];

			if (emaSmoothing > 0 && key !== "lr") {
				// 1. Raw (esmaecido)
				seriesOpts.push({
					label: `${meta.label} (raw)`,
					scale: meta.axis,
					stroke: `${meta.stroke}33`, // 20% opacidade
					width: 1,
					show: !isHidden,
					spanGaps: true,
					points: { show: false },
				});
				// 2. EMA (sólido)
				seriesOpts.push({
					label: meta.label,
					scale: meta.axis,
					stroke: meta.stroke,
					width: 2,
					show: !isHidden,
					spanGaps: true,
					points: { show: false },
				});
			} else {
				// Normal
				seriesOpts.push({
					label: meta.label,
					scale: meta.axis,
					stroke: meta.stroke,
					width: 2,
					show: !isHidden,
					spanGaps: true,
					points: { show: false },
				});
			}
		}

		return {
			width: 600,
			height: 240,
			scales: {
				x: {
					time: xMode === "ts",
				},
				y: {
					auto: true,
					distr: logScaleY ? 3 : 1,
				},
				right: {
					auto: true,
				},
			},
			axes,
			series: seriesOpts,
			cursor: {
				drag: {
					setScale: true,
					x: true,
					y: true,
				},
				focus: {
					prox: 30,
				},
			},
			legend: {
				show: false, // Desliga a nativa para usar nossa legenda limpa e unificada
			},
			hooks: {
				setCursor: [
					(u) => {
						setHoveredIdx(u.cursor.idx ?? null);
					},
				],
			},
		};
	}, [xMode, hasRightAxis, activeKeys, emaSmoothing, logScaleY, hiddenSeries]);

	// Toggle sincronizado de série (liga/desliga raw + EMA juntos)
	const toggleSeries = useCallback((key: string) => {
		setHiddenSeries((prev) => ({
			...prev,
			[key]: !prev[key],
		}));
	}, []);

	// Duplo clique ou botão para resetar zoom
	const handleResetZoom = useCallback(() => {
		const chart = uPlotInstanceRef.current;
		if (!chart || pivoted.xValues.length === 0) return;

		chart.batch(() => {
			const minX = pivoted.xValues[0];
			const maxX = pivoted.xValues[pivoted.xValues.length - 1];
			chart.setScale("x", { min: minX, max: maxX });
			chart.setData(chart.data, true);
		});
	}, [pivoted.xValues]);

	// Se não houver dados
	if (allPoints.length === 0 || activeKeys.length === 0) {
		return (
			<div
				className={`rounded-xl border border-zinc-800 bg-black/40 backdrop-blur-sm p-6 text-center ${className}`}
			>
				<div className="mx-auto flex size-10 items-center justify-center rounded-lg border border-zinc-800 bg-zinc-900/60 backdrop-blur-sm text-zinc-400">
					<IconActivity className="size-5" />
				</div>
				<h4 className="mt-3 font-display text-sm font-semibold text-zinc-200">
					Aguardando Telemetria de Treino
				</h4>
				<p className="mt-1 font-mono text-2xs text-zinc-400">
					O orquestrador transmitirá as curvas de loss e métricas em tempo real.
				</p>
			</div>
		);
	}

	const latestIndex =
		hoveredIdx != null ? hoveredIdx : pivoted.xValues.length - 1;
	const currentSample =
		pivoted.xIndexMap[latestIndex] ??
		pivoted.xIndexMap[pivoted.xIndexMap.length - 1];

	const totalPointsCount = pivoted.xValues.length;

	return (
		<div
			className={`glass-card rounded-xl shadow-lg p-4 space-y-3.5 border border-white/10 bg-zinc-900/40 backdrop-blur-md ${className}`}
		>
			{/* Sumário textual para screen readers */}
			<div className="sr-only" aria-live="polite">
				Gráfico de convergência: época {currentSample?.epoch ?? "N/A"}, step{" "}
				{currentSample?.step ?? "N/A"}. Total de {totalPointsCount} pontos.
			</div>

			{/* Header com Controles Responsivos e Acessíveis */}
			<div className="flex flex-wrap items-center justify-between gap-3 border-b border-white/5 pb-2.5">
				<div className="flex items-center gap-2">
					<div className="flex size-7 shrink-0 items-center justify-center rounded-md bg-brand-500/10 text-brand-400 border border-brand-500/20">
						<IconTrendingUp className="size-4" />
					</div>
					<div>
						<div className="flex items-center gap-2">
							<span className="font-mono text-2xs font-semibold tracking-caps uppercase text-zinc-200">
								Curvas de Convergência
							</span>
							{isJobActive && (
								<span className="flex items-center gap-1 font-mono text-3xs text-brand-400 bg-brand-500/10 border border-brand-500/20 px-1.5 py-0.5 rounded-full">
									<span className="size-1.5 rounded-full bg-brand-400 animate-pulse" />
									Live
								</span>
							)}
						</div>
						<p className="font-mono text-3xs text-zinc-400">
							{totalPointsCount} pontos • Arraste para zoom • Duplo clique
							reseta
						</p>
					</div>
				</div>

				{/* Toolbar de Controles */}
				<div className="flex flex-wrap items-center gap-2.5">
					{/* Alternância do Eixo X */}
					<div className="flex items-center rounded-lg border border-white/10 bg-black/40 p-0.5">
						<label htmlFor={`${chartId}-xaxis`} className="sr-only">
							Eixo X
						</label>
						{(["global_step", "epoch", "ts"] as const).map((mode) => (
							<button
								key={mode}
								type="button"
								onClick={() => setXMode(mode)}
								className={`min-h-[32px] px-2.5 py-1 font-mono text-2xs rounded-md transition cursor-pointer focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-500/70 ${
									xMode === mode
										? "bg-zinc-800 text-zinc-100 shadow-sm font-semibold"
										: "text-zinc-400 hover:text-zinc-200"
								}`}
								aria-pressed={xMode === mode}
							>
								{mode === "global_step"
									? "Step"
									: mode === "epoch"
										? "Época"
										: "Tempo"}
							</button>
						))}
					</div>

					{/* Slider EMA */}
					<div className="flex items-center gap-2 rounded-lg border border-white/10 bg-black/40 px-2.5 py-1 min-h-[32px]">
						<label
							htmlFor={`${chartId}-ema`}
							className="font-mono text-2xs text-zinc-400 cursor-pointer select-none"
						>
							EMA:{" "}
							<span className="text-zinc-200 font-semibold">
								{emaSmoothing.toFixed(2)}
							</span>
						</label>
						<input
							id={`${chartId}-ema`}
							type="range"
							min="0"
							max="0.95"
							step="0.05"
							value={emaSmoothing}
							onChange={(e) => setEmaSmoothing(parseFloat(e.target.value))}
							className="w-16 h-1 bg-zinc-700 rounded-lg appearance-none cursor-pointer accent-brand-500 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-500"
							aria-label="Ajustar suavização EMA"
						/>
					</div>

					{/* Toggle Escala Log Y */}
					<button
						type="button"
						onClick={() => setLogScaleY((v) => !v)}
						className={`min-h-[32px] px-2.5 py-1 font-mono text-2xs rounded-lg border transition cursor-pointer focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-500/70 ${
							logScaleY
								? "bg-brand-500/20 border-brand-500/40 text-brand-300 font-semibold"
								: "border-white/10 bg-black/40 text-zinc-400 hover:text-zinc-200"
						}`}
						aria-pressed={logScaleY}
					>
						Log Y
					</button>

					{/* Botão Reset Zoom */}
					<button
						type="button"
						onClick={handleResetZoom}
						className="min-h-[32px] px-2.5 py-1 font-mono text-2xs rounded-lg border border-white/10 bg-black/40 text-zinc-400 hover:text-zinc-200 hover:border-white/20 transition cursor-pointer focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-500/70"
						title="Duplo clique no gráfico também reseta"
					>
						Reset
					</button>
				</div>
			</div>

			{/* Container uPlot com duplo clique para reset de zoom */}
			<section
				aria-label="Área de plotagem interativa (arraste para zoom, duplo clique para resetar)"
				onDoubleClick={handleResetZoom}
				className="w-full"
			>
				<UPlotChart
					options={uPlotOptions}
					data={uPlotData}
					onInit={(u) => {
						uPlotInstanceRef.current = u;
					}}
					onDestroy={() => {
						uPlotInstanceRef.current = null;
					}}
					onFirstDraw={onRenderMeasurement}
				/>
			</section>

			{/* Legenda Limpa e Interativa: 1 entrada por métrica (liga/desliga raw+EMA) + Valor sob cursor */}
			<div className="flex flex-wrap items-center justify-between gap-3 pt-2 border-t border-white/5 font-mono text-2xs">
				<div className="text-zinc-400 flex items-center gap-1.5">
					<span>Ponto:</span>
					<span className="text-zinc-200 font-semibold">
						{currentSample
							? `Época ${currentSample.epoch ?? "—"} • Step ${currentSample.step}`
							: "—"}
					</span>
					{hoveredIdx != null && hoveredIdx !== pivoted.xValues.length - 1 && (
						<span className="text-brand-400 text-3xs px-1.5 py-0.2 rounded bg-brand-500/10 border border-brand-500/20">
							Histórico
						</span>
					)}
				</div>

				<div className="flex flex-wrap items-center gap-2">
					{activeKeys.map((key) => {
						const meta = SERIES_PALETTE[key] || {
							stroke: "#9a92a6",
							label: key,
							axis: "y" as const,
						};
						const val = pivoted.seriesData[key]?.[latestIndex];
						const isHidden = !!hiddenSeries[key];

						return (
							<button
								key={key}
								type="button"
								onClick={() => toggleSeries(key)}
								className={`flex items-center gap-1.5 px-2 py-0.5 rounded border transition cursor-pointer select-none focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-brand-500 ${
									isHidden
										? "opacity-40 border-zinc-800 bg-zinc-900/40 line-through text-zinc-500"
										: "border-white/10 bg-black/40 text-zinc-300 hover:border-white/20"
								}`}
								title="Clique para alternar visibilidade"
							>
								<span
									className="size-2 rounded-full"
									style={{
										backgroundColor: isHidden ? "#585164" : meta.stroke,
									}}
								/>
								<span className="text-zinc-400">{meta.label}:</span>
								<span className="font-semibold text-zinc-100">
									{formatNum(val)}
								</span>
							</button>
						);
					})}
				</div>
			</div>
		</div>
	);
}
