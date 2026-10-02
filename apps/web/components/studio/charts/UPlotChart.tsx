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
}: UPlotChartProps) {
	const containerRef = useRef<HTMLDivElement | null>(null);
	const chartRef = useRef<uPlot | null>(null);
	const onInitRef = useRef(onInit);
	onInitRef.current = onInit;
	const onDestroyRef = useRef(onDestroy);
	onDestroyRef.current = onDestroy;
	const initialDataRef = useRef(data);
	// Cria / recria o uPlot quando as options estruturais mudarem
	useEffect(() => {
		const el = containerRef.current;
		if (!el) return;

		// Mede tamanho real do container se options width/height não forem rígidos
		const rect = el.getBoundingClientRect();
		const width = Math.max(Math.floor(rect.width) || options.width || 600, 100);
		const height = Math.max(Math.floor(options.height) || 240, 100);

		const mergedOpts: uPlot.Options = {
			...options,
			width,
			height,
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
		// setData atualiza os caminhos Canvas diretamente em sub-milissegundos
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
