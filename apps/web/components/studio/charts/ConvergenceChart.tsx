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
	totalEpochs?: number;
	isJobActive?: boolean;
	className?: string;
}

// Cores do tema Dark Arcane (DESIGN.md)
const SERIES_PALETTE: Record<
	string,
	{ stroke: string; label: string; axis: "y" | "lr" }
> = {
	loss: { stroke: "#a78bfa", label: "Loss", axis: "y" },
	box_loss: { stroke: "#a78bfa", label: "Box Loss", axis: "y" },
	boxLoss: { stroke: "#a78bfa", label: "Box Loss", axis: "y" },
	cls_loss: { stroke: "#c084fc", label: "Cls Loss", axis: "y" },
	clsLoss: { stroke: "#c084fc", label: "Cls Loss", axis: "y" },
	dfl_loss: { stroke: "#e879f9", label: "DFL Loss", axis: "y" },
	dflLoss: { stroke: "#e879f9", label: "DFL Loss", axis: "y" },
	map50: { stroke: "#34d399", label: "mAP50", axis: "y" },
	mAP50: { stroke: "#34d399", label: "mAP50", axis: "y" },
	map5095: { stroke: "#2dd4bf", label: "mAP50-95", axis: "y" },
	"mAP50-95": { stroke: "#2dd4bf", label: "mAP50-95", axis: "y" },
	grad_norm: { stroke: "#f59e0b", label: "Grad Norm", axis: "y" },
	gradNorm: { stroke: "#f59e0b", label: "Grad Norm", axis: "y" },
	lr: { stroke: "#38bdf8", label: "LR", axis: "lr" },
};

function formatNum(v: number | null | undefined): string {
	if (v == null || !Number.isFinite(v)) return "—";
	if (Math.abs(v) < 0.001 && v !== 0) return v.toExponential(2);
	return v.toFixed(4);
}

