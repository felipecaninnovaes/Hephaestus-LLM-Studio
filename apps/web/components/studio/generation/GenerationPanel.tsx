"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { IconSliders } from "@/components/icons";
import { showToast } from "@/components/ui/Toast";
import { useJobTelemetry } from "@/hooks/useJobTelemetry";
import { isApiError } from "@/lib/api";
import { getGenerationDataUrl } from "@/lib/generations";
import {
	clearGeracaoForm,
	consumeGeracaoInitSource,
	createDefaultGeracaoForm,
	GERACAO_APPLY_FORM_EVENT,
	GERACAO_FLUX_SAMPLERS,
	GERACAO_INIT_SOURCE_EVENT,
	GERACAO_INIT_SOURCE_KEY,
	GERACAO_SAMPLERS,
	type GeracaoQuantization,
	type GeracaoSampler,
	type GeracaoUpscaleModel,
	loadGeracaoForm,
	notifyGeracaoCompleted,
	type PartialGeracaoForm,
	saveGeracaoForm,
} from "@/lib/geracao-storage";
import { getJob } from "@/lib/jobs";
import { listModels } from "@/lib/models";
import {
	getGeneratedBatchResults,
	getGeneratedImageUrl,
	startDiffusionGenerateJob,
	uploadGenerationInput,
} from "@/lib/playground";
import type {
	DiffusionGenerateJobRequest,
	Job,
	LoraRef,
	Model,
} from "@/types/studio";
import {
	diffusionGenerateErrorMessage,
	friendlyJobError,
} from "@/types/studio";
import { GenerationInitImageSection } from "./GenerationInitImageSection";
import { GenerationLightboxModal } from "./GenerationLightboxModal";
import { GenerationModelSection } from "./GenerationModelSection";
import { GenerationParametersCard } from "./GenerationParametersCard";
import { GenerationPreviewCard } from "./GenerationPreviewCard";
import { GenerationPromptCard } from "./GenerationPromptCard";
import {
	BASE_MODEL_OPTIONS,
	clampInitStrength,
	type GeneratedImageItem,
	INIT_STRENGTH_DEFAULT,
	UPSCALE_MODEL_DEFAULT,
} from "./generationTypes";

