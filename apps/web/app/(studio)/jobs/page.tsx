"use client";

import { useRouter, useSearchParams } from "next/navigation";
import {
	Suspense,
	useCallback,
	useEffect,
	useMemo,
	useRef,
	useState,
} from "react";
import {
	IconCheck,
	IconDatabase,
	IconDownload,
	IconLayers,
	IconRefresh,
	IconSparkles,
	IconTarget,
	IconTrash,
} from "@/components/icons";
import { AutolabelReviewModal } from "@/components/studio/AutolabelReviewModal";
import { AutotrackerReviewModal } from "@/components/studio/AutotrackerReviewModal";
import {
	ConvergenceChart,
} from "@/components/studio/ConvergenceChart";
import { JobCleanupDialog } from "@/components/studio/JobCleanupDialog";
import { JobLogViewer } from "@/components/studio/JobLogViewer";
import { JobProgressLive } from "@/components/studio/JobProgressLive";
import { JobSamplesGallery } from "@/components/studio/JobSamplesGallery";
import {
	COMPARE_MAX_JOBS,
	COMPARE_MIN_JOBS,
	JobAlertsModal,
	JobArtifactsList,
	JobHeroHeader,
	JobLineage,
	JobMetricsChips,
	JobsHeader,
	JobsSidebar,
} from "@/components/studio/jobs";
import { Button } from "@/components/ui/Button";
import { ConfirmDialog } from "@/components/ui/ConfirmDialog";
import { Spinner } from "@/components/ui/Spinner";
import { showToast } from "@/components/ui/Toast";
import { useJobAlerts } from "@/hooks/useJobAlerts";
import { usePartialCaptions } from "@/hooks/usePartialCaptions";
import { useJobLifecycle } from "@/hooks/useJobLifecycle";
import { useJobMetricSeries } from "@/hooks/useJobMetricSeries";
import { useJobTelemetry } from "@/hooks/useJobTelemetry";
import { ApiError } from "@/lib/api";
import { formatDuration } from "@/lib/format";
import { jobCapabilities } from "@/lib/jobCapabilities";
import { trainingMetrics } from "@/lib/jobMetrics";
import {
	downloadJobArtifactsZip,
	getJobArtifacts,
	getJobMetrics,
	listJobs,
} from "@/lib/jobs";
import {
	buildDiffusionRerun,
	buildDiffusionResume,
	buildYoloRerun,
} from "@/lib/paramsToPreset";
import type {
	Job,
	JobArtifact,
	JobMetrics as JobMetricsType,
	JobStatus,
	MetricPointWithKey,
} from "@/types/studio";

const POLL_INTERVAL = 3000;


/** Status que indicam job em andamento (ativação de polling). */
const ACTIVE_STATUSES: JobStatus[] = [
	"preparing",
	"queued",
	"dispatched",
	"running",
	"cancelling",
];

function isActive(status: JobStatus): boolean {
	return ACTIVE_STATUSES.includes(status);
}

