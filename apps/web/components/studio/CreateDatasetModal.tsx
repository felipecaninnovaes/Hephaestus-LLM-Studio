"use client";

import { useEffect, useRef, useState } from "react";
import { useRouter } from "next/navigation";
import {
  IconCheck,
  IconFileArchive,
  IconFolder,
  IconPlus,
  IconUpload,
  IconX,
} from "@/components/icons";
import { Button } from "@/components/ui/Button";
import { Modal } from "@/components/ui/Modal";
import ProgressBar from "@/components/ui/ProgressBar";
import { SegmentedControl } from "@/components/ui/SegmentedControl";
import { Select, type SelectOption } from "@/components/ui/Select";
import { ApiError } from "@/lib/api";
import { importDataset, importErrorMessage } from "@/lib/backup";
import { CLASS_RE, MAX_CLASSES } from "@/lib/classes";
import {
  inspectDataTransfer,
  inspectZipFile,
  type InspectionResult,
} from "@/lib/dataset-inspector";
import { createDataset } from "@/lib/datasets";
import { formatBytes } from "@/lib/format";
import { uploadImages } from "@/lib/images";
import { TYPE_LABELS, type Dataset, type DatasetType } from "@/types/studio";
import { showToast } from "./Toast";

const TYPES = Object.keys(TYPE_LABELS) as DatasetType[];
const TYPE_OPTIONS: SelectOption<DatasetType>[] = TYPES.map((t) => ({
  value: t,
  label: TYPE_LABELS[t],
}));