export function ConvergenceChart({
	points: rawPoints,
	metrics: legacyMetrics,
	isJobActive = false,
	className = "",
}: ConvergenceChartProps) {
	const chartId = useId();

	// 1. Normaliza dados de entrada: se não recebeu points brutos mas recebeu legacyMetrics, converte
	const allPoints: MetricPointWithKey[] = useMemo(() => {
		if (rawPoints && rawPoints.length > 0) return rawPoints;
		if (!legacyMetrics || legacyMetrics.length === 0) return [];

		const converted: MetricPointWithKey[] = [];
		let seqCounter = 1;
		for (const m of legacyMetrics) {
			const ep = m.epoch;
			const st = m.step ?? 0;
			const ts = new Date().toISOString();

			if (m.loss != null && Number.isFinite(m.loss)) {
				converted.push({
					seq: seqCounter++,
					epoch: ep,
					step: st,
					key: "loss",
					value: m.loss,
					ts,
				});
			}
			if (m.lr != null && Number.isFinite(m.lr)) {
				converted.push({
					seq: seqCounter++,
					epoch: ep,
					step: st,
					key: "lr",
					value: m.lr,
					ts,
				});
			}
			if (m.boxLoss != null && Number.isFinite(m.boxLoss)) {
				converted.push({
					seq: seqCounter++,
					epoch: ep,
					step: st,
					key: "box_loss",
					value: m.boxLoss,
					ts,
				});
			}
			if (m.clsLoss != null && Number.isFinite(m.clsLoss)) {
				converted.push({
					seq: seqCounter++,
					epoch: ep,
					step: st,
					key: "cls_loss",
					value: m.clsLoss,
					ts,
				});
			}
			if (m.dflLoss != null && Number.isFinite(m.dflLoss)) {
				converted.push({
					seq: seqCounter++,
					epoch: ep,
					step: st,
					key: "dfl_loss",
					value: m.dflLoss,
					ts,
				});
			}
			if (m.map50 != null && Number.isFinite(m.map50)) {
				converted.push({
					seq: seqCounter++,
					epoch: ep,
					step: st,
					key: "mAP50",
					value: m.map50,
					ts,
				});
			}
			if (m.map5095 != null && Number.isFinite(m.map5095)) {
				converted.push({
					seq: seqCounter++,
					epoch: ep,
					step: st,
					key: "mAP50-95",
					value: m.map5095,
					ts,
				});
			}
			if (m.gradNorm != null && Number.isFinite(m.gradNorm)) {
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
		return converted;
	}, [rawPoints, legacyMetrics]);

	// Controles do usuário
	const [xMode, setXMode] = useState<XAxisMode>("global_step");
	const [emaSmoothing, setEmaSmoothing] = useState<number>(0.6); // default EMA 0.6
	const [logScaleY, setLogScaleY] = useState<boolean>(false);
	const [hoveredIdx, setHoveredIdx] = useState<number | null>(null);

	const uPlotInstanceRef = useRef<uPlot | null>(null);

	// Pivota os pontos na matriz de uPlot
	const pivoted = useMemo(() => {
		return pivotMetricPoints(allPoints, xMode);
	}, [allPoints, xMode]);

	// Identifica quais séries existem nos dados
	const activeKeys = pivoted.seriesKeys;
	const hasLr = activeKeys.includes("lr");

	// Séries a plotar: para cada chave numérica, podemos ter série bruta (esmaecida) + suavizada (sólida) se EMA > 0
	const seriesConfig = useMemo(() => {
		const list: Array<{
			key: string;
			label: string;
			stroke: string;
			isEma: boolean;
			axis: "y" | "lr";
		}> = [];

		for (const key of activeKeys) {
			const meta = SERIES_PALETTE[key] || {
				stroke: "#9a92a6",
				label: key,
				axis: key === "lr" ? "lr" : "y",
			};

			if (emaSmoothing > 0 && key !== "lr") {
				// Bruta esmaecida
				list.push({
					key: `${key}_raw`,
					label: `${meta.label} (raw)`,
					stroke: `${meta.stroke}44`, // 25% opacidade
					isEma: false,
					axis: meta.axis,
				});
				// Suavizada
				list.push({
					key: `${key}_ema`,
					label: `${meta.label} (EMA)`,
					stroke: meta.stroke,
					isEma: true,
					axis: meta.axis,
				});
			} else {
				// Apenas a série normal
				list.push({
					key,
					label: meta.label,
					stroke: meta.stroke,
					isEma: false,
					axis: meta.axis,
				});
			}
		}

		return list;
	}, [activeKeys, emaSmoothing]);

	// Matriz de dados para o uPlot: [xValues, ...seriesValues]
	const uPlotData = useMemo(() => {
		if (pivoted.xValues.length === 0) {
			return [[]] as uPlot.AlignedData;
		}

		const dataRows: (number | null)[][] = [pivoted.xValues];

		for (const s of seriesConfig) {
			const baseKey = s.key.replace(/_raw$/, "").replace(/_ema$/, "");
			const rawValues = pivoted.seriesData[baseKey] ?? [];

			if (s.isEma) {
				dataRows.push(calculateEMA(rawValues, emaSmoothing));
			} else {
				dataRows.push(rawValues);
			}
		}

		return dataRows as uPlot.AlignedData;
	}, [pivoted, seriesConfig, emaSmoothing]);

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
							return `${d.getHours().toString().padStart(2, "0")}:${d.getMinutes().toString().padStart(2, "0")}:${d.getSeconds().toString().padStart(2, "0")}`;
						}
						if (xMode === "epoch") {
							return `Ep ${Math.round(val)}`;
						}
						return `#${Math.round(val)}`;
					});
				},
			},
			// Eixo Y Principal (Loss, mAP, Grad Norm)
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

		if (hasLr) {
			// Eixo Y Secundário à direita para LR
			axes.push({
				scale: "lr",
				side: 1, // Right
				stroke: "#38bdf8",
				grid: { show: false },
				ticks: { stroke: "#38bdf8", width: 1 },
				values: (_self, splits) => splits.map((v) => v.toExponential(1)),
			});
		}

		const seriesOpts: uPlot.Series[] = [
			// X Series
			{
				label: xMode === "epoch" ? "Época" : xMode === "ts" ? "Tempo" : "Step",
			},
		];

		for (const s of seriesConfig) {
			seriesOpts.push({
				label: s.label,
				scale: s.axis,
				stroke: s.stroke,
				width: s.isEma || (!emaSmoothing && s.key !== "lr") ? 2 : 1,
				spanGaps: true,
				points: { show: false },
			});
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
					distr: logScaleY ? 3 : 1, // 3: Logarithmic, 1: Linear
				},
				lr: {
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
				show: true,
			},
			hooks: {
				setCursor: [
					(u) => {
						setHoveredIdx(u.cursor.idx ?? null);
					},
				],
			},
		};
	}, [xMode, hasLr, seriesConfig, emaSmoothing, logScaleY]);

	// Duplo clique para resetar zoom
	const handleResetZoom = useCallback(() => {
		const chart = uPlotInstanceRef.current;
		if (!chart || pivoted.xValues.length === 0) return;

		chart.batch(() => {
			const minX = pivoted.xValues[0];
			const maxX = pivoted.xValues[pivoted.xValues.length - 1];
			chart.setScale("x", { min: minX, max: maxX });

			// Reseta escala Y para auto-range
			chart.setScale("y", { min: 0, max: 1 });
			chart.setData(chart.data, true);
		});
	}, [pivoted.xValues]);

	// Se não houver dados
	if (allPoints.length === 0) {
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

	// Resumo textual para acessibilidade (screen reader)
	const latestIndex =
		hoveredIdx != null ? hoveredIdx : pivoted.xValues.length - 1;
	const currentSample =
		pivoted.xIndexMap[latestIndex] ??
		pivoted.xIndexMap[pivoted.xIndexMap.length - 1];
	const lastLoss =
		pivoted.seriesData.loss?.[latestIndex] ??
		pivoted.seriesData.box_loss?.[latestIndex];
	const lastLr = pivoted.seriesData.lr?.[latestIndex];

	return (
		<div
			className={`glass-card rounded-xl shadow-lg p-4 space-y-3.5 border border-white/10 bg-zinc-900/40 backdrop-blur-md ${className}`}
		>
			{/* Sumário para leitores de tela */}
			<div className="sr-only" aria-live="polite">
				Gráfico de convergência: época {currentSample?.epoch ?? "N/A"}, step{" "}
				{currentSample?.step ?? "N/A"}.
				{lastLoss != null ? ` Loss atual: ${lastLoss.toFixed(4)}.` : ""}
				{lastLr != null
					? ` Taxa de aprendizado: ${lastLr.toExponential(2)}.`
					: ""}
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
								Curvas de Convergência (uPlot)
							</span>
							{isJobActive && (
								<span className="flex items-center gap-1 font-mono text-3xs text-brand-400 bg-brand-500/10 border border-brand-500/20 px-1.5 py-0.5 rounded-full">
									<span className="size-1.5 rounded-full bg-brand-400 animate-pulse" />
									Live
								</span>
							)}
						</div>
						<p className="font-mono text-3xs text-zinc-400">
							{pivoted.xValues.length} checkpoints • Arraste para zoom • Duplo
							clique reseta
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
				/>
			</section>

			{/* Tooltip Dinâmico / Crosshair Values Card */}
			<div className="flex flex-wrap items-center justify-between gap-3 pt-2 border-t border-white/5 font-mono text-2xs">
				<div className="text-zinc-400 flex items-center gap-1.5">
					<span>Ponto focado:</span>
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

				<div className="flex flex-wrap items-center gap-3">
					{activeKeys.map((key) => {
						const meta = SERIES_PALETTE[key] || {
							stroke: "#9a92a6",
							label: key,
						};
						const val = pivoted.seriesData[key]?.[latestIndex];
						if (val == null) return null;

						return (
							<div key={key} className="flex items-center gap-1.5">
								<span
									className="size-2 rounded-full"
									style={{ backgroundColor: meta.stroke }}
								/>
								<span className="text-zinc-400">{meta.label}:</span>
								<span className="font-semibold text-zinc-100">
									{formatNum(val)}
								</span>
							</div>
						);
					})}
				</div>
			</div>
		</div>
	);
}