function JobsPageContent() {
	const router = useRouter();
	const searchParams = useSearchParams();
	const [jobs, setJobs] = useState<Job[]>([]);
	const [loading, setLoading] = useState(true);
	const [refreshing, setRefreshing] = useState(false);
	const [error, setError] = useState<string | null>(null);
	const [selectedJobId, setSelectedJobId] = useState<string | null>(null);
	const [metrics, setMetrics] = useState<Record<string, JobMetricsType[]>>({});
	const [artifacts, setArtifacts] = useState<Record<string, JobArtifact[]>>({});
	const [pendingArtifactFocus, setPendingArtifactFocus] = useState<
		string | null
	>(null);
	const [zipBusy, setZipBusy] = useState(false);
	const [cleanupOpen, setCleanupOpen] = useState(false);
	const [compareIds, setCompareIds] = useState<Set<string>>(new Set());
	const [alertsModalOpen, setAlertsModalOpen] = useState(false);
	const {
		abortTarget,
		setAbortTarget,
		abortBusy,
		handleAbort,
		deleteTarget,
		setDeleteTarget,
		deleteBusy,
		handleDeleteJob,
		applyBusy,
		applyOverwrite,
		setApplyOverwrite,
		handleApplyBoxes,
		handleApplyCaptions,
		handleDownloadArtifact,
	} = useJobLifecycle({
		onSuccess: () => fetchJobs(),
		onDeleted: (jobId) => {
			if (selectedJobId === jobId) {
				selectJob(null);
			}
		},
		onNavigateDataset: (datasetId) => router.push(`/datasets/${datasetId}`),
	});
	const [reviewJob, setReviewJob] = useState<Job | null>(null);
	const [autotrackerReviewJob, setAutotrackerReviewJob] = useState<Job | null>(
		null,
	);

	const pollRef = useRef<ReturnType<typeof setInterval> | null>(null);

	// Lê query param ?job=jobId para auto-seleção (deep link / navegação do ActionCenter)
	useEffect(() => {
		const qJob = searchParams.get("job");
		if (qJob && qJob !== selectedJobId) {
			setSelectedJobId(qJob);
		}
	}, [searchParams, selectedJobId]);

	// Estado derivado: modo foco = /jobs?job=ID&focus=1
	const focusMode = searchParams.get("focus") === "1" && Boolean(selectedJobId);

	/** Sincroniza seleção de job com a URL (substitui setSelectedJobId direto). */
	const selectJob = useCallback(
		(id: string | null) => {
			setSelectedJobId(id);
			const params = new URLSearchParams(window.location.search);
			if (id) {
				params.set("job", id);
			} else {
				params.delete("job");
			}
			params.delete("focus"); // sair de foco ao trocar/clear job
			const qs = params.toString();
			router.replace(qs ? `/jobs?${qs}` : "/jobs", { scroll: false });
		},
		[router],
	);

	/** Atualiza o parâmetro ?focus=1 sem trocar o job. */
	const setFocus = useCallback(
		(on: boolean, id?: string | null) => {
			const target = id ?? selectedJobId;
			if (!target) return;
			if (target !== selectedJobId) setSelectedJobId(target);
			const params = new URLSearchParams(window.location.search);
			params.set("job", target);
			if (on) {
				params.set("focus", "1");
			} else {
				params.delete("focus");
			}
			const qs = params.toString();
			router.replace(`/jobs?${qs}`, { scroll: false });
		},
		[router, selectedJobId],
	);

	// Resetar applyOverwrite ao trocar de job
	// biome-ignore lint/correctness/useExhaustiveDependencies: reset intencional on-change — re-executa quando selectedJobId muda sem ler seu valor; ler seria artificial
	useEffect(() => {
		setApplyOverwrite(false);
	}, [selectedJobId]);

	const fetchJobs = useCallback(async (isManual = false) => {
		if (isManual) setRefreshing(true);
		try {
			const data = await listJobs();
			setJobs(data.items);
			setError(null);
		} catch (err) {
			if (
				err instanceof ApiError &&
				(err.code === "unauthorized" || err.status === 401)
			) {
				return;
			}
			setError("Falha ao carregar lista de jobs.");
		} finally {
			setLoading(false);
			if (isManual) setRefreshing(false);
		}
	}, []);

	useEffect(() => {
		fetchJobs();
	}, [fetchJobs]);

	// Polling a cada 3s se houver jobs ativos
	useEffect(() => {
		if (pollRef.current) {
			clearInterval(pollRef.current);
			pollRef.current = null;
		}

		if (jobs.some((j) => isActive(j.status))) {
			pollRef.current = setInterval(() => {
				if (
					typeof document !== "undefined" &&
					document.visibilityState === "hidden"
				) {
					return;
				}
				void fetchJobs();
			}, POLL_INTERVAL);
		}

		const handleVisibilityChange = () => {
			if (
				document.visibilityState === "visible" &&
				jobs.some((j) => isActive(j.status))
			) {
				void fetchJobs();
			}
		};
		document.addEventListener("visibilitychange", handleVisibilityChange);

		return () => {
			if (pollRef.current) {
				clearInterval(pollRef.current);
				pollRef.current = null;
			}
			document.removeEventListener("visibilitychange", handleVisibilityChange);
		};
	}, [jobs, fetchJobs]);

	// Jobs agrupados: ativos primeiro, depois terminais — ambos em ordem cronológica (mais recente primeiro)
	const { activeJobs, terminalJobs } = useMemo(() => {
		const sorted = [...jobs].sort(
			(a, b) =>
				new Date(b.createdAt).getTime() - new Date(a.createdAt).getTime(),
		);
		return {
			activeJobs: sorted.filter((j) => isActive(j.status)),
			terminalJobs: sorted.filter((j) => !isActive(j.status)),
		};
	}, [jobs]);

	const selectedJob = useMemo(() => {
		if (selectedJobId) {
			return (
				jobs.find((j) => j.id === selectedJobId) ??
				activeJobs[0] ??
				terminalJobs[0] ??
				null
			);
		}
		// Auto-focus no job ativo mais recente, ou no mais recente terminal
		return activeJobs[0] ?? terminalJobs[0] ?? null;
	}, [jobs, selectedJobId, activeJobs, terminalJobs]);

	const isSelectedActive = selectedJob ? isActive(selectedJob.status) : false;
	const hasPartialCaptions = usePartialCaptions(selectedJob);

	/** Alternância da seleção múltipla de comparação de runs (fatia 4c, máx. 4). */
	const toggleCompare = useCallback((jobId: string) => {
		setCompareIds((prev) => {
			const next = new Set(prev);
			if (next.has(jobId)) {
				next.delete(jobId);
			} else if (next.size < COMPARE_MAX_JOBS) {
				next.add(jobId);
			}
			return next;
		});
	}, []);

	// Hook de séries brutas (fatia 4a) com carga inicial
	const isYoloSelected = selectedJob?.kind.startsWith("yolo");
	const requestedKeys = useMemo(
		() =>
			isYoloSelected
				? ["box_loss", "cls_loss", "dfl_loss", "mAP50", "mAP50-95"]
				: ["loss", "lr", "grad_norm"],
		[isYoloSelected],
	);

	const {
		points: currentJobPoints,
		maxSeq: currentJobMaxSeq,
		appendPoints: appendCurrentJobPoints,
	} = useJobMetricSeries(selectedJob?.id, {
		enabled: !!selectedJob,
		maxPoints: 2500, // ~2x largura em px
		keys: requestedKeys,
	});

	// SSE via useJobTelemetry: conecta ao stream e faz append de deltas sem refetch
	// (no polling de fallback busca o delta via GET /metrics?afterSeq=)
	// Alertas (fatia 3c-UI): snapshot inicial via useJobAlerts; evento SSE `alerts`
	// substitui a lista inteira (idempotente) por applyAlertsSnapshot.
	const { alerts: selectedJobAlerts, applySnapshot: applyAlertsSnapshot } =
		useJobAlerts(selectedJob?.id);
	const telemetry = useJobTelemetry(isSelectedActive ? selectedJob?.id : null, {
		onMetricPoints: useCallback(
			(pts: MetricPointWithKey[], maxSeq: number) => {
				appendCurrentJobPoints(pts, maxSeq || undefined);
			},
			[appendCurrentJobPoints],
		),
		onAlerts: applyAlertsSnapshot,
		metricAfterSeq: currentJobMaxSeq,
		metricKeys: requestedKeys,
	});
	// AC-006-B: pontos reais de métrica de treino do job selecionado
	// (linhas de status/boot do engine ficam só no log, nunca no gráfico/chips)
	const selectedTrainingMetrics = useMemo(
		() => (selectedJob ? trainingMetrics(metrics[selectedJob.id] ?? []) : []),
		[selectedJob, metrics],
	);

	// AC-002: capacidades do job selecionado para gates de UI
	const caps = useMemo(
		() => (selectedJob ? jobCapabilities(selectedJob) : null),
		[selectedJob],
	);

	// Carregar métricas e artefatos quando o selectedJob mudar
	useEffect(() => {
		const targetId = selectedJob?.id;
		if (!targetId) return;
		const ctrl = new AbortController();

		async function loadDetail() {
			try {
				const [m, a] = await Promise.all([
					getJobMetrics(targetId!),
					getJobArtifacts(targetId!),
				]);
				if (!ctrl.signal.aborted) {
					setMetrics((prev) => ({ ...prev, [targetId!]: m.items }));
					setArtifacts((prev) => ({ ...prev, [targetId!]: a.items }));
				}
			} catch {
				// Detalhe é best-effort
			}
		}

		loadDetail();
		return () => ctrl.abort();
	}, [selectedJob?.id]);

	// Re-fetch métricas e artefatos quando o job selecionado está ativo e jobs atualiza
	useEffect(() => {
		const targetId = selectedJob?.id;
		if (!targetId) return;
		const job = jobs.find((j) => j.id === targetId);
		if (!job || !isActive(job.status)) return;

		const ctrl = new AbortController();

		async function refreshDetail() {
			try {
				const [m, a] = await Promise.all([
					getJobMetrics(targetId!),
					getJobArtifacts(targetId!),
				]);
				if (!ctrl.signal.aborted) {
					setMetrics((prev) => ({ ...prev, [targetId!]: m.items }));
					setArtifacts((prev) => ({ ...prev, [targetId!]: a.items }));
				}
			} catch {
				// Detalhe é best-effort
			}
		}

		refreshDetail();
		return () => ctrl.abort();
	}, [selectedJob?.id, jobs]);



	/** Rola até o artefato (checkpoint) destacado a partir da seção de Linhagem. */
	function handleFocusLineageArtifact(ownerJobId: string, artifactId: string) {
		if (ownerJobId !== selectedJobId) {
			selectJob(ownerJobId);
		}
		setPendingArtifactFocus(artifactId);
	}

	// biome-ignore lint/correctness/useExhaustiveDependencies: roda a cada troca de job/artefatos para encontrar o elemento assim que ele existir no DOM
	useEffect(() => {
		if (!pendingArtifactFocus) return;
		const el = document.getElementById(`artifact-${pendingArtifactFocus}`);
		if (!el) return; // aguarda o job alvo ser selecionado/renderizado
		el.scrollIntoView({ behavior: "smooth", block: "center" });
		el.classList.add("ring-2", "ring-brand-400");
		const timer = setTimeout(
			() => el.classList.remove("ring-2", "ring-brand-400"),
			1600,
		);
		setPendingArtifactFocus(null);
		return () => clearTimeout(timer);
	}, [pendingArtifactFocus, selectedJobId, artifacts]);
	function handleResumeFromCheckpoint(job: Job, art: JobArtifact) {
		const resumeData = buildDiffusionResume(job, art);

		try {
			sessionStorage.setItem(
				"hephaestus_diffusion_resume",
				JSON.stringify(resumeData),
			);
		} catch {
			// Best-effort
		}

		router.push("/difusao");
	}

	function handleRerunJob(job: Job) {
		if (job.engine === "diffusion") {
			const resumeData = buildDiffusionRerun(job);

			try {
				sessionStorage.setItem(
					"hephaestus_diffusion_resume",
					JSON.stringify(resumeData),
				);
			} catch {
				// Best-effort
			}

			router.push("/difusao");
		} else if (job.engine === "yolo") {
			// Rerun YOLO: mesma chave do ActionCenter, consumida por /treino.
			try {
				sessionStorage.setItem(
					"heph_rerun_yolo",
					JSON.stringify(buildYoloRerun(job)),
				);
			} catch {
				// Best-effort
			}

			router.push("/treino");
		} else {
			router.push("/treino");
		}
	}

	async function handleDownloadArtifactsZip(job: Job) {
		if (zipBusy) return;
		setZipBusy(true);
		try {
			await downloadJobArtifactsZip(job.id);
		} catch (e) {
			showToast(
				e instanceof Error
					? `Falha ao baixar ZIP: ${e.message}`
					: "Falha ao baixar ZIP",
				"error",
			);
		} finally {
			setZipBusy(false);
		}
	}

	function handleDownloadJobConfig(job: Job) {
		const jobArts = artifacts[job.id] || [];
		const configArt = jobArts.find(
			(a) => a.kind === "config" || a.path.endsWith("training_config.json"),
		);
		if (configArt) {
			handleDownloadArtifact(job.id, configArt);
			return;
		}

		// Fallback honesto: o artefato kind:"config" não existe (job pode ter
		// falhado antes do upload) — rotula a origem para nunca passar por
		// config real da engine.
		const configData = {
			_source: "job.params" as const,
			...(job.params
				? job.params
				: {
						jobId: job.id,
						engine: job.engine,
						model: job.model,
						datasetId: job.datasetId,
						epoch: job.epoch,
						metrics: job.metrics,
						createdAt: job.createdAt,
					}),
		};

		const blob = new Blob([JSON.stringify(configData, null, 2)], {
			type: "application/json",
		});
		const url = URL.createObjectURL(blob);
		const a = document.createElement("a");
		a.href = url;
		a.download = `job_params_${job.id.slice(0, 8)}.json`;
		document.body.appendChild(a);
		a.click();
		document.body.removeChild(a);
		URL.revokeObjectURL(url);
		showToast(
			"Artefato de config ausente — JSON gerado a partir de job.params (não é a config real da engine).",
			"info",
		);
	}

	const totalCount = activeJobs.length + terminalJobs.length;

	return (
		<div className="mx-auto max-w-[1600px] w-full px-4 py-5 md:px-6 lg:px-8 space-y-6">
			{/* ═══════════════════════════════════════════════
          HEADER — EXECUÇÕES
          ═══════════════════════════════════════════════ */}
			<JobsHeader
				totalCount={totalCount}
				refreshing={refreshing}
				onCleanup={() => setCleanupOpen(true)}
				onRefresh={() => void fetchJobs(true)}
			/>

			{/* ═══════════════════════════════════════════════
          WORKSPACE DE 2 COLUNAS: LISTA + DETALHE
          ═══════════════════════════════════════════════ */}
			{!error && (
				<div className="flex flex-col md:flex-row items-start gap-6">
					{/* Coluna 1: Lista de Execuções — oculta no modo foco */}
					{!focusMode && (
						<div className="w-full md:w-80 lg:w-96 shrink-0 space-y-3">
							{/* Barra de comparação de runs (fatia 4c) — some sem seleção */}
							{compareIds.size > 0 && (
								<div className="glass-card flex items-center justify-between gap-2 rounded-xl border border-brand-500/30 bg-brand-500/[0.08] px-3 py-2">
									<span className="font-mono text-2xs text-zinc-300">
										{compareIds.size} job{compareIds.size > 1 ? "s" : ""}{" "}
										selecionado{compareIds.size > 1 ? "s" : ""}
										{compareIds.size < COMPARE_MIN_JOBS
											? ` · selecione mais ${COMPARE_MIN_JOBS - compareIds.size}`
											: ""}
									</span>
									<div className="flex items-center gap-2">
										<button
											type="button"
											onClick={() => setCompareIds(new Set())}
											className="font-mono text-2xs text-zinc-400 hover:text-zinc-200 underline underline-offset-2 cursor-pointer"
										>
											Limpar
										</button>
										<Button
											type="button"
											variant="secondary"
											size="sm"
											disabled={compareIds.size < COMPARE_MIN_JOBS}
											leftIcon={<IconLayers className="size-3" />}
											onClick={() =>
												router.push(
													`/jobs/compare?ids=${Array.from(compareIds).join(",")}`,
												)
											}
										>
											Comparar
										</Button>
									</div>
								</div>
							)}
							<JobsSidebar
								activeJobs={activeJobs}
								terminalJobs={terminalJobs}
								selectedJobId={selectedJob?.id ?? null}
								loading={loading}
								totalCount={totalCount}
								onSelectJob={selectJob}
								onRerunJob={handleRerunJob}
								selectedJobAlerts={selectedJobAlerts}
								onOpenSelectedAlerts={() => setAlertsModalOpen(true)}
								compareIds={compareIds}
								onToggleCompare={toggleCompare}
							/>
						</div>
					)}

					{/* Coluna 2: Painel de Detalhe (flex-1 min-w-0) */}
					<section className="w-full flex-1 min-w-0 space-y-4">
						{selectedJob ? (
							<div className="space-y-4">
								{/* Cabeçalho do Job Selecionado */}
								<JobHeroHeader
									selectedJob={selectedJob}
									focusMode={focusMode}
									isActiveJob={isActive(selectedJob.status)}
									selectedJobId={selectedJobId}
									hasActiveJobs={activeJobs.length > 0}
									activeJobId={activeJobs[0]?.id}
									alerts={selectedJobAlerts}
									onOpenAlerts={() => setAlertsModalOpen(true)}
									onSetFocus={setFocus}
									onDelete={setDeleteTarget}
									onSelectJob={selectJob}
								/>
								<JobAlertsModal
									open={alertsModalOpen}
									onClose={() => setAlertsModalOpen(false)}
									alerts={selectedJobAlerts}
								/>

								{/* Job Hero Card */}
								<div className="glass-card rounded-2xl p-5 space-y-4 border border-white/10">
									<div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3">
										<div>
											<div className="flex items-center gap-2">
												<span className="font-display text-base font-semibold text-zinc-100">
													{selectedJob.model}
												</span>
												<span className="font-mono text-2xs text-zinc-400">
													· {selectedJob.kind} · {selectedJob.engine}
												</span>
											</div>
											<div className="mt-1 flex flex-wrap items-center gap-3 font-mono text-2xs text-zinc-400">
												<span>
													Duração:{" "}
													{formatDuration(
														selectedJob.createdAt,
														selectedJob.finishedAt,
													)}
												</span>
												{selectedJob.epoch != null && (
													<span>Epoch {selectedJob.epoch}</span>
												)}
												{isActive(selectedJob.status) && (
													<span className="text-brand-300 font-semibold">
														{Math.round((selectedJob.progress ?? 0) * 100)}%
													</span>
												)}
												{selectedJob.orchestratorName ? (
													<span className="flex items-center gap-1.5 text-zinc-300">
														<span className="text-zinc-500">·</span>
														<span>
															Nó:{" "}
															<strong className="font-semibold text-zinc-200">
																{selectedJob.orchestratorName}
															</strong>
															{selectedJob.orchestratorKind
																? ` (${selectedJob.orchestratorKind})`
																: ""}
														</span>
														{selectedJob.orchestratorFallback && (
															<span
																className="inline-flex items-center rounded border border-status-alert/30 bg-status-alert/10 px-1.5 py-0.5 text-3xs text-amber-400 font-medium"
																title="Job sofreu fallback automático após timeout no nó solicitado"
															>
																fallback
															</span>
														)}
													</span>
												) : selectedJob.status === "preparing" ? (
													<span className="flex items-center gap-1.5 text-zinc-500">
														<span>·</span>
														<span>Preparando pacote</span>
													</span>
												) : selectedJob.status === "queued" ||
													selectedJob.status === "dispatched" ? (
													<span className="flex items-center gap-1.5 text-zinc-500">
														<span>·</span>
														<span>Aguardando nó</span>
													</span>
												) : null}
												{selectedJob.gpuDevice && (
													<span className="flex items-center gap-1.5 text-zinc-300">
														<span className="text-zinc-500">·</span>
														<span>
															GPU:{" "}
															<strong className="font-semibold text-brand-300 font-mono" title={selectedJob.gpuDevice}>
																{selectedJob.gpuDevice.length > 12 ? `${selectedJob.gpuDevice.slice(0, 8)}…` : selectedJob.gpuDevice}
															</strong>
														</span>
													</span>
												)}
											</div>
										</div>

										<div className="flex items-center gap-2 font-mono text-2xs text-zinc-400">
											<span title={selectedJob.id}>
												ID: {selectedJob.id.slice(0, 8)}…
											</span>
											{selectedJob.datasetId && (
												<Button
													type="button"
													variant="secondary"
													size="sm"
													onClick={() =>
														router.push(`/datasets/${selectedJob.datasetId}`)
													}
													leftIcon={<IconDatabase className="size-3" />}
												>
													Dataset
												</Button>
											)}
										</div>
									</div>

									{/* Telemetria Unificada ao Vivo para jobs ativos (ADR-0021) */}
									{isActive(selectedJob.status) && (
										<div className="pt-2">
											<JobProgressLive
												jobKind={selectedJob.kind}
												phase={
													telemetry.phase ||
													selectedJob.phase ||
													selectedJob.status
												}
												phaseMessage={
													telemetry.phaseMessage || selectedJob.phaseMessage
												}
												progress={
													telemetry.progress || selectedJob.progress || 0
												}
												vramUsedGb={
													telemetry.vramUsedGb ?? selectedJob.vramUsedGb
												}
												vramReservedGb={
													telemetry.vramReservedGb ?? selectedJob.vramReservedGb
												}
												stepTimeSeconds={telemetry.stepTimeSeconds}
												speed={telemetry.speed}
												etaSeconds={telemetry.etaSeconds}
												etaFormatted={telemetry.etaFormatted}
												step={telemetry.step ?? selectedJob.step}
												totalSteps={
													telemetry.totalSteps ?? selectedJob.totalSteps
												}
												epoch={telemetry.epoch ?? selectedJob.epoch}
												totalEpochs={
													telemetry.totalEpochs ?? selectedJob.totalEpochs
												}
												isLive={telemetry.isLive}
												isFinished={telemetry.isFinished}
											/>
										</div>
									)}

									{/* Métricas da Execução & Curvas de Convergência — regido por caps (AC-002) */}
									{caps &&
										caps.convergenceChart &&
										selectedTrainingMetrics.length > 0 && (
											<div className="space-y-4 pt-3 border-t border-white/10">
												<ConvergenceChart
													points={currentJobPoints}
													metrics={selectedTrainingMetrics}
													jobKind={selectedJob.kind}
													totalEpochs={selectedJob.epoch || 100}
													isJobActive={selectedJob.status === "running"}
												/>

												<JobMetricsChips
													metrics={selectedTrainingMetrics}
													metricChipsType={caps.metricChips}
													step={selectedJob.step}
													progress={selectedJob.progress}
												/>
											</div>
										)}

									{/* Chip único "Imagens processadas" para autolabel/autotracker (AC-002) */}
									{caps && caps.metricChips === "progress" && (
										<JobMetricsChips
											metrics={selectedTrainingMetrics}
											metricChipsType="progress"
											step={selectedJob.step}
											progress={selectedJob.progress}
										/>
									)}

									{/* Artefatos e Amostras Geradas */}
									{artifacts[selectedJob.id] &&
										artifacts[selectedJob.id].length > 0 && (
											<div className="space-y-4 pt-3 border-t border-white/10">
												{/* Galeria de Amostras — gated por caps.samplesGallery (AC-002) */}
												{caps && caps.samplesGallery && (
													<JobSamplesGallery
														jobId={selectedJob.id}
														artifacts={artifacts[selectedJob.id]}
														onDownload={(jId, art) =>
															handleDownloadArtifact(jId, art)
														}
													/>
												)}

												{/* Outros Artefatos Gerados */}
												<JobArtifactsList
													job={selectedJob}
													artifacts={artifacts[selectedJob.id]}
													onDownload={handleDownloadArtifact}
													onResume={handleResumeFromCheckpoint}
												/>
											</div>
										)}

									<JobLineage
										jobId={selectedJob.id}
										loadedArtifactJobIds={
											new Set(
												Object.keys(artifacts).filter(
													(jid) => (artifacts[jid]?.length ?? 0) > 0,
												),
											)
										}
										onSelectJob={selectJob}
										onSelectDataset={(datasetId) =>
											router.push(`/datasets/${datasetId}`)
										}
										onFocusArtifact={handleFocusLineageArtifact}
									/>

									{/* Ações do Job */}
									<div className="flex items-center justify-between gap-3 pt-2">
										{caps &&
											caps.applyAction &&
											selectedJob.kind === "autotracker" &&
											selectedJob.status === "done" && (
												<div className="flex items-center gap-3 flex-wrap">
													<Button
														type="button"
														variant="primary"
														size="sm"
														onClick={() => setAutotrackerReviewJob(selectedJob)}
													>
														<IconSparkles className="size-3.5 text-brand-400" />
														<span>Revisar e Aplicar</span>
													</Button>

													<div className="flex items-center gap-2 border-l border-white/10 pl-3">
														<label className="flex items-center gap-2 text-xs text-zinc-400 cursor-pointer">
															<input
																type="checkbox"
																checked={applyOverwrite}
																onChange={(e) =>
																	setApplyOverwrite(e.target.checked)
																}
																className="rounded border-zinc-700 bg-zinc-800 text-brand-500 focus:ring-brand-500/40"
															/>
															<span>Sobrescrever</span>
														</label>
														<Button
															type="button"
															variant="secondary"
															size="sm"
															disabled={applyBusy}
															loading={applyBusy}
															onClick={() => handleApplyBoxes(selectedJob)}
														>
															<IconCheck className="size-3.5 text-zinc-300" />
															<span>
																{applyBusy ? "Aplicando…" : "Aplicação direta"}
															</span>
														</Button>
													</div>
												</div>
											)}

										{caps &&
											caps.applyAction &&
											selectedJob.kind === "autolabel" &&
											selectedJob.status === "done" && (
												<div className="flex items-center gap-3 flex-wrap">
													<Button
														type="button"
														variant="primary"
														size="sm"
														onClick={() => setReviewJob(selectedJob)}
													>
														<IconSparkles className="size-3.5 text-brand-400" />
														<span>Revisar Legendas (Curadoria)</span>
													</Button>

													<div className="flex items-center gap-2 border-l border-white/10 pl-3">
														<label className="flex items-center gap-2 text-xs text-zinc-400 cursor-pointer">
															<input
																type="checkbox"
																checked={applyOverwrite}
																onChange={(e) =>
																	setApplyOverwrite(e.target.checked)
																}
																className="rounded border-zinc-700 bg-zinc-800 text-brand-500 focus:ring-brand-500/40"
															/>
															<span>Sobrescrever</span>
														</label>
														<Button
															type="button"
															variant="secondary"
															size="sm"
															disabled={applyBusy}
															loading={applyBusy}
															onClick={() => handleApplyCaptions(selectedJob)}
															title="Aplica todas as legendas geradas sem inspeção prévia"
														>
															<IconCheck className="size-3.5 text-zinc-400" />
															<span>
																{applyBusy
																	? "Aplicando…"
																	: "Aplicar Todas Direto"}
															</span>
														</Button>
													</div>
												</div>
											)}

										{caps?.applyAction &&
											selectedJob.kind === "autolabel" &&
											(selectedJob.status === "failed" ||
												selectedJob.status === "cancelled") &&
											hasPartialCaptions && (
												<div className="flex items-center gap-3 flex-wrap">
													<Button
														type="button"
														variant="primary"
														size="sm"
														onClick={() => setReviewJob(selectedJob)}
													>
														<IconSparkles className="size-3.5 text-brand-400" />
														<span>Revisar legendas parciais</span>
													</Button>
												</div>
											)}

										{/* Ações para jobs finalizados de difusão ou YOLO — gated por caps.rerun (AC-002) */}
										{caps &&
											caps.rerun &&
											!isActive(selectedJob.status) &&
											(selectedJob.engine === "diffusion" ||
												selectedJob.engine === "yolo") && (
												<Button
													type="button"
													variant="secondary"
													size="sm"
													onClick={() => handleRerunJob(selectedJob)}
													title="Abrir a Forja pré-carregada com todos os parâmetros deste treino para submeter novamente"
												>
													<IconRefresh className="size-3.5 text-brand-400" />
													<span>Repetir Treino</span>
												</Button>
											)}

										{/* F2: ZIP de todos os artefatos (BFF stored) */}
										{!isActive(selectedJob.status) &&
											(artifacts[selectedJob.id]?.length ?? 0) > 0 && (
												<Button
													type="button"
													variant="secondary"
													size="sm"
													disabled={zipBusy}
													onClick={() =>
														void handleDownloadArtifactsZip(selectedJob)
													}
													title="Baixar todos os artefatos do job (modelo, config, métricas, amostras e logs) em um único ZIP"
												>
													<IconDownload className="size-3.5 text-zinc-400" />
													<span>
														{zipBusy
															? "Montando ZIP…"
															: "Baixar artefatos (.zip)"}
													</span>
												</Button>
											)}

										{selectedJob.engine === "diffusion" && (
											<div className="flex items-center gap-2.5 flex-wrap">
												<Button
													type="button"
													variant="secondary"
													size="sm"
													onClick={() => handleDownloadJobConfig(selectedJob)}
													title="Baixar training_config.json da engine; quando o artefato não existe, gera job_params_<id>.json rotulado com _source job.params"
												>
													<IconDownload className="size-3.5 text-zinc-400" />
													<span>Baixar JSON de Treino</span>
												</Button>

												{artifacts[selectedJob.id] &&
													artifacts[selectedJob.id].some(
														(a) =>
															a.kind === "checkpoint" ||
															a.kind === "model" ||
															a.path.endsWith(".safetensors"),
													) && (
														<Button
															type="button"
															variant="primary"
															size="sm"
															onClick={() => {
																const ckpts = (
																	artifacts[selectedJob.id] || []
																).filter(
																	(a) =>
																		a.kind === "checkpoint" ||
																		a.kind === "model" ||
																		a.path.endsWith(".safetensors"),
																);
																const lastCkpt = ckpts[ckpts.length - 1];
																if (lastCkpt) {
																	handleResumeFromCheckpoint(
																		selectedJob,
																		lastCkpt,
																	);
																}
															}}
															title="Continuar treinamento adicionando épocas a partir do último checkpoint"
														>
															<IconSparkles className="size-3.5 text-sky-400" />
															<span>Continuar Treino</span>
														</Button>
													)}
											</div>
										)}

										{isActive(selectedJob.status) && (
											<Button
												type="button"
												variant="destructive"
												size="sm"
												onClick={() => setAbortTarget(selectedJob)}
											>
												<IconTrash className="size-3.5" />
												<span>Cancelar Execução</span>
											</Button>
										)}

										{!isActive(selectedJob.status) && (
											<Button
												type="button"
												variant="destructive"
												size="sm"
												onClick={() => setDeleteTarget(selectedJob)}
												title="Excluir job e seus artefatos (a galeria de gerações é preservada)"
											>
												<IconTrash className="size-3.5" />
												<span>Excluir</span>
											</Button>
										)}
									</div>
								</div>

								{/* Log Viewer */}
								<JobLogViewer
									job={selectedJob}
									metrics={metrics[selectedJob.id] || []}
									artifacts={artifacts[selectedJob.id] || []}
									livePhase={telemetry.phase}
									livePhaseMessage={telemetry.phaseMessage}
								/>
							</div>
						) : (
							/* Empty State */
							<div className="glass-card flex flex-col items-center gap-3.5 rounded-2xl p-12 text-center border border-white/10">
								<span className="flex size-12 items-center justify-center rounded-xl border border-white/10 bg-white/5 text-zinc-400 backdrop-blur-sm">
									<IconTarget className="size-6 text-brand-400/60" />
								</span>
								<div className="max-w-md space-y-1">
									<h3 className="font-display text-sm font-semibold text-zinc-200">
										Nenhuma execução selecionada
									</h3>
									<p className="text-xs text-zinc-400">
										Selecione uma execução na lista ao lado para ver detalhes,
										métricas e artefatos.
									</p>
								</div>
							</div>
						)}
					</section>
				</div>
			)}

			{/* Confirmação de Abort */}
			<ConfirmDialog
				open={Boolean(abortTarget)}
				title="Cancelar execução do Job"
				body={
					<p className="text-xs text-zinc-300">
						Tem certeza de que deseja interromper o job{" "}
						<strong className="text-white font-mono">
							{abortTarget?.model}
						</strong>{" "}
						({abortTarget?.id.slice(0, 8)}…)? O processo será interrompido
						imediatamente.
					</p>
				}
				confirmLabel="Sim, cancelar job"
				danger
				busy={abortBusy}
				onConfirm={handleAbort}
				onClose={() => setAbortTarget(null)}
			/>

			{/* Confirmação de exclusão de Job */}
			<ConfirmDialog
				open={Boolean(deleteTarget)}
				title="Excluir job"
				body={
					<p className="text-xs text-zinc-300">
						Esta ação remove o job{" "}
						<strong className="text-white font-mono">
							{deleteTarget?.model}
						</strong>{" "}
						({deleteTarget?.id.slice(0, 8)}…) do histórico e{" "}
						<strong>apaga seus artefatos no armazenamento</strong>. Modelos
						derivados deste job saem do catálogo. As imagens já salvas na
						galeria de geração são preservadas.
					</p>
				}
				confirmLabel="Sim, excluir job"
				danger
				busy={deleteBusy}
				onConfirm={handleDeleteJob}
				onClose={() => setDeleteTarget(null)}
			/>

			{/* Diálogo de limpeza em lote */}
			<JobCleanupDialog
				open={cleanupOpen}
				onClose={() => setCleanupOpen(false)}
				terminalJobs={terminalJobs.map((j) => ({
					id: j.id,
					status: j.status as "done" | "failed" | "cancelled",
					createdAt: j.createdAt,
					finishedAt: j.finishedAt,
				}))}
				onDone={() => void fetchJobs()}
			/>

			<AutolabelReviewModal
				open={!!reviewJob}
				onClose={() => setReviewJob(null)}
				jobId={reviewJob?.id ?? null}
				datasetId={reviewJob?.datasetId}
				onApplied={() => {
					void fetchJobs();
				}}
			/>

			{autotrackerReviewJob && (
				<AutotrackerReviewModal
					open={!!autotrackerReviewJob}
					job={autotrackerReviewJob}
					onClose={() => setAutotrackerReviewJob(null)}
					onApplied={() => {
						void fetchJobs();
					}}
				/>
			)}
		</div>
	);
}

export default function JobsPage() {
	return (
		<Suspense
			fallback={
				<div className="flex h-full items-center justify-center p-8">
					<Spinner className="size-8" />
				</div>
			}
		>
			<JobsPageContent />
		</Suspense>
	);
}
