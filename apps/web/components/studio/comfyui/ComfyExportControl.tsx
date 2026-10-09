"use client";

import Link from "next/link";
import { type ReactNode, useCallback, useRef, useState } from "react";
import { IconChevronDown, IconUpload } from "@/components/icons";
import { Button, getButtonClasses } from "@/components/ui/Button";
import { useComfyExport } from "@/hooks/useComfyExport";
import { useComfyTargets } from "@/hooks/useComfyTargets";
import { chooseExportTarget } from "@/lib/comfyExport";
import type { ComfyExportSource, ComfyTarget } from "@/types/comfyui";
import { ComfyExportStatus } from "./ComfyExportStatus";
import { ComfyTargetMenu } from "./ComfyTargetMenu";

interface ComfyExportControlProps {
	source: ComfyExportSource;
	/** Nome curto do item, usado nos toasts. */
	label: string;
	className?: string;
	/** Recebe o botão e o bloco de status para o chamador posicionar. */
	children: (parts: { button: ReactNode; status: ReactNode }) => ReactNode;
}

/** Botão "Enviar ao ComfyUI": 0 destinos → /settings, 1 → direto, vários → menu. */
export function ComfyExportControl({
	source,
	label,
	className = "",
	children,
}: ComfyExportControlProps) {
	const { targets, loaded } = useComfyTargets();
	const { state, start } = useComfyExport(source, label);
	const [menuOpen, setMenuOpen] = useState(false);
	const anchorRef = useRef<HTMLButtonElement>(null);
	const closeMenu = useCallback(() => setMenuOpen(false), []);

	const choice = chooseExportTarget(targets);
	const busy = state.phase === "starting" || state.phase === "running";
	const cls = `shrink-0 ${className}`.trim();

	let button: ReactNode;
	if (loaded && choice.kind === "none") {
		button = (
			<Link
				href="/settings"
				className={getButtonClasses({
					variant: "secondary",
					size: "sm",
					className: cls,
				})}
				title="Nenhum destino ComfyUI cadastrado — configurar em Configurações"
			>
				<IconUpload className="size-3.5" />
				<span>Enviar ao ComfyUI</span>
			</Link>
		);
	} else {
		button = (
			<>
				<Button
					ref={anchorRef}
					type="button"
					variant="secondary"
					size="sm"
					className={cls}
					disabled={!loaded}
					loading={busy}
					aria-haspopup={choice.kind === "menu" ? "menu" : undefined}
					aria-expanded={choice.kind === "menu" ? menuOpen : undefined}
					title={
						choice.kind === "direct"
							? `Enviar ao ComfyUI (${choice.target.name})`
							: "Enviar ao ComfyUI"
					}
					onClick={() => {
						if (choice.kind === "direct") void start(choice.target);
						else if (choice.kind === "menu") setMenuOpen((o) => !o);
					}}
				>
					{!busy && <IconUpload className="size-3.5" />}
					<span>Enviar ao ComfyUI</span>
					{choice.kind === "menu" && !busy && (
						<IconChevronDown className="size-3" />
					)}
				</Button>
				{menuOpen && choice.kind === "menu" && (
					<ComfyTargetMenu
						anchorRef={anchorRef}
						targets={choice.targets}
						onClose={closeMenu}
						onSelect={(t: ComfyTarget) => {
							setMenuOpen(false);
							void start(t);
						}}
					/>
				)}
			</>
		);
	}

	const status = (
		<ComfyExportStatus
			state={state}
			onOverwrite={() => state.target && void start(state.target, true)}
		/>
	);
	return <>{children({ button, status })}</>;
}
