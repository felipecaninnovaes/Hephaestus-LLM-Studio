"use client";

import { useEffect, useRef } from "react";
import { createPortal } from "react-dom";
import { useFloatingPosition } from "@/hooks/useFloatingPosition";
import type { ComfyTarget } from "@/types/comfyui";

interface ComfyTargetMenuProps {
	anchorRef: React.RefObject<HTMLElement | null>;
	targets: ComfyTarget[];
	onSelect: (t: ComfyTarget) => void;
	onClose: () => void;
}

/** Menu flutuante (portal) de escolha do destino ComfyUI. */
export function ComfyTargetMenu({
	anchorRef,
	targets,
	onSelect,
	onClose,
}: ComfyTargetMenuProps) {
	const menuRef = useRef<HTMLDivElement>(null);
	const { coords } = useFloatingPosition({
		anchorRef,
		isOpen: true,
		align: "auto",
		menuWidth: "auto",
		minMenuHeight: 160,
	});

	useEffect(() => {
		menuRef.current?.querySelector<HTMLElement>("[role=menuitem]")?.focus();
		function onKey(e: KeyboardEvent) {
			if (e.key === "Escape") {
				onClose();
				anchorRef.current?.focus();
			}
		}
		function onDown(e: MouseEvent) {
			const t = e.target as Node;
			if (menuRef.current?.contains(t) || anchorRef.current?.contains(t))
				return;
			onClose();
		}
		window.addEventListener("keydown", onKey);
		document.addEventListener("mousedown", onDown);
		return () => {
			window.removeEventListener("keydown", onKey);
			document.removeEventListener("mousedown", onDown);
		};
	}, [anchorRef, onClose]);

	if (!coords) return null;

	return createPortal(
		<div
			ref={menuRef}
			role="menu"
			aria-label="Destino ComfyUI"
			style={{
				position: "fixed",
				top: coords.top,
				bottom: coords.bottom,
				left: coords.left,
				right: coords.right,
				zIndex: 500,
			}}
			className="glass-menu min-w-56 max-w-80 rounded-xl p-1.5 shadow-2xl"
		>
			<div className="px-2.5 pb-1 pt-1 font-mono text-3xs uppercase tracking-caps text-zinc-400">
				Enviar para
			</div>
			{targets.map((t) => (
				<button
					key={t.id}
					type="button"
					role="menuitem"
					onClick={() => onSelect(t)}
					className="flex w-full cursor-pointer flex-col rounded-lg px-2.5 py-1.5 text-left transition hover:bg-white/[0.06] focus-visible:bg-white/[0.06] focus-visible:outline-none"
				>
					<span className="truncate text-xs font-medium text-zinc-100">
						{t.name}
					</span>
					<span className="truncate font-mono text-2xs text-zinc-400">
						{t.baseUrl}
					</span>
				</button>
			))}
		</div>,
		document.body,
	);
}
