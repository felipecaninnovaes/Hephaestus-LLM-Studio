"use client";

import { useEffect, useRef } from "react";
import uPlot from "uplot";
import "uplot/dist/uPlot.min.css";

export interface UPlotChartProps {
	options: uPlot.Options;
	data: uPlot.AlignedData;
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
	className,
	onInit,
	onDestroy,
	onFirstDraw,
}: UPlotChartProps) {
	const containerRef = useRef<HTMLDivElement | null>(null);
	const chartRef = useRef<uPlot | null>(null);
	const onInitRef = useRef(onInit);
	onInitRef.current = onInit;
	const onDestroyRef = useRef(onDestroy);
	onDestroyRef.current = onDestroy;
	const onFirstDrawRef = useRef(onFirstDraw);
	onFirstDrawRef.current = onFirstDraw;

	const initialDataRef = useRef(data);
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

		const chart = new uPlot(mergedOpts, initialDataRef.current, el);
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

	// Atualiza dados de forma imperativa e eficiente sem recriar o gráfico
	useEffect(() => {
		const chart = chartRef.current;
		if (!chart) return;
		chart.setData(data, false);
	}, [data]);

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
