"use client";

import React, {
	forwardRef,
	useCallback,
	useEffect,
	useId,
	useImperativeHandle,
	useMemo,
	useRef,
	useState,
	type ForwardedRef,
	type ReactElement,
} from "react";
import { createPortal } from "react-dom";
import { IconCheck, IconChevronDown, IconSearch } from "@/components/icons";
import {
	useFloatingPosition,
	type Alignment,
	type FloatingWidth,
} from "@/hooks/useFloatingPosition";
import { useListboxNavigation } from "@/hooks/useListboxNavigation";

export interface SelectOption<T extends string | number = string | number> {
	value: T;
	label: string;
	description?: string;
	badge?: React.ReactNode;
	icon?: React.ReactNode;
	disabled?: boolean;
	disabledReason?: string;
}

export interface SelectRefHandle {
	focus: () => void;
	open: () => void;
	close: () => void;
}

export interface SelectProps<T extends string | number = string | number> {
	id?: string;
	name?: string;
	label?: string;
	error?: string | null;
	hint?: string;
	options: SelectOption<T>[];
	value?: T;
	defaultValue?: T;
	onChange?: (value: T) => void;
	placeholder?: string;
	disabled?: boolean;
	loading?: boolean;
	loadingText?: string;
	emptyText?: string;
	fontMono?: boolean;
	searchable?: boolean;
	searchPlaceholder?: string;
	className?: string;
	triggerClassName?: string;
	menuClassName?: string;
	size?: "sm" | "default" | "lg";
	align?: Alignment;
	menuWidth?: FloatingWidth;
}

const sizeClasses: Record<"sm" | "default" | "lg", string> = {
	sm: "h-8 text-2xs px-2.5 py-1",
	default: "min-h-[38px] text-xs px-3.5 py-2",
	lg: "min-h-[44px] text-sm px-4 py-2.5",
};

