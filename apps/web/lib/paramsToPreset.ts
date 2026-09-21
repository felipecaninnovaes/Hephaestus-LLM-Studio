/**
 * Mapper ÚNICO job.params (camelCase do BFF) → preset da Forja + payloads de
 * resume/rerun via sessionStorage (C1 — treino-observabilidade).
 *
 * Consumido pelo ActionCenter e por /jobs (elimina os dois mappers duplicados
 * e divergentes). Snake_case legado é tolerado só como fallback de leitura —
 * a escrita do BFF é camelCase (contrato packages/contracts/openapi.yaml).
 */
import type { DiffusionPreset, Job, JobArtifact } from "@/types/studio";

function asNumber(v: unknown, fallback: number): number {
	return typeof v === "number" && Number.isFinite(v) ? v : fallback;
}

function asString(v: unknown, fallback: string): string {
	return typeof v === "string" ? v : fallback;
}

function asBool(v: unknown, fallback: boolean): boolean {
	return typeof v === "boolean" ? v : fallback;
}

/** Campo camelCase do BFF com fallback snake_case legado. */
function pick(
	p: Record<string, unknown>,
	camel: string,
	snake: string,
): unknown {
	return p[camel] ?? p[snake];
}

export interface DiffusionPresetFromJob extends Partial<DiffusionPreset> {
	/** UUID de pesos (fine-tune) — só faz sentido no rerun; resume usa o checkpoint. */
	weights?: string | null;
	/** Nome sugerido do adaptador — idem. */
	outputName?: string | null;
}

/**
 * Converte job.params no preset completo da Forja. Não dropa nenhum campo
 * persistido pelo BFF (submit_diffusion_job): inclui lrScheduler,
 * lrWarmupSteps, enableBucket, sample*, weights e outputName.
 */
export function paramsToPreset(job: Job): DiffusionPresetFromJob | undefined {
	if (!job.params) return undefined;
	const p = job.params as Record<string, unknown>;
	const samplePrompt = pick(p, "samplePrompt", "sample_prompt");
	return {
		baseModel: (asString(p.baseModel, "") ||
			asString(p.base_model, "") ||
			job.model) as DiffusionPreset["baseModel"],
		triggerWord: asString(pick(p, "triggerWord", "trigger_word"), ""),
		rank: asNumber(p.rank, 16),
		alpha: asNumber(p.alpha, 16),
		resolution: asNumber(p.resolution, 1024),
		gradientAccumulationSteps: asNumber(
			pick(p, "gradientAccumulationSteps", "gradient_accumulation_steps"),
			1,
		),
		optimizer: asString(
			p.optimizer,
			"adamw8bit",
		) as DiffusionPreset["optimizer"],
		lrScheduler: asString(
			pick(p, "lrScheduler", "lr_scheduler"),
			"cosine",
		) as DiffusionPreset["lrScheduler"],
		lrWarmupSteps: asNumber(pick(p, "lrWarmupSteps", "lr_warmup_steps"), 0),
		mixedPrecision: asString(
			pick(p, "mixedPrecision", "mixed_precision"),
			"fp16",
		) as DiffusionPreset["mixedPrecision"],
		quantization: asString(
			p.quantization,
			"4bit",
		) as DiffusionPreset["quantization"],
		controlDatasetId: asString(
			pick(p, "controlDatasetId", "control_dataset_id"),
			"",
		),
		cacheTextEmbeddings: asBool(
			pick(p, "cacheTextEmbeddings", "cache_text_embeddings"),
			false,
		),
		enableBucket: asBool(pick(p, "enableBucket", "enable_bucket"), true),
		checkpointInterval: asNumber(
			pick(p, "checkpointInterval", "checkpoint_interval"),
			1,
		),
		epochs: asNumber(p.epochs, 10),
		batchSize: asNumber(pick(p, "batchSize", "batch_size"), 1),
		learningRate:
			p.learningRate != null
				? String(p.learningRate)
				: p.learning_rate != null
					? String(p.learning_rate)
					: "0.0001",
		enableSamples: asBool(
			pick(p, "enableSamples", "enable_samples"),
			typeof samplePrompt === "string" && samplePrompt.length > 0,
		),
		samplePrompt: asString(samplePrompt, ""),
		sampleInterval: asNumber(pick(p, "sampleInterval", "sample_interval"), 1),
		sampleSeed:
			p.sampleSeed != null
				? String(p.sampleSeed)
				: p.sample_seed != null
					? String(p.sample_seed)
					: "42",
		weights: typeof p.weights === "string" ? p.weights : null,
		outputName: typeof p.outputName === "string" ? p.outputName : null,
	};
}

/** Extrai epoch_offset do path do artefato (epoch_N) ou do job. */
export function diffusionEpochOffset(job: Job, art: JobArtifact): number {
	const match = art.path.match(/epoch_(\d+)/);
	if (match) return parseInt(match[1], 10);
	return job.epoch ?? 0;
}