export function slugPreview(title: string): string {
  return title
    .toLowerCase()
    .trim()
    .normalize("NFD")
    .replace(/[\u0300-\u036f]/g, "")
    .replace(/\s+/g, "-")
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

interface Props {
  open: boolean;
  onClose: () => void;
  onCreated: (dataset: Dataset) => void;
  initialMode?: "empty" | "import";
  initialInspection?: InspectionResult | null;
}

type Phase = "form" | "confirm";

export default function CreateDatasetModal({
  open,
  onClose,
  onCreated,
  initialMode = "empty",
  initialInspection = null,
}: Props) {
  const router = useRouter();
  const nameRef = useRef<HTMLInputElement>(null);
  const zipInputRef = useRef<HTMLInputElement>(null);
  const folderInputRef = useRef<HTMLInputElement>(null);

  const [mode, setMode] = useState<"empty" | "import">(initialMode);
  const [phase, setPhase] = useState<Phase>("form");
  const [title, setTitle] = useState("");
  const [type, setType] = useState<DatasetType>("yolo_bbox");
  const [classesRaw, setClassesRaw] = useState("");
  const [inspection, setInspection] = useState<InspectionResult | null>(null);

  const [nameError, setNameError] = useState<string | null>(null);
  const [classesError, setClassesError] = useState<string | null>(null);
  const [topError, setTopError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [busyText, setBusyText] = useState("");
  const [isDraggingModal, setIsDraggingModal] = useState(false);
  const [uploadProgress, setUploadProgress] = useState<{ sent: number; total: number; batchIndex: number; batchCount: number } | null>(null);
  const cancelledRef = useRef(false);

  useEffect(() => {
    if (!open) return;
    setPhase("form");
    setNameError(null);
    setClassesError(null);
    setTopError(null);
    setBusy(false);

    if (initialInspection) {
      setMode("import");
      applyInspection(initialInspection);
    } else {
      setMode(initialMode);
      setTitle("");
      setType("yolo_bbox");
      setClassesRaw("");
      setInspection(null);
    }

    const t = setTimeout(() => nameRef.current?.focus(), 50);
    return () => clearTimeout(t);
  }, [open, initialMode, initialInspection]);

  if (!open) return null;

  function applyInspection(res: InspectionResult) {
    setInspection(res);
    setTitle(res.title);
    setType(res.category);
    setClassesRaw(res.classes.join(", "));
    setNameError(null);
    setClassesError(null);
    setTopError(null);
  }

  async function handleZipSelect(e: React.ChangeEvent<HTMLInputElement>) {
    const file = e.target.files?.[0];
    if (!file) return;
    setBusy(true);
    setBusyText("Inspecionando arquivo ZIP…");
    try {
      const res = await inspectZipFile(file);
      applyInspection(res);
    } catch (err) {
      setTopError(err instanceof Error ? err.message : "Falha ao inspecionar ZIP.");
    } finally {
      setBusy(false);
      if (zipInputRef.current) zipInputRef.current.value = "";
    }
  }

  async function handleFolderSelect(e: React.ChangeEvent<HTMLInputElement>) {
    const fileList = e.target.files;
    if (!fileList || fileList.length === 0) return;
    setBusy(true);
    setBusyText("Inspecionando pasta…");
    try {
      const files: File[] = [];
      const classSet = new Set<string>();
      let rootDir = "";

      for (let i = 0; i < fileList.length; i++) {
        const file = fileList[i];
        const path = file.webkitRelativePath || file.name;
        const parts = path.split("/");
        if (!rootDir && parts.length > 1) {
          rootDir = parts[0];
        }
        if (parts.length > 2) {
          const sub = parts[parts.length - 2]?.toLowerCase();
          if (sub && !["images", "labels", "train", "val", "test", "data"].includes(sub)) {
            classSet.add(sub.replace(/[^a-z0-9_]/g, "_").slice(0, 64));
          }
        }
        files.push(file);
      }

      const res: InspectionResult = {
        title: rootDir ? rootDir.replace(/[-_]+/g, " ").trim().slice(0, 96) : "Novo Dataset",
        category: "yolo_bbox",
        classes: Array.from(classSet),
        imagesCount: files.length,
        isBackupZip: false,
        folderFiles: files,
        sourceLabel: rootDir ? `Pasta "${rootDir}"` : `${files.length} arquivos`,
      };
      applyInspection(res);
    } catch (err) {
      setTopError(err instanceof Error ? err.message : "Falha ao inspecionar pasta.");
    } finally {
      setBusy(false);
      if (folderInputRef.current) folderInputRef.current.value = "";
    }
  }

  async function handleModalDrop(e: React.DragEvent) {
    e.preventDefault();
    e.stopPropagation();
    setIsDraggingModal(false);
    setBusy(true);
    setBusyText("Inspecionando dados soltos…");
    try {
      const res = await inspectDataTransfer(e.dataTransfer);
      if (res) {
        setMode("import");
        applyInspection(res);
      } else {
        setTopError("Nenhum arquivo ZIP ou imagem suportada detectada.");
      }
    } catch (err) {
      setTopError(err instanceof Error ? err.message : "Falha ao processar arquivos soltos.");
    } finally {
      setBusy(false);
    }
  }

  function parseClasses(): string[] {
    const seen = new Set<string>();
    const list: string[] = [];
    for (const part of classesRaw.split(",")) {
      const name = part.trim();
      if (!name || seen.has(name)) continue;
      seen.add(name);
      list.push(name);
    }
    return list;
  }

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    setNameError(null);
    setClassesError(null);
    setTopError(null);

    const trimmed = title.trim();
    if (!trimmed || trimmed.length > 96) {
      setNameError("Dê um nome ao dataset (máx 96 caracteres).");
      return;
    }

    const classes = parseClasses();
    const bad = classes.find((c) => !CLASS_RE.test(c));
    if (bad) {
      setClassesError("Classe inválida: use letras, números e _ (máx 64).");
      return;
    }
    if (classes.length > MAX_CLASSES) {
      setClassesError("Máximo de 200 classes por dataset.");
      return;
    }

    // 1. Ingestão via Pacote de Backup Hephaestus (.zip)
    if (mode === "import" && inspection?.isBackupZip && inspection.file) {
      await runBackupImport(inspection.file, false);
      return;
    }

    // 2. Ingestão de Pasta com Imagens Locais
    if (mode === "import" && inspection?.folderFiles && inspection.folderFiles.length > 0) {
      await runFolderIngest(trimmed, type, classes, inspection.folderFiles);
      return;
    }

    // 3. Criação de Container Vazio Tradicional
    await runEmptyCreation(trimmed, type, classes);
  }

  async function runBackupImport(file: File, replace: boolean) {
    setBusy(true);
    setBusyText(replace ? "Substituindo dataset…" : "Importando backup…");
    try {
      const trimmed = title.trim();
      const created = await importDataset(file, {
        ...(trimmed ? { title: trimmed.slice(0, 96) } : {}),
        ...(replace ? { replace: true } : {}),
      });
      showToast(
        `Dataset importado com sucesso — ${created.imagesCount.toLocaleString()} imagens, ${created.classes.length} classes.`,
        "success",
        {
          label: "Abrir dataset",
          onClick: () => router.push(`/datasets/${created.id}`),
        },
      );
      onClose();
      onCreated(created);
    } catch (err) {
      if (err instanceof ApiError) {
        if (err.status === 401) {
          router.replace("/login");
          return;
        }
        if (err.status === 413) {
          setTopError("Arquivo maior que o limite de 200 MiB.");
          return;
        }
        if (err.status === 409 && err.code === "slug_conflict" && !replace) {
          setPhase("confirm");
          return;
        }
        setTopError(importErrorMessage(err.code));
        return;
      }
      setTopError("Falha ao importar backup.");
    } finally {
      setBusy(false);
    }
  }

  async function runFolderIngest(
    datasetTitle: string,
    datasetType: DatasetType,
    classes: string[],
    files: File[],
  ) {
    setBusy(true);
    cancelledRef.current = false;
    setUploadProgress(null);
    setBusyText("Criando dataset…");
    try {
      const created = await createDataset(
        classes.length > 0
          ? { title: datasetTitle, type: datasetType, classes }
          : { title: datasetTitle, type: datasetType },
      );

      setBusyText(`Enviando ${files.length} imagens…`);

      // Warn about oversized files before uploading
      const OVERSIZE_LIMIT = 200 * 1024 * 1024;
      const oversizedCount = files.filter((f) => f.size > OVERSIZE_LIMIT).length;
      if (oversizedCount > 0) {
        showToast(
          `${oversizedCount} arquivo(s) excedem 200 MiB e serão rejeitados.`,
          "info",
        );
      }

      const uploadRes = await uploadImages(created.id, files, {
        onProgress: (p) => {
          setUploadProgress(p);
          setBusyText(`Enviando imagens — ${p.sent} de ${p.total} (lote ${p.batchIndex} de ${p.batchCount})…`);
        },
        isCancelled: () => cancelledRef.current,
      });

      const uploadedCount = uploadRes.items.filter(
        (i) => i.status === "stored" || i.status === "duplicate",
      ).length;
      const rejectedItems = uploadRes.items.filter(
        (i) => i.status === "rejected" || i.status === "failed",
      );
      const rejectedCount = rejectedItems.length;
      const wasCancelled = cancelledRef.current;

      if (wasCancelled) {
        const summary = rejectedCount > 0
          ? `${uploadedCount} imagens importadas de ${files.length} antes do cancelamento, ${rejectedCount} rejeitadas.`
          : `${uploadedCount} imagens importadas de ${files.length} antes do cancelamento.`;
        showToast(summary, "info");
      } else if (rejectedCount === 0) {
        showToast(
          `${uploadedCount} imagens importadas com sucesso.`,
          "success",
          {
            label: "Abrir dataset",
            onClick: () => router.push(`/datasets/${created.id}`),
          },
        );
      } else {
        const examples = rejectedItems
          .slice(0, 3)
          .map((r) => `${r.filename} (${r.reason ?? r.status})`)
          .join(", ");
        const suffix = rejectedCount > 3 ? ` (+${rejectedCount - 3} mais)` : "";
        showToast(
          `${uploadedCount} imagens importadas, ${rejectedCount} rejeitadas: ${examples}${suffix}`,
          rejectedCount > uploadedCount ? "error" : "info",
        );
      }
      onClose();
      onCreated(created);
    } catch (err) {
      if (err instanceof ApiError) {
        if (err.code === "slug_conflict") {
          setNameError("Já existe um dataset com esse nome.");
          return;
        }
        if (err.status === 401) {
          router.replace("/login");
          return;
        }
        setTopError(err.message || "Erro na ingestão.");
        return;
      }
      setTopError("Falha na ingestão da pasta.");
    } finally {
      setBusy(false);
      setUploadProgress(null);
    }
  }

  async function runEmptyCreation(
    datasetTitle: string,
    datasetType: DatasetType,
    classes: string[],
  ) {
    setBusy(true);
    setBusyText("Criando dataset…");
    try {
      const created = await createDataset(
        classes.length > 0
          ? { title: datasetTitle, type: datasetType, classes }
          : { title: datasetTitle, type: datasetType },
      );
      showToast("Dataset criado com sucesso.", "success", {
        label: "Abrir dataset",
        onClick: () => router.push(`/datasets/${created.id}`),
      });
      onClose();
      onCreated(created);
    } catch (err) {
      if (err instanceof ApiError) {
        if (err.code === "slug_conflict") {
          setNameError("Já existe um dataset com esse nome.");
          return;
        }
        if (err.code === "invalid_request") {
          setTopError("Verifique os campos preenchidos.");
          return;
        }
        if (err.code === "unauthorized" || err.status === 401) {
          router.replace("/login");
          return;
        }
      }
      setTopError("Falha ao criar dataset.");
    } finally {
      setBusy(false);
    }
  }

  const slug = slugPreview(title);
  const simpleSlug = title.toLowerCase().trim().replace(/\s+/g, "-");
  const showAdjustHint = slug.length > 0 && slug !== simpleSlug;

  return (
    <Modal
      open={open}
      onClose={() => {
        if (!busy) onClose();
      }}
      title={phase === "confirm" ? "Substituir Dataset Existente" : "Novo Dataset & Ingestão"}
      description={
        phase === "confirm"
          ? "Ação irreversível de substituição de dados"
          : "Crie um container vazio ou ingeste pacotes ZIP e pastas"
      }
      icon={mode === "import" ? <IconUpload className="h-4 w-4" /> : <IconPlus className="h-4 w-4" />}
      maxWidth="lg"
      onDragOver={(e) => {
        e.preventDefault();
        setIsDraggingModal(true);
      }}
      onDragLeave={() => setIsDraggingModal(false)}
      onDrop={handleModalDrop}
    >

        {/* Confirmação de Substituição (Slug Conflict) */}
        {phase === "confirm" ? (
          <div className="mt-4 space-y-4 text-xs">
            <div className="rounded-xl border border-rose-500/30 bg-rose-500/10 backdrop-blur-sm p-3.5 text-xs leading-relaxed text-rose-200">
              <p className="font-semibold text-rose-100 mb-1">Conflito de Identificador (Slug)</p>
              Já existe um dataset com este identificador. A substituição irá apagar o dataset atual e
              recriá-lo a partir do arquivo importado. Esta operação não pode ser desfeita.
            </div>
            <div className="flex justify-end space-x-2 pt-2">
              <Button
                type="button"
                variant="ghost"
                size="md"
                onClick={() => setPhase("form")}
                disabled={busy}
              >
                Cancelar
              </Button>
              <Button
                type="button"
                variant="destructive"
                size="md"
                onClick={() => inspection?.file && runBackupImport(inspection.file, true)}
                disabled={busy}
                loading={busy}
              >
                {busy ? "Substituindo…" : "Substituir Dataset"}
              </Button>
            </div>
          </div>
        ) : (
          <form onSubmit={handleSubmit} className="mt-4 space-y-4 text-xs">
            {/* Seletor Segmentado de Modo */}
            <SegmentedControl<"empty" | "import">
              ariaLabel="Modo de criação do dataset"
              value={mode}
              onChange={(val) => {
                setMode(val);
                if (val === "empty") setInspection(null);
              }}
              className="w-full"
              options={[
                { id: "empty", label: "Container Vazio" },
                { id: "import", label: "Ingestão (ZIP / Pasta)" },
              ]}
            />

            {/* Alerta de Erro Topo */}
            {topError && (
              <p role="alert" className="rounded-lg border border-rose-500/30 bg-rose-500/10 backdrop-blur-sm px-3 py-2 text-xs text-rose-300">
                {topError}
              </p>
            )}

            {/* Modo Ingestão: Dropzone & Detecção */}
            {mode === "import" && (
              <div>
                {!inspection ? (
                  <div
                    className={`relative rounded-xl border-2 border-dashed p-6 text-center transition-all backdrop-blur-sm ${
                      isDraggingModal
                        ? "border-brand-500 bg-brand-500/15"
                        : "border-zinc-800 bg-black/40 hover:border-zinc-700"
                    }`}
                  >
                    <div className="mx-auto flex h-10 w-10 items-center justify-center rounded-xl border border-zinc-800 bg-zinc-900 text-zinc-400 mb-2.5">
                      <IconUpload className="h-5 w-5" />
                    </div>
                    <p className="text-xs font-semibold text-zinc-200">
                      Arraste um pacote ZIP ou pasta aqui
                    </p>
                    <p className="mt-0.5 text-[11px] text-zinc-400">
                      Autodeteção de classes, anotações e contagem de imagens
                    </p>

                    <div className="mt-4 flex items-center justify-center gap-2">
                      <Button
                        type="button"
                        variant="secondary"
                        size="sm"
                        onClick={() => zipInputRef.current?.click()}
                        leftIcon={<IconFileArchive className="h-3.5 w-3.5 text-brand-400" />}
                        className="font-mono text-[11px]"
                      >
                        Selecionar ZIP
                      </Button>
                      <Button
                        type="button"
                        variant="secondary"
                        size="sm"
                        onClick={() => folderInputRef.current?.click()}
                        leftIcon={<IconFolder className="h-3.5 w-3.5 text-amber-400" />}
                        className="font-mono text-[11px]"
                      >
                        Selecionar Pasta
                      </Button>
                    </div>

                    <input
                      ref={zipInputRef}
                      type="file"
                      accept=".zip"
                      onChange={handleZipSelect}
                      className="hidden"
                    />
                    <input
                      ref={folderInputRef}
                      type="file"
                      multiple
                      onChange={handleFolderSelect}
                      className="hidden"
                      {...({ webkitdirectory: "", directory: "" } as React.InputHTMLAttributes<HTMLInputElement>)}
                    />
                  </div>
                ) : (
                  <div className="rounded-xl border border-brand-500/30 bg-brand-500/[0.08] backdrop-blur-sm p-3.5">
                    <div className="flex items-center justify-between">
                      <div className="flex items-center gap-2.5 min-w-0">
                        <div className="flex h-7 w-7 shrink-0 items-center justify-center rounded-lg border border-brand-500/40 bg-brand-500/20 backdrop-blur-sm text-brand-300">
                          {inspection.isBackupZip ? <IconFileArchive className="h-4 w-4" /> : <IconFolder className="h-4 w-4" />}
                        </div>
                        <div className="min-w-0">
                          <p className="truncate font-mono text-xs font-semibold text-zinc-200" title={inspection.sourceLabel}>
                            {inspection.sourceLabel}
                          </p>
                          <p className="font-mono text-[11px] text-brand-300">
                            {inspection.imagesCount} imagens identificadas
                            {inspection.file && ` · ${formatBytes(inspection.file.size)}`}
                          </p>
                        </div>
                      </div>
                      <button
                        type="button"
                        onClick={() => {
                          setInspection(null);
                          setTitle("");
                          setClassesRaw("");
                        }}
                        className="text-[11px] font-mono text-zinc-400 hover:text-white underline px-2 py-1"
                      >
                        Trocar
                      </button>
                    </div>

                    {/* Classes Autodetectadas */}
                    {inspection.classes.length > 0 && (
                      <div className="mt-3 pt-2.5 border-t border-brand-500/20">
                        <span className="text-[11px] font-mono uppercase tracking-caps text-brand-200 block mb-1.5">
                          Classes Detectadas ({inspection.classes.length}):
                        </span>
                        <div className="flex flex-wrap gap-1 max-h-20 overflow-y-auto">
                          {inspection.classes.map((c) => (
                            <span
                              key={c}
                              className="inline-flex items-center gap-1 text-[11px] font-mono px-2 py-0.5 rounded bg-black/40 text-zinc-200 border border-brand-500/20 backdrop-blur-sm"
                            >
                              <IconCheck className="h-2.5 w-2.5 text-[#34d399]" />
                              {c}
                            </span>
                          ))}
                        </div>
                      </div>
                    )}
                  </div>
                )}
              </div>
            )}

            {/* Campo Nome */}
            <div>
              <label htmlFor="create-dataset-name" className="tracking-caps mb-1 block font-mono text-[11px] font-medium uppercase text-zinc-300">
                Nome do Dataset
              </label>
              <input
                id="create-dataset-name"
                ref={nameRef}
                type="text"
                required
                maxLength={96}
                placeholder="Ex.: Inspeção de PCB v2"
                value={title}
                onChange={(e) => setTitle(e.target.value)}
                className="w-full rounded-xl border border-zinc-800 bg-black/40 backdrop-blur-sm px-3 py-2 font-mono text-zinc-200 focus:border-brand-500 focus:outline-none focus:ring-1 focus:ring-brand-500/40"
              />
              {slug && (
                <p className="mt-1 font-mono text-[11px] text-zinc-400">
                  slug: {slug}
                  {showAdjustHint && " · o servidor pode ajustar"}
                </p>
              )}
              {nameError && (
                <p role="alert" className="mt-1 text-[11px] text-rose-300">
                  {nameError}
                </p>
              )}
            </div>

            {/* Campo Tipo / Tarefa */}
            <Select
              id="create-dataset-type"
              label="Tipo / Tarefa"
              options={TYPE_OPTIONS}
              value={type}
              onChange={(val) => setType(val as DatasetType)}
              fontMono
            />

            {/* Campo Classes */}
            <div>
              <label htmlFor="create-dataset-classes" className="tracking-caps mb-1 block font-mono text-[11px] font-medium uppercase text-zinc-300">
                Classes {mode === "import" && inspection?.classes.length ? "(ajustáveis)" : "(opcional)"}
              </label>
              <input
                id="create-dataset-classes"
                type="text"
                placeholder="defeito_a, defeito_b, anomalia"
                value={classesRaw}
                onChange={(e) => setClassesRaw(e.target.value)}
                className="w-full rounded-xl border border-zinc-800 bg-black/40 backdrop-blur-sm px-3 py-2 font-mono text-zinc-200 focus:border-brand-500 focus:outline-none focus:ring-1 focus:ring-brand-500/40"
              />
              <p className="mt-1 text-[11px] text-zinc-400">
                Separadas por vírgula — ordem vira índice YOLO
              </p>
              {classesError && (
                <p role="alert" className="mt-1 text-[11px] text-rose-300">
                  {classesError}
                </p>
              )}
            </div>

            {/* Ações do Rodapé */}
            <div className="pt-3 border-t border-white/10 space-y-3">
              {/* Progress bar during upload */}
              {uploadProgress && (
                <div className="space-y-1.5">
                  <ProgressBar
                    value={(uploadProgress.sent / uploadProgress.total) * 100}
                    variant="brand"
                    size="sm"
                  />
                  <p className="font-mono text-[11px] text-zinc-400 text-center">
                    {uploadProgress.sent} de {uploadProgress.total} · lote{" "}
                    <span className="text-zinc-200">{uploadProgress.batchIndex}</span>
                    /{uploadProgress.batchCount}
                  </p>
                </div>
              )}

              <div className="flex items-center justify-between">
                <span className="font-mono text-[11px] text-zinc-400">
                  {busy
                    ? busyText
                    : mode === "import" && inspection
                      ? "Pronto para criar e ingestar"
                      : "Container vazio"}
                </span>

                <div className="flex items-center space-x-2">
                  {uploadProgress ? (
                    <Button
                      type="button"
                      variant="destructive"
                      size="md"
                      onClick={() => { cancelledRef.current = true; }}
                    >
                      Cancelar envio
                    </Button>
                  ) : (
                    <Button
                      type="button"
                      variant="ghost"
                      size="md"
                      onClick={onClose}
                      disabled={busy}
                    >
                      Cancelar
                    </Button>
                  )}
                  <Button
                    type="submit"
                    variant="primary"
                    size="md"
                    disabled={busy || (mode === "import" && !inspection && !title.trim())}
                    loading={busy && !uploadProgress}
                  >
                    {busy && !uploadProgress
                      ? busyText || "Processando…"
                      : mode === "import" && inspection
                        ? `Criar e Ingestar (${inspection.imagesCount} imgs)`
                        : "Criar Dataset"}
                  </Button>
                </div>
              </div>
            </div>
          </form>
        )}
    </Modal>
  );
}