export function GenerationPanel() {
	/* ── Models data ── */
	const [allModels, setAllModels] = useState<Model[]>([]);
	const [loadingModels, setLoadingModels] = useState(true);

	/* ── Form state ── */
	const [baseModel, setBaseModel] = useState<
		"flux-2-klein-4b" | "sdxl" | "sd15" | "qwen-image-2.1"
	>("flux-2-klein-4b");
	const [customModelId, setCustomModelId] = useState<string>("");
	const [textEncoderModelId, setTextEncoderModelId] = useState<string>("");
	const [distilled, setDistilled] = useState(true);
	const [loras, setLoras] = useState<LoraRef[]>([]);
	const [prompt, setPrompt] = useState("");
	const [negativePrompt, setNegativePrompt] = useState("");
	const [showNegative, setShowNegative] = useState(false);
	const [width, setWidth] = useState(1024);
	const [height, setHeight] = useState(1024);
	const [steps, setSteps] = useState(4);
	const [guidanceScale, setGuidanceScale] = useState(1.0);
	const [seed, setSeed] = useState(() => Math.floor(Math.random() * 1000000));
	const [isLockedSeed, setIsLockedSeed] = useState(false);
	const [quantization, setQuantization] = useState<GeracaoQuantization>("4bit");
	const [sampler, setSampler] = useState<GeracaoSampler>("default");
	const [upscaleEnabled, setUpscaleEnabled] = useState(false);
	const [upscaleModel, setUpscaleModel] = useState<GeracaoUpscaleModel>(
		UPSCALE_MODEL_DEFAULT,
	);
	const [upscaleScale, setUpscaleScale] = useState<2 | 4>(4);
	const [batchSize, setBatchSize] = useState(1);
	const [selectedOrchestratorId, setSelectedOrchestratorId] = useState<
		string | null
	>(null);
	const [paramsOpen, setParamsOpen] = useState(true);

	/* ── Imagem inicial img2img ── */
	const [initImageId, setInitImageId] = useState<string | null>(null);
	const [initImageMeta, setInitImageMeta] = useState<{
		filename: string;
		width: number;
		height: number;
	} | null>(null);
	const [initGenerationId, setInitGenerationId] = useState<string | null>(null);
	const [initUploadPreview, setInitUploadPreview] = useState<string | null>(
		null,
	);
	const [initGalleryPreview, setInitGalleryPreview] = useState<string | null>(
		null,
	);
	const [initStrength, setInitStrength] = useState(INIT_STRENGTH_DEFAULT);
	const [initUploading, setInitUploading] = useState(false);
	const initFileRef = useRef<HTMLInputElement | null>(null);
	const initUploadSeqRef = useRef(0);
	const initUploadPreviewRef = useRef<string | null>(null);

	/* ── Execution state ── */
	const [submitting, setSubmitting] = useState(false);
	const [activeJobId, setActiveJobId] = useState<string | null>(null);
	const [activeJob, setActiveJob] = useState<Job | null>(null);
	const telemetry = useJobTelemetry(activeJobId);
	const [currentDisplayItem, setCurrentDisplayItem] =
		useState<GeneratedImageItem | null>(null);
	const [batchResults, setBatchResults] = useState<GeneratedImageItem[]>([]);
	const [history, setHistory] = useState<GeneratedImageItem[]>([]);
	const [lightboxOpen, setLightboxOpen] = useState(false);

	const pollingRef = useRef<number | null>(null);
	const submittedParamsRef = useRef<Omit<
		GeneratedImageItem,
		"imageUrl" | "thumbUrl"
	> | null>(null);

	/* ── Derived model lists ── */
	const diffusionModels = useMemo(
		() => allModels.filter((m) => m.engine === "diffusion"),
		[allModels],
	);
	const loraModels = useMemo(
		() => diffusionModels.filter((m) => m.kind === "lora" || !m.kind),
		[diffusionModels],
	);
	const checkpointModels = useMemo(
		() => diffusionModels.filter((m) => m.kind === "checkpoint"),
		[diffusionModels],
	);
	const textEncoderModels = useMemo(
		() => diffusionModels.filter((m) => m.kind === "text_encoder"),
		[diffusionModels],
	);

	const effectiveArch = useMemo(() => {
		if (customModelId) {
			const found = checkpointModels.find((m) => m.id === customModelId);
			return found?.arch ?? null;
		}
		return baseModel;
	}, [customModelId, checkpointModels, baseModel]);
	const isFlux2 = effectiveArch === "flux-2-klein-4b";

	const modelSelectOptions = useMemo(() => {
		const opts = BASE_MODEL_OPTIONS.map((o) => ({
			value: `preset:${o.value}`,
			label: `${o.label} (oficial)`,
			description: o.description,
		}));
		const byArch: Record<string, Model[]> = {};
		for (const m of checkpointModels) {
			const key = m.arch ?? "sem arch detectado";
			if (!byArch[key]) byArch[key] = [];
			byArch[key].push(m);
		}
		for (const arch of Object.keys(byArch).sort()) {
			for (const m of byArch[arch]) {
				opts.push({
					value: m.id,
					label: `${m.name} (custom · ${arch})`,
					description: `checkpoint ${arch} · ${m.source}`,
				});
			}
		}
		if (
			customModelId &&
			!checkpointModels.some((m) => m.id === customModelId)
		) {
			opts.push({
				value: customModelId,
				label: "Modelo removido — faça upload em Modelos & Pesos",
				description: "checkpoint indisponível",
			});
		}
		return opts;
	}, [checkpointModels, customModelId]);
	const modelSelectValue = customModelId
		? customModelId
		: `preset:${baseModel}`;

	const textEncoderOptions = useMemo(() => {
		const opts = [
			{
				value: "",
				label: "Encoder oficial BFL (padrão)",
				description: "Qwen3 do repo BFL",
			},
		];
		for (const m of textEncoderModels) {
			opts.push({
				value: m.id,
				label: m.name,
				description: `text_encoder${m.arch ? ` ${m.arch}` : ""} · ${m.source}`,
			});
		}
		if (
			textEncoderModelId &&
			!textEncoderModels.some((m) => m.id === textEncoderModelId)
		) {
			opts.push({
				value: textEncoderModelId,
				label: "Encoder removido — faça upload em Modelos & Pesos",
				description: "text_encoder indisponível",
			});
		}
		return opts;
	}, [textEncoderModels, textEncoderModelId]);

	/* ── Fetch models ── */
	useEffect(() => {
		let active = true;
		listModels()
			.then((res) => {
				if (active) {
					setAllModels(res.items.filter((m) => m.engine === "diffusion"));
					setLoadingModels(false);
				}
			})
			.catch(() => {
				if (active) setLoadingModels(false);
			});
		return () => {
			active = false;
		};
	}, []);

	/* ── Persistência do form ── */
	const hydratedFormRef = useRef(false);
	const skipNextPersistRef = useRef(false);
	const persistTimerRef = useRef<number | null>(null);

	const applyStoredForm = useCallback((stored: PartialGeracaoForm) => {
		if (stored.baseModel !== undefined) setBaseModel(stored.baseModel);
		if (stored.customModelId !== undefined)
			setCustomModelId(stored.customModelId);
		if (stored.textEncoderModelId !== undefined)
			setTextEncoderModelId(stored.textEncoderModelId);
		if (stored.distilled !== undefined) setDistilled(stored.distilled);
		if (stored.loras !== undefined) setLoras(stored.loras);
		if (stored.prompt !== undefined) setPrompt(stored.prompt);
		if (stored.negativePrompt !== undefined)
			setNegativePrompt(stored.negativePrompt);
		if (stored.showNegative !== undefined) setShowNegative(stored.showNegative);
		if (stored.width !== undefined) setWidth(stored.width);
		if (stored.height !== undefined) setHeight(stored.height);
		if (stored.steps !== undefined) setSteps(stored.steps);
		if (stored.guidanceScale !== undefined)
			setGuidanceScale(stored.guidanceScale);
		if (stored.seed !== undefined) setSeed(stored.seed);
		if (stored.isLockedSeed !== undefined) setIsLockedSeed(stored.isLockedSeed);
		if (stored.quantization !== undefined) setQuantization(stored.quantization);
		if (stored.sampler !== undefined) setSampler(stored.sampler);
		if (stored.upscale !== undefined) {
			setUpscaleEnabled(stored.upscale !== null);
			if (stored.upscale !== null) {
				setUpscaleModel(stored.upscale.model);
				setUpscaleScale(stored.upscale.scale);
			}
		}
		if (stored.batchSize !== undefined) setBatchSize(stored.batchSize);
		if (stored.initStrength !== undefined)
			setInitStrength(clampInitStrength(stored.initStrength));
	}, []);

	useEffect(() => {
		const stored = loadGeracaoForm();
		if (stored) applyStoredForm(stored);
		hydratedFormRef.current = true;
	}, [applyStoredForm]);

	useEffect(() => {
		const onApplyForm = () => {
			const stored = loadGeracaoForm();
			if (stored) applyStoredForm(stored);
		};
		window.addEventListener(GERACAO_APPLY_FORM_EVENT, onApplyForm);
		return () =>
			window.removeEventListener(GERACAO_APPLY_FORM_EVENT, onApplyForm);
	}, [applyStoredForm]);

	useEffect(() => {
		if (!hydratedFormRef.current) return;
		if (skipNextPersistRef.current) {
			skipNextPersistRef.current = false;
			return;
		}
		if (persistTimerRef.current !== null)
			window.clearTimeout(persistTimerRef.current);
		persistTimerRef.current = window.setTimeout(() => {
			saveGeracaoForm({
				baseModel,
				customModelId,
				textEncoderModelId,
				distilled,
				loras,
				prompt,
				negativePrompt,
				showNegative,
				width,
				height,
				steps,
				guidanceScale,
				seed,
				isLockedSeed,
				quantization,
				sampler,
				upscale: upscaleEnabled
					? { model: upscaleModel, scale: upscaleScale }
					: null,
				batchSize,
				initStrength,
			});
		}, 300);
		return () => {
			if (persistTimerRef.current !== null)
				window.clearTimeout(persistTimerRef.current);
		};
	}, [
		baseModel,
		customModelId,
		textEncoderModelId,
		distilled,
		loras,
		prompt,
		negativePrompt,
		showNegative,
		width,
		height,
		steps,
		guidanceScale,
		seed,
		isLockedSeed,
		quantization,
		sampler,
		upscaleEnabled,
		upscaleModel,
		upscaleScale,
		batchSize,
		initStrength,
	]);

	const handleBaseModelChange = useCallback(
		(modelVal: string) => {
			const b = modelVal as "flux-2-klein-4b" | "sdxl" | "sd15" | "qwen-image-2.1";
			setBaseModel(b);
			setSampler((prev) =>
				b === "flux-2-klein-4b"
					? (GERACAO_FLUX_SAMPLERS as readonly string[]).includes(prev)
						? prev
						: "default"
					: prev,
			);
			if (b === "flux-2-klein-4b") {
				setGuidanceScale(distilled ? 1.0 : 3.5);
				setSteps(distilled ? 4 : 20);
				setWidth(1024);
				setHeight(1024);
			} else if (b === "sdxl") {
				setGuidanceScale(7.0);
				setSteps(25);
				setWidth(1024);
				setHeight(1024);
				setShowNegative(true);
			} else if (b === "sd15") {
				setGuidanceScale(7.0);
				setSteps(20);
				setWidth(512);
				setHeight(512);
				setShowNegative(true);
			} else if (b === "qwen-image-2.1") {
				setGuidanceScale(3.5);
				setSteps(25);
				setWidth(1024);
				setHeight(1024);
				setShowNegative(false);
			}
		},
		[distilled],
	);

	const handleModelSelectChange = useCallback(
		(val: string) => {
			let nextCustom = "";
			let nextBase = baseModel;
			if (val.startsWith("preset:")) {
				nextBase = val.slice("preset:".length) as
					| "flux-2-klein-4b"
					| "sdxl"
					| "sd15"
					| "qwen-image-2.1";
				setBaseModel(nextBase);
				setCustomModelId("");
			} else {
				nextCustom = val;
				setCustomModelId(val);
			}
			const nextArch = nextCustom
				? (checkpointModels.find((m) => m.id === nextCustom)?.arch ?? null)
				: nextBase;
			if (nextArch !== "flux-2-klein-4b") setTextEncoderModelId("");
			if (!nextCustom) handleBaseModelChange(nextBase);
		},
		[baseModel, checkpointModels, handleBaseModelChange],
	);

	const handleVariantChange = useCallback((val: "distilled" | "base") => {
		const isDistilled = val === "distilled";
		setDistilled(isDistilled);
		if (isDistilled) {
			setSteps((s) => (s > 8 ? 4 : s));
			setGuidanceScale((g) => (g > 2.0 ? 1.0 : g));
		} else {
			setSteps((s) => (s < 12 ? 20 : s));
			setGuidanceScale((g) => (g <= 1.5 ? 3.5 : g));
		}
	}, []);

	const handleRollSeed = useCallback(() => {
		setSeed(Math.floor(Math.random() * 10000000));
	}, []);

	const handleInitFile = useCallback(async (file: File) => {
		const seq = ++initUploadSeqRef.current;
		setInitUploading(true);
		try {
			const uploaded = await uploadGenerationInput(file);
			if (seq !== initUploadSeqRef.current) return;
			setInitGenerationId(null);
			setInitGalleryPreview(null);
			setInitImageId(uploaded.id);
			setInitImageMeta({
				filename: uploaded.filename,
				width: uploaded.width,
				height: uploaded.height,
			});
			const objectUrl = URL.createObjectURL(file);
			initUploadPreviewRef.current = objectUrl;
			setInitUploadPreview((prev) => {
				if (prev) URL.revokeObjectURL(prev);
				return objectUrl;
			});
			showToast("Imagem inicial carregada.", "success");
		} catch (err) {
			if (seq !== initUploadSeqRef.current) return;
			showToast(
				err instanceof Error ? err.message : "Falha ao enviar imagem inicial.",
				"error",
			);
		} finally {
			if (seq === initUploadSeqRef.current) setInitUploading(false);
		}
	}, []);

	const handleClearInit = useCallback(() => {
		initUploadSeqRef.current += 1;
		setInitUploading(false);
		setInitImageId(null);
		setInitImageMeta(null);
		setInitGenerationId(null);
		setInitGalleryPreview(null);
		setInitUploadPreview((prev) => {
			if (prev) URL.revokeObjectURL(prev);
			return null;
		});
		initUploadPreviewRef.current = null;
		if (initFileRef.current) initFileRef.current.value = "";
	}, []);

	const applyInitGeneration = useCallback((generationId: string) => {
		if (!generationId) return;
		initUploadSeqRef.current += 1;
		setInitUploading(false);
		setInitUploadPreview((prev) => {
			if (prev) URL.revokeObjectURL(prev);
			return null;
		});
		initUploadPreviewRef.current = null;
		setInitImageId(null);
		setInitImageMeta(null);
		setInitGenerationId(generationId);
		setInitGalleryPreview(getGenerationDataUrl(generationId));
		if (initFileRef.current) initFileRef.current.value = "";
		showToast("Imagem da galeria carregada como entrada.", "success");
	}, []);

	useEffect(() => {
		const consumed = consumeGeracaoInitSource();
		if (consumed) applyInitGeneration(consumed.generationId);
	}, [applyInitGeneration]);

	useEffect(() => {
		const onInitSource = (e: Event) => {
			const detail = (e as CustomEvent<{ generationId?: string }>).detail;
			if (detail?.generationId) applyInitGeneration(detail.generationId);
		};
		const onStorage = (e: StorageEvent) => {
			if (e.key !== GERACAO_INIT_SOURCE_KEY || !e.newValue) return;
			try {
				const parsed = JSON.parse(e.newValue) as { generationId?: string };
				if (parsed?.generationId) applyInitGeneration(parsed.generationId);
			} catch {
				/* key corrompida — ignora */
			}
		};
		window.addEventListener(GERACAO_INIT_SOURCE_EVENT, onInitSource);
		window.addEventListener("storage", onStorage);
		return () => {
			window.removeEventListener(GERACAO_INIT_SOURCE_EVENT, onInitSource);
			window.removeEventListener("storage", onStorage);
		};
	}, [applyInitGeneration]);

	useEffect(() => {
		return () => {
			if (initUploadPreviewRef.current) {
				URL.revokeObjectURL(initUploadPreviewRef.current);
				initUploadPreviewRef.current = null;
			}
		};
	}, []);

	const handleResetForm = useCallback(() => {
		const defaults = createDefaultGeracaoForm();
		setBaseModel(defaults.baseModel);
		setCustomModelId(defaults.customModelId);
		setTextEncoderModelId(defaults.textEncoderModelId);
		setDistilled(defaults.distilled);
		setLoras(defaults.loras);
		setPrompt(defaults.prompt);
		setNegativePrompt(defaults.negativePrompt);
		setShowNegative(defaults.showNegative);
		setWidth(defaults.width);
		setHeight(defaults.height);
		setSteps(defaults.steps);
		setGuidanceScale(defaults.guidanceScale);
		setSeed(defaults.seed);
		setIsLockedSeed(defaults.isLockedSeed);
		setQuantization(defaults.quantization);
		setSampler(defaults.sampler);
		setUpscaleEnabled(defaults.upscale !== null);
		if (defaults.upscale !== null) {
			setUpscaleModel(defaults.upscale.model);
			setUpscaleScale(defaults.upscale.scale);
		}
		setBatchSize(defaults.batchSize);
		setInitStrength(defaults.initStrength);
		if (persistTimerRef.current !== null) {
			window.clearTimeout(persistTimerRef.current);
			persistTimerRef.current = null;
		}
		clearGeracaoForm();
		skipNextPersistRef.current = true;
		showToast("Configurações restauradas para o padrão.", "success");
	}, []);

	const handleGenerate = useCallback(
		async (overrideSeed?: number) => {
			if (!prompt.trim()) {
				showToast("Insira uma descrição para gerar.", "error");
				return;
			}
			const effectiveSeed =
				typeof overrideSeed === "number" && Number.isFinite(overrideSeed)
					? overrideSeed
					: seed;
			const isDistilledActive = isFlux2 ? distilled : false;

			const request: DiffusionGenerateJobRequest = {
				prompt: prompt.trim(),
				negativePrompt: negativePrompt.trim() || undefined,
				width,
				height,
				steps,
				guidanceScale,
				seed: effectiveSeed,
				quantization,
				sampler,
				upscale: upscaleEnabled
					? { model: upscaleModel, scale: upscaleScale }
					: null,
				distilled: isDistilledActive,
				batchSize: batchSize > 1 ? batchSize : undefined,
				loras:
					loras.filter((l) => l.modelId).length > 0
						? loras.filter((l) => l.modelId)
						: undefined,
				orchestratorId: selectedOrchestratorId,
			};

			if (customModelId) {
				request.customModelId = customModelId;
			} else {
				request.baseModel = baseModel;
			}
			if (isFlux2 && textEncoderModelId) {
				request.textEncoderModelId = textEncoderModelId;
			}

			if (initImageId) {
				request.initImageId = initImageId;
				request.initStrength = initStrength;
			} else if (initGenerationId) {
				request.initGenerationId = initGenerationId;
				request.initStrength = initStrength;
			}

			submittedParamsRef.current = {
				jobId: "",
				prompt: prompt.trim(),
				negativePrompt: negativePrompt.trim() || undefined,
				baseModel: customModelId ? `custom:${customModelId}` : baseModel,
				customModelId: customModelId ? customModelId : undefined,
				seed: effectiveSeed,
				steps,
				guidanceScale,
				quantization,
				sampler,
				upscale: upscaleEnabled
					? { model: upscaleModel, scale: upscaleScale }
					: null,
				distilled: isDistilledActive,
				loras: loras.filter((l) => l.modelId),
				width,
				height,
				createdAt: new Date().toISOString(),
			};

			setSubmitting(true);
			try {
				const res = await startDiffusionGenerateJob(request);
				setActiveJobId(res.jobId);

				showToast(
					res.status === "preparing"
						? "Geração aceita — preparando pacote."
						: res.queuePosition
							? `Job enfileirado (posição ${res.queuePosition})`
							: "Geração iniciada!",
					"info",
				);

				if (!isLockedSeed) {
					setSeed(Math.floor(Math.random() * 1000000));
				}
			} catch (err) {
				console.error("Falha ao submeter job de difusão:", err);
				if (isApiError(err)) {
					const detailed =
						err.message && err.message !== "invalid request"
							? err.message
							: diffusionGenerateErrorMessage(err.code);
					showToast(detailed, "error");
				} else {
					const fallback =
						err instanceof Error && err.message
							? err.message
							: "Erro inesperado ao iniciar a geração.";
					showToast(fallback, "error");
				}
			} finally {
				setSubmitting(false);
			}
		},
		[
			prompt,
			negativePrompt,
			width,
			height,
			steps,
			guidanceScale,
			seed,
			quantization,
			sampler,
			upscaleEnabled,
			upscaleModel,
			upscaleScale,
			batchSize,
			distilled,
			isFlux2,
			loras,
			selectedOrchestratorId,
			customModelId,
			baseModel,
			textEncoderModelId,
			isLockedSeed,
			initImageId,
			initGenerationId,
			initStrength,
		],
	);

	const handleCancelJob = useCallback(async () => {
		if (!activeJobId) return;
		try {
			setActiveJobId(null);
			showToast("Geração cancelada.", "info");
		} catch {
			showToast("Falha ao cancelar o job.", "error");
		}
	}, [activeJobId]);

	useEffect(() => {
		if (!activeJobId) {
			setActiveJob(null);
			if (pollingRef.current !== null) {
				window.clearInterval(pollingRef.current);
				pollingRef.current = null;
			}
			return;
		}

		let cancelled = false;

		const checkJob = async () => {
			try {
				const job = await getJob(activeJobId);
				if (cancelled) return;
				setActiveJob(job);

				if (job.status === "done") {
					const batchItems = await getGeneratedBatchResults(job.id);
					const params = submittedParamsRef.current || {
						prompt,
						negativePrompt,
						baseModel,
						customModelId,
						seed,
						steps,
						guidanceScale,
						quantization,
						sampler,
						upscale: upscaleEnabled
							? { model: upscaleModel, scale: upscaleScale }
							: null,
						loras,
						width,
						height,
					};

					const newItems: GeneratedImageItem[] = batchItems.map(
						(item, idx) => ({
							jobId: job.id,
							imageUrl: item.imageUrl,
							thumbUrl: item.thumbUrl,
							prompt: params.prompt,
							negativePrompt: params.negativePrompt,
							baseModel: params.baseModel,
							customModelId: params.customModelId,
							seed: params.seed + idx,
							steps: params.steps,
							guidanceScale: params.guidanceScale,
							quantization: params.quantization,
							sampler: params.sampler,
							upscale: params.upscale ?? null,
							loras: params.loras,
							width: params.width,
							height: params.height,
							batchIndex: idx,
							createdAt: job.finishedAt || new Date().toISOString(),
						}),
					);

					if (newItems.length > 0) {
						setBatchResults(newItems);
						setCurrentDisplayItem(newItems[0]);
						setHistory((prev) => [
							...newItems.filter(
								(n) =>
									!prev.some(
										(h) => h.jobId === n.jobId && h.batchIndex === n.batchIndex,
									),
							),
							...prev,
						]);
						notifyGeracaoCompleted(job.id);
					} else {
						const imgUrl = await getGeneratedImageUrl(job.id);
						if (imgUrl && !cancelled) {
							const singleItem: GeneratedImageItem = {
								jobId: job.id,
								imageUrl: imgUrl,
								prompt: params.prompt,
								negativePrompt: params.negativePrompt,
								baseModel: params.baseModel,
								customModelId: params.customModelId,
								seed: params.seed,
								steps: params.steps,
								guidanceScale: params.guidanceScale,
								quantization: params.quantization,
								sampler: params.sampler,
								upscale: params.upscale ?? null,
								loras: params.loras,
								width: params.width,
								height: params.height,
								createdAt: job.finishedAt || new Date().toISOString(),
							};
							setBatchResults([singleItem]);
							setCurrentDisplayItem(singleItem);
							setHistory((prev) => [
								singleItem,
								...prev.filter((h) => h.jobId !== singleItem.jobId),
							]);
							notifyGeracaoCompleted(job.id);
						}
					}

					showToast("Geração concluída!", "success");
					setActiveJobId(null);
				} else if (job.status === "failed" || job.status === "cancelled") {
					setActiveJobId(null);
					showToast(
						friendlyJobError(job.error) || "A geração falhou.",
						"error",
					);
				}
			} catch {
				// ignore sparse polling failures
			}
		};

		void checkJob();
		pollingRef.current = window.setInterval(checkJob, 2000);

		return () => {
			cancelled = true;
			if (pollingRef.current !== null) {
				window.clearInterval(pollingRef.current);
				pollingRef.current = null;
			}
		};
	}, [
		activeJobId,
		prompt,
		negativePrompt,
		baseModel,
		customModelId,
		seed,
		steps,
		guidanceScale,
		quantization,
		sampler,
		upscaleEnabled,
		upscaleModel,
		upscaleScale,
		loras,
		width,
		height,
	]);

	const handleDownload = useCallback(() => {
		if (!currentDisplayItem) return;
		const a = document.createElement("a");
		a.href = currentDisplayItem.imageUrl;
		a.download = `geracao-${currentDisplayItem.baseModel}-seed${currentDisplayItem.seed}.png`;
		document.body.appendChild(a);
		a.click();
		document.body.removeChild(a);
	}, [currentDisplayItem]);

	const isBusy = submitting || !!activeJobId;
	const initActive = initImageId !== null || initGenerationId !== null;
	const initPreviewUrl = initUploadPreview ?? initGalleryPreview;
	const initOriginLabel = initImageId
		? "Upload"
		: initGenerationId
			? "Galeria"
			: null;

	/* Samplers dinâmicos por modelo */
	const samplerOptions = useMemo(() => {
		const list = isFlux2 ? GERACAO_FLUX_SAMPLERS : GERACAO_SAMPLERS;
		return list.map((s) => ({
			value: s,
			label: s === "default" ? "Padrão" : s,
		}));
	}, [isFlux2]);

	/* Hotkey Ctrl+Enter / Cmd+Enter */
	useEffect(() => {
		const handleKeyDown = (e: KeyboardEvent) => {
			if ((e.ctrlKey || e.metaKey) && e.key === "Enter") {
				e.preventDefault();
				if (!isBusy) void handleGenerate();
			}
		};
		window.addEventListener("keydown", handleKeyDown);
		return () => window.removeEventListener("keydown", handleKeyDown);
	}, [isBusy, handleGenerate]);

	return (
		<div className="flex flex-col lg:h-full lg:min-h-0 lg:flex-row">
			{/* ════ COLUNA ESQUERDA — CONTROLES (320-384px, scrollável) ════ */}
			<div className="w-full shrink-0 lg:w-[384px] lg:overflow-y-auto glass-card rounded-2xl">
				<div className="lg:hidden">
					<button
						type="button"
						onClick={() => setParamsOpen((p) => !p)}
						className="glass-card flex w-full items-center justify-between px-4 py-3 text-sm font-semibold text-zinc-200 cursor-pointer"
					>
						<span className="flex items-center gap-2">
							<IconSliders className="size-4 text-brand-400" />
							Parâmetros
						</span>
						<span className="font-mono text-3xs text-zinc-400">
							{paramsOpen ? "Ocultar" : "Mostrar"}
						</span>
					</button>
				</div>

				<div
					className={`${paramsOpen ? "block" : "hidden"} lg:block p-4 md:p-5 space-y-4`}
				>
					<GenerationModelSection
						modelSelectOptions={modelSelectOptions}
						modelSelectValue={modelSelectValue}
						onModelSelectChange={handleModelSelectChange}
						disabled={isBusy}
						loadingModels={loadingModels}
						checkpointModelsCount={checkpointModels.length}
						customModelId={customModelId}
						effectiveArch={effectiveArch}
						textEncoderOptions={textEncoderOptions}
						textEncoderModelId={textEncoderModelId}
						setTextEncoderModelId={setTextEncoderModelId}
						isFlux2={isFlux2}
						distilled={distilled}
						onVariantChange={handleVariantChange}
						loras={loras}
						setLoras={setLoras}
						loraModels={loraModels}
					/>

					<GenerationPromptCard
						prompt={prompt}
						setPrompt={setPrompt}
						negativePrompt={negativePrompt}
						setNegativePrompt={setNegativePrompt}
						showNegative={showNegative}
						setShowNegative={setShowNegative}
						disabled={isBusy}
					/>

					<GenerationInitImageSection
						initActive={initActive}
						initUploading={initUploading}
						initPreviewUrl={initPreviewUrl}
						initOriginLabel={initOriginLabel}
						initImageMeta={initImageMeta}
						initGenerationId={initGenerationId}
						initStrength={initStrength}
						setInitStrength={setInitStrength}
						onClearInit={handleClearInit}
						onInitFile={handleInitFile}
						initFileRef={initFileRef}
						disabled={isBusy}
					/>

					<GenerationParametersCard
						width={width}
						height={height}
						setWidth={setWidth}
						setHeight={setHeight}
						steps={steps}
						setSteps={setSteps}
						guidanceScale={guidanceScale}
						setGuidanceScale={setGuidanceScale}
						isLockedSeed={isLockedSeed}
						setIsLockedSeed={setIsLockedSeed}
						seed={seed}
						setSeed={setSeed}
						onRollSeed={handleRollSeed}
						sampler={sampler}
						setSampler={setSampler}
						samplerOptions={samplerOptions}
						quantization={quantization}
						setQuantization={setQuantization}
						upscaleEnabled={upscaleEnabled}
						setUpscaleEnabled={setUpscaleEnabled}
						upscaleModel={upscaleModel}
						setUpscaleModel={setUpscaleModel}
						upscaleScale={upscaleScale}
						setUpscaleScale={setUpscaleScale}
						batchSize={batchSize}
						setBatchSize={setBatchSize}
						selectedOrchestratorId={selectedOrchestratorId}
						setSelectedOrchestratorId={setSelectedOrchestratorId}
						isBusy={isBusy}
						activeJobId={activeJobId}
						onGenerate={() => void handleGenerate()}
						onCancelJob={handleCancelJob}
						onResetForm={handleResetForm}
					/>
				</div>
			</div>

			{/* ════ COLUNA DIREITA — RESULTADO + TELEMETRIA ════ */}
			<GenerationPreviewCard
				activeJobId={activeJobId}
				activeJob={activeJob}
				telemetry={telemetry}
				currentDisplayItem={currentDisplayItem}
				batchResults={batchResults}
				history={history}
				onSelectDisplayItem={setCurrentDisplayItem}
				onDownload={handleDownload}
				onOpenLightbox={() => setLightboxOpen(true)}
			/>

			{/* ════ LIGHTBOX ════ */}
			<GenerationLightboxModal
				open={lightboxOpen && !!currentDisplayItem}
				onClose={() => setLightboxOpen(false)}
				item={currentDisplayItem}
				onDownload={handleDownload}
			/>
		</div>
	);
}

export default GenerationPanel;