export interface DiffusionResumePayload {
	datasetId: string | null;
	epochOffset: number;
	resumeCheckpoint?: { id: string; name: string; epoch: number };
	initialPreset?: DiffusionPresetFromJob;
}

/**
 * Payload canônico da chave `hephaestus_diffusion_resume` para
 * "Continuar Treino" (retomada por pesos LoRA + epoch_offset).
 */
export function buildDiffusionResume(
	job: Job,
	art: JobArtifact,
): DiffusionResumePayload {
	const epochOffset = diffusionEpochOffset(job, art);
	const checkpointName = art.path.split("/").pop() || "checkpoint.safetensors";
	const initialPreset = paramsToPreset(job);
	if (initialPreset) {
		// A retomada continua dos pesos do checkpoint — weights/outputName do job
		// original não se aplicam (evita sobrescrever o adaptador de origem).
		initialPreset.weights = undefined;
		initialPreset.outputName = undefined;
	}
	return {
		datasetId: job.datasetId,
		epochOffset,
		resumeCheckpoint: { id: art.id, name: checkpointName, epoch: epochOffset },
		initialPreset,
	};
}

/** Payload canônico da chave `hephaestus_diffusion_resume` para "Repetir Treino". */
export function buildDiffusionRerun(job: Job): DiffusionResumePayload {
	return {
		datasetId: job.datasetId,
		epochOffset: 0,
		initialPreset: paramsToPreset(job),
	};
}

/**
 * Payload canônico da chave `heph_rerun_yolo` (consumida por /treino).
 * Espelha os params YOLO persistidos pelo BFF (PrepareSpec) — sem `lrf`
 * (o wire YOLO não tem esse campo) e com seed/weights/outputName.
 */
export interface YoloRerunPayload {
	datasetId: string | null;
	model: string;
	epochs: number;
	batch: number;
	imgsz: number;
	lr0: number;
	optimizer: string;
	mosaic: boolean;
	mixupFlip: boolean;
	seed?: number | null;
	weights?: string | null;
	outputName?: string | null;
}

export function buildYoloRerun(job: Job): YoloRerunPayload {
	const p = (job.params ?? {}) as Record<string, unknown>;
	const augment = (p.augment ?? {}) as Record<string, unknown>;
	return {
		datasetId: job.datasetId,
		model: typeof p.model === "string" ? p.model : job.model,
		epochs: asNumber(p.epochs, 100),
		batch: asNumber(p.batch, 16),
		imgsz: asNumber(p.imgsz, 640),
		lr0: asNumber(p.lr0, 0.01),
		optimizer: asString(p.optimizer, "AdamW"),
		mosaic: asBool(augment.mosaic, true),
		mixupFlip: asBool(augment.mixupFlip, false),
		seed: typeof p.seed === "number" ? p.seed : null,
		weights: typeof p.weights === "string" ? p.weights : null,
		outputName: typeof p.outputName === "string" ? p.outputName : null,
	};
}

/** Subconjunto do form YOLO pré-preenchível via rerun (lr0 em string, como no form). */
export interface YoloFormPreset {
	model?: string;
	epochs?: number;
	batch?: number;
	imgsz?: number;
	lr0?: string;
	optimizer?: string;
	augment?: { mosaic: boolean; mixupFlip: boolean };
}

export interface ParsedYoloRerun {
	datasetId: string;
	params: YoloFormPreset;
	weightsId: string;
	outputName: string;
}

/** Lê e valida o conteúdo de `heph_rerun_yolo` (null = ausente/inválido). */
export function parseYoloRerun(stored: string | null): ParsedYoloRerun | null {
	if (!stored) return null;
	let parsed: Record<string, unknown>;
	try {
		parsed = JSON.parse(stored) as Record<string, unknown>;
	} catch {
		return null;
	}
	if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
		return null;
	}
	const nestedAugment = (parsed.augment ?? {}) as Record<string, unknown>;
	return {
		datasetId: typeof parsed.datasetId === "string" ? parsed.datasetId : "",
		params: {
			...(typeof parsed.model === "string" ? { model: parsed.model } : {}),
			...(typeof parsed.epochs === "number" ? { epochs: parsed.epochs } : {}),
			...(typeof parsed.batch === "number" ? { batch: parsed.batch } : {}),
			...(typeof parsed.imgsz === "number" ? { imgsz: parsed.imgsz } : {}),
			...(parsed.lr0 != null ? { lr0: String(parsed.lr0) } : {}),
			...(typeof parsed.optimizer === "string"
				? { optimizer: parsed.optimizer }
				: {}),
			augment: {
				mosaic: asBool(nestedAugment.mosaic ?? parsed.mosaic, true),
				mixupFlip: asBool(nestedAugment.mixupFlip ?? parsed.mixupFlip, false),
			},
		},
		weightsId: typeof parsed.weights === "string" ? parsed.weights : "",
		outputName: typeof parsed.outputName === "string" ? parsed.outputName : "",
	};
}
