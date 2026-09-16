"use client";

import { useMemo, useState } from "react";
import { IconTag, IconTrash, IconPlus, IconSparkles } from "@/components/icons";
import { Modal } from "@/components/ui/Modal";
import { Button } from "@/components/ui/Button";
import { Select, type SelectOption } from "@/components/ui/Select";
import { Input } from "@/components/ui/Input";
import { ApiError } from "@/lib/api";
import { batchUpdateBoxes } from "@/lib/images";
import { CLASS_RE, MAX_CLASSES, putClasses } from "@/lib/classes";
import { getDataset } from "@/lib/datasets";
import type { StudioClass } from "@/types/studio";
import { showToast } from "./Toast";

type BatchAction = "remap" | "delete";

interface Props {
  open: boolean;
  datasetId: string;
  datasetClasses: StudioClass[];
  selectedImageIds: string[];
  totalInView: number;
  onClose: () => void;
  onSuccess: () => void;
  onClassesUpdated?: (classes: StudioClass[]) => void;
}

export function BatchEditClassesModal({
  open,
  datasetId,
  datasetClasses,
  selectedImageIds,
  totalInView,
  onClose,
  onSuccess,
  onClassesUpdated,
}: Props) {
  const [action, setAction] = useState<BatchAction>("remap");
  const [sourceClassId, setSourceClassId] = useState<string>("");
  const [targetClassId, setTargetClassId] = useState<string>("");
  const [isCreatingNewClass, setIsCreatingNewClass] = useState(false);
  const [newClassName, setNewClassName] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Inicializa a classe de origem padrão se ainda não escolhida
  const activeSourceId = sourceClassId || (datasetClasses.length > 0 ? datasetClasses[0].id : "");

  // Options para Source Class
  const sourceOptions = useMemo<SelectOption<string>[]>(() => {
    return datasetClasses.map((c) => ({
      value: c.id,
      label: c.name,
      badge: (
        <span
          className="size-2.5 rounded-full inline-block border border-white/20"
          style={{ backgroundColor: c.color || "#888" }}
        />
      ),
    }));
  }, [datasetClasses]);

  // Options para Target Class (excluindo sourceClassId)
  const targetOptions = useMemo<SelectOption<string>[]>(() => {
    return datasetClasses
      .filter((c) => c.id !== activeSourceId)
      .map((c) => ({
        value: c.id,
        label: c.name,
        badge: (
          <span
            className="size-2.5 rounded-full inline-block border border-white/20"
            style={{ backgroundColor: c.color || "#888" }}
          />
        ),
      }));
  }, [datasetClasses, activeSourceId]);

  const activeTargetId =
    targetClassId && targetClassId !== activeSourceId
      ? targetClassId
      : targetOptions.length > 0
        ? targetOptions[0].value
        : "";

  const sourceClassName = datasetClasses.find((c) => c.id === activeSourceId)?.name ?? "classe selecionada";
  const targetClassName = isCreatingNewClass
    ? newClassName.trim() || "nova classe"
    : datasetClasses.find((c) => c.id === activeTargetId)?.name ?? "classe de destino";

  const isSelectionScope = selectedImageIds.length > 0;
  const targetCount = isSelectionScope ? selectedImageIds.length : totalInView;

  async function handleCreateClass(): Promise<string | null> {
    const trimmed = newClassName.trim();
    if (!trimmed) {
      setError("Informe o nome da nova classe.");
      return null;
    }
    if (!CLASS_RE.test(trimmed)) {
      setError("Nome deve conter apenas letras, números e underline (máx. 64 caracteres).");
      return null;
    }
    if (datasetClasses.some((c) => c.name.toLowerCase() === trimmed.toLowerCase())) {
      setError("Uma classe com esse nome já existe no dataset.");
      return null;
    }
    if (datasetClasses.length >= MAX_CLASSES) {
      setError(`Limite de ${MAX_CLASSES} classes atingido.`);
      return null;
    }

    try {
      const freshDs = await getDataset(datasetId);
      const payload = [
        ...freshDs.classes.map((c) => ({ id: c.id, name: c.name })),
        { name: trimmed },
      ];
      const res = await putClasses(datasetId, payload);
      onClassesUpdated?.(res.classes);
      const created = res.classes.find((c) => c.name.toLowerCase() === trimmed.toLowerCase());
      return created ? created.id : null;
    } catch {
      setError("Falha ao cadastrar a nova classe.");
      return null;
    }
  }

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    setError(null);

    if (!activeSourceId) {
      setError("Selecione a classe de origem.");
      return;
    }

    let finalTargetId: string | null = null;

    if (action === "remap") {
      if (isCreatingNewClass) {
        setBusy(true);
        const createdId = await handleCreateClass();
        if (!createdId) {
          setBusy(false);
          return;
        }
        finalTargetId = createdId;
      } else {
        if (!activeTargetId) {
          setError("Selecione a classe de destino ou crie uma nova.");
          return;
        }
        finalTargetId = activeTargetId;
      }
    }

    setBusy(true);
    try {
      const res = await batchUpdateBoxes(datasetId, {
        imageIds: isSelectionScope ? selectedImageIds : null,
        action,
        sourceClassId: activeSourceId,
        targetClassId: action === "remap" ? finalTargetId : null,
      });

      const actionText = action === "remap" ? "remapadas" : "excluídas";
      showToast(
        `${res.affectedBoxes} boxes ${actionText} com sucesso em ${res.affectedImages} imagens.`,
        "success",
      );
      onSuccess();
      onClose();
    } catch (err) {
      if (err instanceof ApiError) {
        setError(err.message || "Erro ao processar alteração em lote.");
      } else {
        setError("Erro inesperado ao alterar classes em lote.");
      }
    } finally {
      setBusy(false);
    }
  }

  return (
    <Modal
      open={open}
      onClose={busy ? () => {} : onClose}
      title="Edição de Classes em Lote"
      description={
        isSelectionScope
          ? `Afetando ${selectedImageIds.length} imagens selecionadas na galeria.`
          : `Afetando todas as ${totalInView} imagens ativas do dataset.`
      }
      maxWidth="md"
    >
      <form onSubmit={handleSubmit} className="space-y-5">
        {/* Toggle de Modo: Remapear vs Excluir */}
        <div className="flex rounded-xl border border-white/10 bg-zinc-900/60 p-1">
          <button
            type="button"
            onClick={() => {
              setAction("remap");
              setError(null);
            }}
            className={`flex-1 flex items-center justify-center gap-2 rounded-lg px-3 py-2 text-xs font-semibold transition-all cursor-pointer ${
              action === "remap"
                ? "bg-brand-500/20 text-brand-300 border border-brand-500/30 shadow-sm"
                : "text-zinc-400 hover:text-zinc-200"
            }`}
          >
            <IconTag className="size-3.5" />
            <span>Remapear Classe</span>
          </button>
          <button
            type="button"
            onClick={() => {
              setAction("delete");
              setError(null);
            }}
            className={`flex-1 flex items-center justify-center gap-2 rounded-lg px-3 py-2 text-xs font-semibold transition-all cursor-pointer ${
              action === "delete"
                ? "bg-rose-500/20 text-rose-300 border border-rose-500/30 shadow-sm"
                : "text-zinc-400 hover:text-zinc-200"
            }`}
          >
            <IconTrash className="size-3.5" />
            <span>Excluir Classe da Seleção</span>
          </button>
        </div>

        {/* Classe de Origem */}
        <div className="space-y-1.5">
          <span className="text-xs font-medium text-zinc-300">
            Classe de Origem {action === "remap" ? "(a ser substituída)" : "(a ser removida)"}
          </span>
          {sourceOptions.length > 0 ? (
            <Select
              value={activeSourceId}
              onChange={(val) => {
                setSourceClassId(val);
                setError(null);
              }}
              options={sourceOptions}
            />
          ) : (
            <div className="rounded-lg border border-white/10 bg-zinc-900/40 p-3 text-xs text-zinc-400">
              Nenhuma classe cadastrada no dataset.
            </div>
          )}
        </div>

        {/* Classe de Destino (somente modo Remap) */}
        {action === "remap" && (
          <div className="space-y-2 rounded-xl border border-white/10 bg-zinc-900/30 p-3.5">
            <div className="flex items-center justify-between">
              <span className="text-xs font-medium text-zinc-300">
                Nova Classe de Destino
              </span>
              <button
                type="button"
                onClick={() => {
                  setIsCreatingNewClass(!isCreatingNewClass);
                  setError(null);
                }}
                className="text-2xs font-mono text-brand-400 hover:text-brand-300 flex items-center gap-1 cursor-pointer transition-colors"
              >
                {isCreatingNewClass ? (
                  "Selecionar classe existente"
                ) : (
                  <>
                    <IconPlus className="size-3" />
                    <span>Criar nova classe</span>
                  </>
                )}
              </button>
            </div>

            {isCreatingNewClass ? (
              <div className="space-y-1.5">
                <Input
                  value={newClassName}
                  onChange={(e) => {
                    setNewClassName(e.target.value);
                    setError(null);
                  }}
                  placeholder="Ex: male_face"
                  autoFocus
                />
                <p className="text-2xs text-zinc-500">
                  A classe será adicionada ao dataset e aplicada nas boxes selecionadas.
                </p>
              </div>
            ) : targetOptions.length > 0 ? (
              <Select
                value={activeTargetId}
                onChange={(val) => {
                  setTargetClassId(val);
                  setError(null);
                }}
                options={targetOptions}
              />
            ) : (
              <div className="text-xs text-zinc-400 py-1">
                Não há outra classe cadastrada.{" "}
                <button
                  type="button"
                  onClick={() => setIsCreatingNewClass(true)}
                  className="text-brand-400 underline hover:text-brand-300"
                >
                  Criar nova classe agora
                </button>
              </div>
            )}
          </div>
        )}

        {/* Resumo Explicativo */}
        <div className="rounded-xl border border-white/10 bg-zinc-950/60 p-3 text-xs text-zinc-400 leading-relaxed">
          {action === "remap" ? (
            <p>
              Todas as bounding boxes anotadas como{" "}
              <strong className="text-zinc-200 font-mono">
                {sourceClassName}
              </strong>{" "}
              nas{" "}
              <strong className="text-brand-400 font-mono">
                {targetCount} {targetCount === 1 ? "imagem" : "imagens"}
              </strong>{" "}
              {isSelectionScope ? "selecionadas" : "do dataset"} serão reatribuídas para{" "}
              <strong className="text-brand-300 font-mono">
                {targetClassName}
              </strong>
              .
            </p>
          ) : (
            <p>
              Todas as bounding boxes anotadas como{" "}
              <strong className="text-rose-400 font-mono">
                {sourceClassName}
              </strong>{" "}
              serão excluídas permanentemente das{" "}
              <strong className="text-zinc-200 font-mono">
                {targetCount} {targetCount === 1 ? "imagem" : "imagens"}
              </strong>{" "}
              {isSelectionScope ? "selecionadas" : "do dataset"}. Demais classes não serão alteradas.
            </p>
          )}
        </div>

        {error && (
          <div className="rounded-xl border border-rose-500/30 bg-rose-500/10 p-3 text-xs text-rose-300 font-medium">
            {error}
          </div>
        )}

        {/* Rodapé com botões de ação */}
        <div className="flex items-center justify-end gap-3 pt-2 border-t border-white/10">
          <Button
            type="button"
            variant="secondary"
            onClick={onClose}
            disabled={busy}
          >
            Cancelar
          </Button>
          <Button
            type="submit"
            variant={action === "delete" ? "destructive" : "primary"}
            disabled={
              busy ||
              !activeSourceId ||
              (action === "remap" && !isCreatingNewClass && !activeTargetId) ||
              (action === "remap" && isCreatingNewClass && !newClassName.trim())
            }
          >
            {busy ? (
              "Processando…"
            ) : action === "remap" ? (
              <>
                <IconTag className="size-3.5 mr-1.5" />
                <span>Aplicar Remapeamento</span>
              </>
            ) : (
              <>
                <IconTrash className="size-3.5 mr-1.5" />
                <span>Excluir Boxes da Classe</span>
              </>
            )}
          </Button>
        </div>
      </form>
    </Modal>
  );
}

export default BatchEditClassesModal;
