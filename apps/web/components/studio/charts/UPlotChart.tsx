"use client";

import { useEffect, useRef } from "react";
import uPlot from "uplot";
import "uplot/dist/uPlot.min.css";
import { nextXRange, type XRange } from "@/lib/metricMath";

export interface UPlotChartProps {
	options: uPlot.Options;
	data: uPlot.AlignedData;
	/** Range X completo dos dados atuais (auto-follow quando o usuário não está com zoom) */
	xRange?: XRange;
	/** Mudança de valor descarta o zoom (ex.: modo Step / Época / Tempo) */
	resetKey?: string;
	className?: string;
	onInit?: (u: uPlot) => void;
	onDestroy?: () => void;
	/** Callback para medir o tempo do mount até o primeiro draw */
	onFirstDraw?: (durationMs: number) => void;
}

/**
 * Wrapper React fino sobre uPlot (ADR-0024 / Fatia 4a).
 * - SSR safe ("use client")
 * - Lifecycle via useRef + useEffect
 * - ResizeObserver para auto-resize responsivo e rápido
 * - Destroy limpo no unmount
 */
export function UPlotChart({
	options,
	data,
	xRange,
	resetKey,
	className,
	onInit,
	onDestroy,
	onFirstDraw,
}: UPlotChartProps) {
	const containerRef = useRef<HTMLDivElement | null>(null);
	const chartRef = useRef<uPlot | null>(null);
	const dataRef = useRef(data);
	useEffect(() => {
		dataRef.current = data;
	});
	const onInitRef = useRef(onInit);
	onInitRef.current = onInit;
	const onDestroyRef = useRef(onDestroy);
	onDestroyRef.current = onDestroy;
	const onFirstDrawRef = useRef(onFirstDraw);
	onFirstDrawRef.current = onFirstDraw;

	const hasDrawnFirstRef = useRef(false);

	// Cria / recria o uPlot quando as options estruturais mudarem
	useEffect(() => {
		const el = containerRef.current;
		if (!el) return;

		// Marca início da medição de render
		performance.mark("uplot-mount-start");
		hasDrawnFirstRef.current = false;

		const rect = el.getBoundingClientRect();
		const width = Math.max(Math.floor(rect.width) || options.width || 600, 100);
		const height = Math.max(Math.floor(options.height) || 240, 100);

		// Clona hooks para interceptar o primeiro draw
		const userHooks = options.hooks || {};
		const drawHooks = [...(userHooks.draw || [])];

		drawHooks.unshift((_u: uPlot) => {
			if (!hasDrawnFirstRef.current) {
				hasDrawnFirstRef.current = true;
				performance.mark("uplot-draw-end");
				try {
					performance.measure(
						"uplot-initial-render",
						"uplot-mount-start",
						"uplot-draw-end",
					);
					const entries = performance.getEntriesByName("uplot-initial-render");
					const last = entries[entries.length - 1];
					if (last && onFirstDrawRef.current) {
						onFirstDrawRef.current(last.duration);
					}
				} catch {
					// Medição opcional
				} finally {
					performance.clearMarks("uplot-mount-start");
					performance.clearMarks("uplot-draw-end");
					performance.clearMeasures("uplot-initial-render");
				}
			}
		});

		const mergedOpts: uPlot.Options = {
			...options,
			width,
			height,
			hooks: {
				...userHooks,
				draw: drawHooks,
			},
		};

		const chart = new uPlot(mergedOpts, dataRef.current, el);
		chartRef.current = chart;

		if (onInitRef.current) {
			onInitRef.current(chart);
		}

		return () => {
			chart.destroy();
			chartRef.current = null;
			if (onDestroyRef.current) {
				onDestroyRef.current();
			}
		};
	}, [options]);

	// Atualiza dados de forma imperativa. Troca de `resetKey` ou instância nova
	// descarta o zoom; com dados novos, o X acompanha o range completo só se o
	// usuário já estava nele (zoom preservado caso contrário).
	const syncRef = useRef<{ chart: uPlot | null; key?: string; full: XRange | null }>(
		{ chart: null, full: null },
	);
	useEffect(() => {
		const chart = chartRef.current;
		if (!chart) return;
		const prev = syncRef.current;
		syncRef.current = { chart, key: resetKey, full: xRange ?? null };
		if (!xRange) {
			chart.setData(data, false);
			return;
		}
		const { min, max } = chart.scales.x;
		const current = min != null && max != null ? { min, max } : null;
		const reset = prev.chart !== chart || prev.key !== resetKey;
		const target = nextXRange(current, prev.full, xRange, reset);
		if (!target) {
			chart.setData(data, false);
			return;
		}
		chart.batch(() => {
			chart.setData(data, true);
			chart.setScale("x", target);
		});
	}, [data, xRange, resetKey]);

	// ResizeObserver para manter o gráfico esticado na largura do card
	useEffect(() => {
		const el = containerRef.current;
		if (!el) return;

		let rafId: number | null = null;
		const observer = new ResizeObserver((entries) => {
			for (const entry of entries) {
				const w = Math.floor(entry.contentRect.width);
				if (w > 0 && chartRef.current) {
					if (rafId) cancelAnimationFrame(rafId);
					rafId = requestAnimationFrame(() => {
						const curH = chartRef.current?.height ?? (options.height || 240);
						chartRef.current?.setSize({ width: w, height: curH });
					});
				}
			}
		});

		observer.observe(el);

		return () => {
			if (rafId) cancelAnimationFrame(rafId);
			observer.disconnect();
		};
	}, [options.height]);

	return (
		<div
			ref={containerRef}
			className={`relative w-full overflow-hidden select-none ${className ?? ""}`}
		/>
	);
}