function SelectInner<T extends string | number = string | number>(
	{
		id,
		name,
		label,
		error,
		hint,
		options = [],
		value,
		defaultValue,
		onChange,
		placeholder = "Selecione uma opção…",
		disabled = false,
		loading = false,
		loadingText = "Carregando opções…",
		emptyText = "Nenhuma opção encontrada",
		fontMono = false,
		searchable = false,
		searchPlaceholder = "Buscar opção…",
		className = "",
		triggerClassName = "",
		menuClassName = "",
		size = "default",
		align = "auto",
		menuWidth = "trigger",
	}: SelectProps<T>,
	forwardedRef: ForwardedRef<SelectRefHandle>,
): ReactElement | null {
	const generatedId = useId();
	const selectId =
		id || (label ? label.toLowerCase().replace(/\s+/g, "-") : generatedId);
	const listboxId = `${selectId}-listbox`;

	const [isOpen, setIsOpen] = useState(false);
	const [internalValue, setInternalValue] = useState<T | undefined>(
		value !== undefined ? value : defaultValue,
	);
	const [searchQuery, setSearchQuery] = useState("");

	const containerRef = useRef<HTMLDivElement>(null);
	const triggerRef = useRef<HTMLButtonElement>(null);
	const menuRef = useRef<HTMLDivElement>(null);
	const searchInputRef = useRef<HTMLInputElement>(null);
	const optionsListRef = useRef<HTMLDivElement>(null);

	const isControlled = value !== undefined;
	const currentValue = isControlled ? value : internalValue;

	useImperativeHandle(forwardedRef, () => ({
		focus: () => triggerRef.current?.focus(),
		open: () => {
			if (!disabled && !loading) setIsOpen(true);
		},
		close: () => setIsOpen(false),
	}));

	const filteredOptions = useMemo(() => {
		if (!searchable || !searchQuery.trim()) return options;
		const q = searchQuery.toLowerCase().trim();
		return options.filter(
			(opt) =>
				opt.label.toLowerCase().includes(q) ||
				opt.description?.toLowerCase().includes(q),
		);
	}, [options, searchable, searchQuery]);

	const selectedOption = useMemo(() => {
		return options.find((opt) => String(opt.value) === String(currentValue));
	}, [options, currentValue]);

	const selectedIndex = useMemo(() => {
		return filteredOptions.findIndex(
			(opt) => String(opt.value) === String(currentValue),
		);
	}, [filteredOptions, currentValue]);

	const selectOption = useCallback(
		(opt: SelectOption<T>) => {
			if (opt.disabled) return;
			if (!isControlled) {
				setInternalValue(opt.value);
			}
			onChange?.(opt.value);
			setIsOpen(false);
			triggerRef.current?.focus();
		},
		[isControlled, onChange],
	);

	const {
		coords: menuCoords,
		placement,
		horizontalPlacement,
	} = useFloatingPosition({
		anchorRef: triggerRef,
		isOpen,
		align,
		menuWidth,
	});

	const { highlightedIndex, setHighlightedIndex, handleKeyDown } =
		useListboxNavigation<SelectOption<T>>({
			items: filteredOptions,
			isOpen,
			setIsOpen,
			optionsListRef,
			triggerRef,
			onSelect: selectOption,
			selectedIndex,
			disabled,
			loading,
		});

	// Fechamento ao clicar fora (portaled menu fica no document.body)
	useEffect(() => {
		if (!isOpen) return;

		function handlePointerDown(e: PointerEvent) {
			const target = e.target as Node;
			if (containerRef.current?.contains(target)) return;
			if (menuRef.current?.contains(target)) return;
			setIsOpen(false);
		}

		document.addEventListener("pointerdown", handlePointerDown);
		return () =>
			document.removeEventListener("pointerdown", handlePointerDown);
	}, [isOpen]);

	// Reseta busca ao fechar
	useEffect(() => {
		if (!isOpen) {
			setSearchQuery("");
		}
	}, [isOpen]);

	// Foco no input de busca ao abrir menu
	useEffect(() => {
		if (isOpen && menuCoords && searchable) {
			searchInputRef.current?.focus();
		}
	}, [isOpen, menuCoords, searchable]);

	const widthClasses =
		menuWidth === "auto"
			? "w-max max-w-[min(440px,calc(100vw-32px))]"
			: menuWidth && menuWidth !== "trigger" && menuWidth !== "fixed"
				? menuWidth
				: "max-w-[calc(100vw-32px)]";

	return (
		<div
			ref={containerRef}
			className={`relative w-full ${isOpen ? "z-50" : "z-auto"} ${className}`}
		>
			{name && (
				<input
					type="hidden"
					name={name}
					value={currentValue !== undefined ? String(currentValue) : ""}
				/>
			)}

			{label && (
				<label
					id={`${selectId}-label`}
					htmlFor={selectId}
					className="tracking-caps mb-1.5 block font-mono text-2xs font-medium uppercase text-zinc-300 select-none cursor-pointer"
				>
					{label}
				</label>
			)}

			<button
				ref={triggerRef}
				type="button"
				id={selectId}
				role="combobox"
				aria-haspopup="listbox"
				aria-expanded={isOpen}
				aria-controls={listboxId}
				aria-labelledby={label ? `${selectId}-label` : undefined}
				aria-activedescendant={
					isOpen && highlightedIndex >= 0
						? `${listboxId}-opt-${highlightedIndex}`
						: undefined
				}
				disabled={disabled || loading}
				onClick={() => setIsOpen((prev) => !prev)}
				onKeyDown={handleKeyDown}
				className={`group flex w-full items-center justify-between gap-2.5 rounded-xl border bg-black/40 backdrop-blur-sm text-left transition-all duration-150 select-none ${
					fontMono ? "font-mono" : "font-sans"
				} ${sizeClasses[size]} ${
					isOpen
						? "border-brand-500 ring-2 ring-brand-500/30 bg-black/60 shadow-[0_0_15px_rgba(131,80,242,0.15)]"
						: error
							? "border-rose-500/50 hover:border-rose-500/70"
							: "border-zinc-800 hover:border-zinc-700 hover:bg-black/55"
				} ${
					disabled || loading
						? "cursor-not-allowed opacity-55"
						: "cursor-pointer focus-visible:outline-none focus-visible:border-brand-500 focus-visible:ring-2 focus-visible:ring-brand-500/60"
				} ${triggerClassName}`}
			>
				<div className="flex min-w-0 flex-1 items-center gap-2">
					{loading ? (
						<span className="flex items-center gap-2 text-zinc-500 text-xs font-mono">
							<span className="h-1.5 w-1.5 animate-pulse rounded-full bg-brand-400" />
							{loadingText}
						</span>
					) : selectedOption ? (
						<div className="flex min-w-0 flex-1 items-center gap-2">
							{selectedOption.icon && (
								<span className="shrink-0 text-zinc-400 group-hover:text-zinc-300 transition-colors [&_svg]:size-3.5">
									{selectedOption.icon}
								</span>
							)}
							<span
								className="truncate font-medium text-zinc-100"
								title={selectedOption.label}
							>
								{selectedOption.label}
							</span>
							{selectedOption.badge && (
								<span className="ml-auto shrink-0">{selectedOption.badge}</span>
							)}
						</div>
					) : (
						<span className="truncate text-zinc-500">{placeholder}</span>
					)}
				</div>

				<span
					className={`shrink-0 text-zinc-500 transition-transform duration-200 [&_svg]:size-3.5 ${
						isOpen ? "rotate-180 text-brand-400" : "group-hover:text-zinc-400"
					}`}
				>
					<IconChevronDown />
				</span>
			</button>

			{isOpen &&
				menuCoords &&
				typeof document !== "undefined" &&
				createPortal(
					<div
						ref={menuRef}
						data-placement={placement}
						data-align={horizontalPlacement}
						style={{
							position: "fixed",
							top: menuCoords.top,
							bottom: menuCoords.bottom,
							left: menuCoords.left,
							right: menuCoords.right,
							width: menuCoords.width,
							zIndex: 500,
						}}
						className={`glass-menu rounded-xl p-1.5 shadow-2xl ${widthClasses} ${menuClassName}`}
					>
						{searchable && (
							<div className="relative mb-1.5 px-1 pt-1">
								<div className="relative flex items-center">
									<span className="pointer-events-none absolute left-2.5 flex items-center text-zinc-500 [&_svg]:size-3.5">
										<IconSearch />
									</span>
									<input
										ref={searchInputRef}
										type="text"
										value={searchQuery}
										onChange={(e) => {
											setSearchQuery(e.target.value);
											setHighlightedIndex(0);
										}}
										placeholder={searchPlaceholder}
										className="w-full rounded-lg border border-white/10 bg-black/50 backdrop-blur-sm py-1.5 pr-3 pl-8 font-mono text-xs text-zinc-200 placeholder-zinc-500 transition focus:border-brand-500/70 focus:bg-black/70 focus:outline-none focus:ring-1 focus:ring-brand-500/40"
										onKeyDown={(e) => {
											if (e.key === "Enter") {
												e.preventDefault();
											}
											handleKeyDown(e);
										}}
									/>
								</div>
							</div>
						)}

						<div
							ref={optionsListRef}
							id={listboxId}
							role="listbox"
							aria-label={label || "Opções"}
							onKeyDown={handleKeyDown}
							className="max-h-60 overflow-y-auto overscroll-contain py-0.5 space-y-0.5 focus:outline-none scrollbar-thin"
						>
							{filteredOptions.length === 0 ? (
								<div className="px-3 py-4 text-center font-mono text-2xs text-zinc-500">
									{emptyText}
								</div>
							) : (
								filteredOptions.map((opt, idx) => {
									const isSelected =
										String(opt.value) === String(currentValue);
									const isHighlighted = idx === highlightedIndex;
									const isDisabled = !!opt.disabled;

									return (
										<div
											key={String(opt.value)}
											id={`${listboxId}-opt-${idx}`}
											role="option"
											aria-selected={isSelected}
											aria-disabled={isDisabled}
											tabIndex={isDisabled ? -1 : isHighlighted ? 0 : -1}
											title={
												isDisabled && opt.disabledReason
													? opt.disabledReason
													: opt.label
											}
											onClick={() => !isDisabled && selectOption(opt)}
											onKeyDown={(e) => {
												if (isDisabled) return;
												if (e.key === "Enter" || e.key === " ") {
													e.preventDefault();
													selectOption(opt);
												}
											}}
											onMouseEnter={() =>
												!isDisabled && setHighlightedIndex(idx)
											}
											className={`group/item relative flex cursor-pointer items-center justify-between rounded-lg px-2.5 py-2 text-xs transition-all select-none ${
												fontMono ? "font-mono" : "font-sans"
											} ${
												isDisabled
													? "cursor-not-allowed opacity-45 bg-transparent text-zinc-500"
													: isSelected
														? "bg-brand-500/15 text-white font-medium border-l-2 border-brand-500 shadow-[inset_0_1px_0_rgba(255,255,255,0.06)]"
														: isHighlighted
															? "bg-white/[0.07] text-zinc-100"
															: "text-zinc-300 hover:bg-white/[0.04] hover:text-zinc-100"
											}`}
										>
											<div className="flex min-w-0 flex-1 items-center gap-2.5">
												{opt.icon && (
													<span
														className={`shrink-0 transition-colors [&_svg]:size-3.5 ${
															isSelected
																? "text-brand-400"
																: "text-zinc-500 group-hover/item:text-zinc-400"
														}`}
													>
														{opt.icon}
													</span>
												)}

												<div className="flex min-w-0 flex-col">
													<span className="leading-snug">{opt.label}</span>
													{opt.description && (
														<span className="truncate text-2xs text-zinc-500">
															{opt.description}
														</span>
													)}
													{isDisabled && opt.disabledReason && (
														<span className="mt-0.5 truncate text-2xs font-mono text-rose-400/80">
															{opt.disabledReason}
														</span>
													)}
												</div>
											</div>

											<div className="flex items-center gap-2 shrink-0 ml-2">
												{opt.badge && <div>{opt.badge}</div>}
												{isSelected && (
													<span className="flex items-center text-brand-400 [&_svg]:size-3.5">
														<IconCheck />
													</span>
												)}
											</div>
										</div>
									);
								})
							)}
						</div>
					</div>,
					document.body,
				)}

			{error && (
				<p role="alert" className="mt-1 font-mono text-2xs text-rose-300">
					{error}
				</p>
			)}

			{!error && hint && (
				<p className="mt-1 font-mono text-2xs text-zinc-500 leading-normal">
					{hint}
				</p>
			)}
		</div>
	);
}

export interface SelectComponent {
	<T extends string | number = string | number>(
		props: SelectProps<T> & React.RefAttributes<SelectRefHandle>,
	): ReactElement | null;
	displayName?: string;
}

export const Select: SelectComponent = forwardRef(SelectInner) as unknown as SelectComponent;
Select.displayName = "Select";

export default Select;
