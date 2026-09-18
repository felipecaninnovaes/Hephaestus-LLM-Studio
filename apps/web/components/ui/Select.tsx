"use client";

import React, {
	forwardRef,
	useCallback,
	useEffect,
	useId,
	useImperativeHandle,
	useRef,
	useState,
} from "react";
import { createPortal } from "react-dom";
import { IconCheck, IconChevronDown, IconSearch } from "@/components/icons";

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
	align?: "left" | "right" | "auto";
	menuWidth?: "trigger" | "auto" | string;
}

export const Select = forwardRef<SelectRefHandle, SelectProps<any>>(
	function Select(
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
		},
		forwardedRef,
	) {
		const generatedId = useId();
		const selectId =
			id || (label ? label.toLowerCase().replace(/\s+/g, "-") : generatedId);
		const listboxId = `${selectId}-listbox`;

		const [isOpen, setIsOpen] = useState(false);
		const [internalValue, setInternalValue] = useState<
			string | number | undefined
		>(value !== undefined ? value : defaultValue);
		const [searchQuery, setSearchQuery] = useState("");
		const [highlightedIndex, setHighlightedIndex] = useState<number>(-1);
		const [placement, setPlacement] = useState<"bottom" | "top">("bottom");
		const [horizontalPlacement, setHorizontalPlacement] = useState<
			"left" | "right"
		>("left");
		// Fixed viewport coords for the portaled menu (anchored to the trigger rect).
		const [menuCoords, setMenuCoords] = useState<{
			top?: number;
			bottom?: number;
			left?: number;
			right?: number;
			width?: number;
		} | null>(null);

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

		// Filter options if searchable
		const filteredOptions = React.useMemo(() => {
			if (!searchable || !searchQuery.trim()) return options;
			const q = searchQuery.toLowerCase().trim();
			return options.filter(
				(opt) =>
					opt.label.toLowerCase().includes(q) ||
					(opt.description?.toLowerCase().includes(q)),
			);
		}, [options, searchable, searchQuery]);

		const selectedOption = React.useMemo(() => {
			return options.find((opt) => String(opt.value) === String(currentValue));
		}, [options, currentValue]);

		// Compute flip (top/bottom, left/right) + fixed viewport coords for the
		// portaled menu, anchored to the trigger's getBoundingClientRect().
		const updatePlacement = useCallback(() => {
			const anchor = triggerRef.current ?? containerRef.current;
			if (!anchor) return;
			const rect = anchor.getBoundingClientRect();
			const gap = 6; // matches previous mt-1.5 / mb-1.5 offset
			const minMenuHeight = 200;

			let vertical: "bottom" | "top";
			const spaceBelow = window.innerHeight - rect.bottom;
			if (spaceBelow < minMenuHeight && rect.top > minMenuHeight) {
				vertical = "top";
			} else {
				vertical = "bottom";
			}
			setPlacement(vertical);

			let horizontal: "left" | "right";
			if (align === "right") {
				horizontal = "right";
			} else if (align === "left") {
				horizontal = "left";
			} else {
				// Auto: if trigger is close to the right edge of the viewport, align to right
				const spaceRight = window.innerWidth - rect.right;
				horizontal = spaceRight < 240 ? "right" : "left";
			}
			setHorizontalPlacement(horizontal);

			setMenuCoords({
				...(vertical === "bottom"
					? { top: rect.bottom + gap }
					: { bottom: window.innerHeight - rect.top + gap }),
				...(horizontal === "left"
					? { left: rect.left }
					: { right: window.innerWidth - rect.right }),
				// Default ("trigger"): menu is EXACTLY the trigger width.
				...(menuWidth === "trigger" || menuWidth === "fixed"
					? { width: rect.width }
					: {}),
			});
		}, [align, menuWidth]);

		// Handle outside clicks
		useEffect(() => {
			if (!isOpen) return;

			function handlePointerDown(e: PointerEvent) {
				const target = e.target as Node;
				if (containerRef.current?.contains(target)) {
					return;
				}
				// Menu is portaled to document.body (outside the container): clicks
				// inside it must NOT close the menu.
				if (menuRef.current?.contains(target)) {
					return;
				}
				setIsOpen(false);
			}

			document.addEventListener("pointerdown", handlePointerDown);
			return () =>
				document.removeEventListener("pointerdown", handlePointerDown);
		}, [isOpen]);

		// Keep the portaled menu anchored to the trigger while open
		// (panel/page scroll or viewport resize recalculates fixed coords).
		useEffect(() => {
			if (!isOpen) return;
			const handleReposition = () => updatePlacement();
			window.addEventListener("resize", handleReposition);
			document.addEventListener("scroll", handleReposition, true);
			return () => {
				window.removeEventListener("resize", handleReposition);
				document.removeEventListener("scroll", handleReposition, true);
			};
		}, [isOpen, updatePlacement]);

		// When opening, reset search, set initial highlighted index, update placement
		useEffect(() => {
			if (isOpen) {
				updatePlacement();
				setSearchQuery("");
				const selectedIdx = filteredOptions.findIndex(
					(opt) => String(opt.value) === String(currentValue),
				);
				setHighlightedIndex(selectedIdx >= 0 ? selectedIdx : 0);
			} else {
				setSearchQuery("");
				setHighlightedIndex(-1);
				setMenuCoords(null);
			}
		}, [isOpen, currentValue, filteredOptions, updatePlacement]);

		// Focus the search input once the portaled menu exists (menuCoords set).
		useEffect(() => {
			if (isOpen && menuCoords && searchable) {
				searchInputRef.current?.focus();
			}
		}, [isOpen, menuCoords, searchable]);

		// Scroll highlighted item into view
		useEffect(() => {
			if (isOpen && highlightedIndex >= 0 && optionsListRef.current) {
				const el = optionsListRef.current.children[
					highlightedIndex
				] as HTMLElement;
				if (el) {
					el.scrollIntoView({ block: "nearest" });
				}
			}
		}, [highlightedIndex, isOpen]);

		const selectOption = useCallback(
			(opt: SelectOption<any>) => {
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

		// Keyboard navigation
		const handleKeyDown = useCallback(
			(e: React.KeyboardEvent) => {
				if (disabled || loading) return;

				if (!isOpen) {
					if (
						e.key === "ArrowDown" ||
						e.key === "ArrowUp" ||
						e.key === "Enter" ||
						e.key === " "
					) {
						e.preventDefault();
						setIsOpen(true);
					}
					return;
				}

				switch (e.key) {
					case "Escape":
						e.preventDefault();
						setIsOpen(false);
						triggerRef.current?.focus();
						break;

					case "ArrowDown": {
						e.preventDefault();
						if (filteredOptions.length === 0) break;
						let nextIdx = (highlightedIndex + 1) % filteredOptions.length;
						let attempts = 0;
						while (
							filteredOptions[nextIdx]?.disabled &&
							attempts < filteredOptions.length
						) {
							nextIdx = (nextIdx + 1) % filteredOptions.length;
							attempts++;
						}
						setHighlightedIndex(nextIdx);
						break;
					}

					case "ArrowUp": {
						e.preventDefault();
						if (filteredOptions.length === 0) break;
						let prevIdx =
							(highlightedIndex - 1 + filteredOptions.length) %
							filteredOptions.length;
						let attempts = 0;
						while (
							filteredOptions[prevIdx]?.disabled &&
							attempts < filteredOptions.length
						) {
							prevIdx =
								(prevIdx - 1 + filteredOptions.length) % filteredOptions.length;
							attempts++;
						}
						setHighlightedIndex(prevIdx);
						break;
					}

					case "Enter":
					case " ": {
						if (
							e.key === " " &&
							document.activeElement === searchInputRef.current
						) {
							return;
						}
						e.preventDefault();
						const targetOpt = filteredOptions[highlightedIndex];
						if (targetOpt && !targetOpt.disabled) {
							selectOption(targetOpt);
						}
						break;
					}

					case "Tab":
						setIsOpen(false);
						break;
				}
			},
			[
				disabled,
				loading,
				isOpen,
				filteredOptions,
				highlightedIndex,
				selectOption,
			],
		);

		// Size styling
		const sizeClasses = {
			sm: "h-8 text-2xs px-2.5 py-1",
			default: "min-h-[38px] text-xs px-3.5 py-2",
			lg: "min-h-[44px] text-sm px-4 py-2.5",
		}[size];

		// Portal (position: fixed in document.body) has no parent sizing context:
		// "trigger" (default) uses an exact inline width (rect.width); "auto" sizes
		// to content; custom strings pass through as classes.
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
				{/* Hidden input for standard form submission */}
				{name && (
					<input
						type="hidden"
						name={name}
						value={currentValue !== undefined ? String(currentValue) : ""}
					/>
				)}

				{/* Label */}
				{label && (
					<label
						id={`${selectId}-label`}
						htmlFor={selectId}
						className="tracking-caps mb-1.5 block font-mono text-2xs font-medium uppercase text-zinc-300 select-none cursor-pointer"
					>
						{label}
					</label>
				)}

				{/* Trigger Button */}
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
					} ${sizeClasses} ${
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
									<span className="ml-auto shrink-0">
										{selectedOption.badge}
									</span>
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

				{/* Dropdown Popover (.glass-menu) — portaled to document.body (fixed),
          so ancestor overflow (e.g. overflow-y-auto panels) can't clip it
          or coerce horizontal scrolling. */}
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
								zIndex: 50,
							}}
							className={`glass-menu rounded-xl p-1.5 shadow-2xl ${widthClasses} ${menuClassName}`}
						>
							{/* Optional Search Box */}
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

							{/* Options List */}
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

				{/* Error message */}
				{error && (
					<p role="alert" className="mt-1 font-mono text-2xs text-rose-300">
						{error}
					</p>
				)}

				{/* Help / hint message */}
				{!error && hint && (
					<p className="mt-1 font-mono text-2xs text-zinc-500 leading-normal">
						{hint}
					</p>
				)}
			</div>
		);
	},
);

Select.displayName = "Select";
export default Select;
