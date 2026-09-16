"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import { useRouter } from "next/navigation";
import {
  IconSparkles,
  IconCpu,
  IconZap,
  IconServer,
  IconLock,
  IconInfo,
  IconRefresh,
} from "@/components/icons";
import { Modal } from "@/components/ui/Modal";
import { Button } from "@/components/ui/Button";
import { Input } from "@/components/ui/Input";
import { Select, type SelectOption } from "@/components/ui/Select";
import { ApiError } from "@/lib/api";
import { startAutolabelJob } from "@/lib/autolabel";
import { autolabelErrorMessage, type AutolabelModel, type StudioClass } from "@/types/studio";
import { showToast } from "./Toast";
import { openActionCenter } from "@/lib/events";
import NodeSelect from "./NodeSelect";

interface Props {
  open: boolean;
  datasetId: string;
  datasetTitle: string;
  onClose: () => void;
  onJobCreated: () => void;
  classes?: StudioClass[];
  initialFilterClassId?: string | null;
  selectedImageIds?: string[];
  totalImagesCount?: number;
}

export type ApiProviderPreset = "openai" | "ollama" | "openrouter" | "lmstudio" | "custom";

interface ProviderConfig {
  id: ApiProviderPreset;
  label: string;
  badge: string;
  defaultBase: string;
  defaultModel: string;
  suggestedModels: string[];
  keyPlaceholder: string;
  keyRequired: boolean;
  hint: string;
}

const PROVIDER_PRESETS: ProviderConfig[] = [
  {
    id: "openai",
    label: "OpenAI Oficial",
    badge: "Nuvem",
    defaultBase: "https://api.openai.com/v1",
    defaultModel: "gpt-4o-mini",
    suggestedModels: ["gpt-4o-mini", "gpt-4o", "chatgpt-4o-latest"],
    keyPlaceholder: "sk-proj-... (obrigatório)",
    keyRequired: true,
    hint: "Requer API Key ativa da OpenAI.",
  },
  {
    id: "ollama",
    label: "Ollama Local",
    badge: "Host",
    defaultBase: "http://localhost:11434/v1",
    defaultModel: "llava",
    suggestedModels: ["llava", "llama3.2-vision", "bakllava", "minicpm-v"],
    keyPlaceholder: "Opcional (não exigido pelo Ollama)",
    keyRequired: false,
    hint: "Conexão direta com Ollama no host. A resolução de rede é mapeada automaticamente para o container.",
  },
  {
    id: "openrouter",
    label: "OpenRouter",
    badge: "Gateway",
    defaultBase: "https://openrouter.ai/api/v1",
    defaultModel: "openai/gpt-4o-mini",
    suggestedModels: [
      "openai/gpt-4o-mini",
      "google/gemini-flash-1.5",
      "anthropic/claude-3-haiku",
      "meta-llama/llama-3.2-11b-vision-instruct",
    ],
    keyPlaceholder: "sk-or-v1-...",
    keyRequired: true,
    hint: "Acesso a dezenas de VLMs comerciais e abertos via chave unificada.",
  },
  {
    id: "lmstudio",
    label: "vLLM / LM Studio",
    badge: "Local",
    defaultBase: "http://localhost:1234/v1",
    defaultModel: "local-model",
    suggestedModels: ["local-model", "qwen2-vl", "llava-v1.6"],
    keyPlaceholder: "Opcional ou Bearer Token local",
    keyRequired: false,
    hint: "LM Studio em :1234 ou vLLM em :8000 compatível com protocolo OpenAI.",
  },
  {
    id: "custom",
    label: "Customizado",
    badge: "Livre",
    defaultBase: "https://api.openai.com/v1",
    defaultModel: "gpt-4o-mini",
    suggestedModels: [],
    keyPlaceholder: "sk-... (se requerido pelo endpoint)",
    keyRequired: false,
    hint: "Endpoint base, ID do modelo e chave totalmente configuráveis.",
  },
];

const AUTOLABEL_STORAGE_KEY = "hephaestus_autolabel_custom_config_v1";

interface AutoLabelSavedConfig {
  provider: ApiProviderPreset;
  apiBase: string;
  openaiModel: string;
  apiKey?: string;
  model?: AutolabelModel;
  disableReasoning?: boolean;
}

const PROMPT_PRESETS = [
  {
    label: "Foco na Classe YOLO",
    text: "Descreva em detalhes o(a) {class_name} visível nesta imagem, incluindo cores, acabamento, posição e estado de conservação.",
  },
  {
    label: "Difusão LoRA",
    text: "Descreva detalhadamente o sujeito principal, iluminação, cores, textura e composição desta imagem para treinamento de difusão.",
  },
  {
    label: "Inspeção Técnica",
    text: "Identifique com precisão todos os componentes, materiais, conexões e eventuais defeitos visíveis nesta peça.",
  },
  {
    label: "Tags & Palavras-chave",
    text: "Forneça uma lista concisa de palavras-chave e tags descritivas separadas por vírgula para esta imagem.",
  },
];

function sanitizeParam(val: string): string {
  return val.trim().replace(/^["']+|["']+$/g, "");
}

function sanitizeUrl(val: string): string {
  return val.trim().replace(/^["']+|["']+$/g, "").replace(/\/+$/, "");
}

export type ScopeMode = "all" | "class" | "selected";

export default function AutoLabelModal({
  open,
  datasetId,
  datasetTitle,
  onClose,
  onJobCreated,
  classes = [],
  initialFilterClassId = null,
  selectedImageIds = [],
  totalImagesCount,
}: Props) {
  const router = useRouter();

  // Escopo de processamento
  const [scopeMode, setScopeMode] = useState<ScopeMode>("all");
  const [selectedClassId, setSelectedClassId] = useState<string>("");

  const classOptions = useMemo<SelectOption<string>[]>(() => {
    if (!classes) return [];
    return classes.map((cls) => ({
      value: cls.id,
      label: `${cls.name} (idx: ${cls.idx})`,
    }));
  }, [classes]);

  // Estados principais
  const [model, setModel] = useState<AutolabelModel>("florence-2");
  const [prompt, setPrompt] = useState("");
  const [selectedOrchestratorId, setSelectedOrchestratorId] = useState<string | null>(null);

  // Estados de Provedor OpenAI / Compatível
  const [selectedProvider, setSelectedProvider] = useState<ApiProviderPreset>("openai");
  const [apiKey, setApiKey] = useState("");
  const [showApiKey, setShowApiKey] = useState(false);
  const [apiBase, setApiBase] = useState("https://api.openai.com/v1");
  const [openaiModel, setOpenaiModel] = useState("gpt-4o-mini");
  const [disableReasoning, setDisableReasoning] = useState(true);

  const [busy, setBusy] = useState(false);
  const [topError, setTopError] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  // Helper para salvar preferências no localStorage
  function persistCustomConfig(updates: Partial<AutoLabelSavedConfig>) {
    try {
      const raw = localStorage.getItem(AUTOLABEL_STORAGE_KEY);
      const current = raw ? (JSON.parse(raw) as Partial<AutoLabelSavedConfig>) : {};
      const next: AutoLabelSavedConfig = {
        provider: updates.provider ?? current.provider ?? selectedProvider,
        apiBase: updates.apiBase ?? current.apiBase ?? apiBase,
        openaiModel: updates.openaiModel ?? current.openaiModel ?? openaiModel,
        apiKey: updates.apiKey !== undefined ? updates.apiKey : (current.apiKey ?? apiKey),
        model: updates.model ?? current.model ?? model,
        disableReasoning:
          updates.disableReasoning !== undefined
            ? updates.disableReasoning
            : (current.disableReasoning ?? disableReasoning),
      };
      localStorage.setItem(AUTOLABEL_STORAGE_KEY, JSON.stringify(next));
    } catch {
      // Ignora erro em ambientes restritos
    }
  }

  function handleInsertTag(tag: string) {
    setPrompt((prev) => (prev ? `${prev} ${tag}` : tag));
  }

  useEffect(() => {
    if (!open) return;
    setTopError(null);
    setBusy(false);

    if (selectedImageIds && selectedImageIds.length > 0) {
      setScopeMode("selected");
    } else if (initialFilterClassId) {
      setScopeMode("class");
      setSelectedClassId(initialFilterClassId);
    } else {
      setScopeMode("all");
    }

    if (classes && classes.length > 0) {
      if (initialFilterClassId) {
        setSelectedClassId(initialFilterClassId);
      } else if (!selectedClassId) {
        setSelectedClassId(classes[0].id);
      }
    }

    // Restaura configurações salvas de OpenAI/compatível do localStorage se existirem
    try {
      const raw = localStorage.getItem(AUTOLABEL_STORAGE_KEY);
      if (raw) {
        const saved = JSON.parse(raw) as Partial<AutoLabelSavedConfig>;
        if (saved.provider) setSelectedProvider(saved.provider);
        if (saved.apiBase) setApiBase(saved.apiBase);
        if (saved.openaiModel) setOpenaiModel(saved.openaiModel);
        if (saved.apiKey !== undefined) setApiKey(saved.apiKey);
        if (saved.model) setModel(saved.model);
        if (saved.disableReasoning !== undefined) setDisableReasoning(saved.disableReasoning);
      }
    } catch {
      // Falha silenciosa se localStorage corrompido
    }

    const t = setTimeout(() => inputRef.current?.focus(), 40);
    return () => clearTimeout(t);
  }, [open]);

  function handleSelectProvider(p: ProviderConfig) {
    setSelectedProvider(p.id);
    if (p.id !== "custom") {
      setApiBase(p.defaultBase);
      setOpenaiModel(p.defaultModel);
      persistCustomConfig({
        provider: p.id,
        apiBase: p.defaultBase,
        openaiModel: p.defaultModel,
      });
    } else {
      persistCustomConfig({ provider: "custom" });
    }
  }

  function handleResetProvider() {
    const p = PROVIDER_PRESETS.find((x) => x.id === selectedProvider) ?? PROVIDER_PRESETS[0];
    setApiBase(p.defaultBase);
    setOpenaiModel(p.defaultModel);
    persistCustomConfig({
      apiBase: p.defaultBase,
      openaiModel: p.defaultModel,
    });
  }

  if (!open) return null;

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    setTopError(null);
    setBusy(true);

    try {
      const payload: Parameters<typeof startAutolabelJob>[0] = {
        datasetId,
        model,
        ...(prompt.trim() ? { prompt: prompt.trim() } : {}),
        ...(selectedOrchestratorId ? { orchestratorId: selectedOrchestratorId } : {}),
      };

      if (scopeMode === "class" && selectedClassId) {
        payload.filterClassId = selectedClassId;
      } else if (scopeMode === "selected" && selectedImageIds && selectedImageIds.length > 0) {
        payload.imageIds = selectedImageIds;
      }

      if (model === "openai") {
        const provider = PROVIDER_PRESETS.find((p) => p.id === selectedProvider);
        if (provider?.keyRequired && !apiKey.trim()) {
          setTopError(`O provedor ${provider.label} requer uma API Key para autenticação.`);
          setBusy(false);
          return;
        }

        const cleanKey = sanitizeParam(apiKey);
        const cleanBase = sanitizeUrl(apiBase);
        const cleanModel = sanitizeParam(openaiModel);

        if (cleanKey) payload.apiKey = cleanKey;
        if (cleanBase) payload.apiBase = cleanBase;
        if (cleanModel) payload.openaiModel = cleanModel;
        if (disableReasoning) {
          payload.reasoningEffort = "none";
        }

        // Salva as configurações utilizadas para conveniência futura
        persistCustomConfig({
          provider: selectedProvider,
          apiBase: cleanBase,
          openaiModel: cleanModel,
          apiKey: cleanKey,
          disableReasoning,
          model,
        });
      }

      const result = await startAutolabelJob(payload);
      showToast(
        `AutoLabel iniciado com sucesso (posição ${result.queuePosition ?? "—"} na fila).`,
        "success",
      );
      onClose();
      onJobCreated();
      openActionCenter();
    } catch (err) {
      if (err instanceof ApiError) {
        if (err.code === "unauthorized" || err.status === 401) {
          router.replace("/login");
          return;
        }
        setTopError(autolabelErrorMessage(err.code));
        return;
      }
      setTopError("Falha ao criar job de AutoLabel.");
    } finally {
      setBusy(false);
    }
  }

  return (
    <Modal
      open={open}
      onClose={onClose}
      title="AutoLabel v2"
      description={
        <span
          className="block truncate font-mono text-2xs text-zinc-400"
          title={datasetTitle}
        >
          {datasetTitle}
        </span>
      }
      icon={<IconSparkles className="h-4 w-4 text-brand-400" />}
      maxWidth="lg"
      busy={busy}
      ariaLabel="AutoLabel v2"
    >
      <form onSubmit={handleSubmit} className="space-y-5 text-xs">
        {topError && (
          <p
            role="alert"
            className="rounded-lg border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-xs text-rose-300 backdrop-blur-sm"
          >
            {topError}
          </p>
        )}

        {/* ESCOPO DE PROCESSAMENTO */}
        <div className="space-y-2 rounded-xl border border-white/10 bg-white/[0.02] p-3">
          <div className="flex items-center justify-between">
            <span className="font-mono text-xs font-medium text-zinc-200">
              Escopo de Execução
            </span>
            <span className="font-mono text-3xs text-zinc-400">
              {scopeMode === "all" && (totalImagesCount ? `${totalImagesCount} imagens` : "todas as imagens")}
              {scopeMode === "class" && "filtro por classe YOLO"}
              {scopeMode === "selected" && `${selectedImageIds?.length ?? 0} imagens selecionadas`}
            </span>
          </div>

          <div className="grid grid-cols-1 sm:grid-cols-3 gap-2">
            {/* Todas */}
            <button
              type="button"
              disabled={busy}
              onClick={() => setScopeMode("all")}
              className={`flex flex-col text-left p-2.5 rounded-lg border transition ${
                scopeMode === "all"
                  ? "border-brand-500/60 bg-brand-500/10 text-white ring-1 ring-brand-500/30"
                  : "border-white/10 bg-white/[0.02] text-zinc-400 hover:border-white/20 hover:text-zinc-200"
              }`}
            >
              <span className="font-semibold text-xs text-zinc-100">Dataset Completo</span>
              <span className="font-mono text-3xs text-zinc-400">Todas as imagens ativas</span>
            </button>

            {/* Filtrar por Classe */}
            <button
              type="button"
              disabled={busy || !classes || classes.length === 0}
              onClick={() => {
                setScopeMode("class");
                if (!selectedClassId && classes && classes.length > 0) {
                  setSelectedClassId(classes[0].id);
                }
              }}
              className={`flex flex-col text-left p-2.5 rounded-lg border transition ${
                scopeMode === "class"
                  ? "border-status-alert/60 bg-status-alert/10 text-white ring-1 ring-status-alert/30"
                  : "border-white/10 bg-white/[0.02] text-zinc-400 hover:border-white/20 hover:text-zinc-200 disabled:opacity-40"
              }`}
            >
              <span className="font-semibold text-xs text-zinc-100">Por Classe YOLO</span>
              <span className="font-mono text-3xs text-zinc-400">Apenas com a classe anotada</span>
            </button>

            {/* Selecionadas */}
            <button
              type="button"
              disabled={busy || !selectedImageIds || selectedImageIds.length === 0}
              onClick={() => setScopeMode("selected")}
              className={`flex flex-col text-left p-2.5 rounded-lg border transition ${
                scopeMode === "selected"
                  ? "border-sky-500/60 bg-sky-500/10 text-white ring-1 ring-sky-500/30"
                  : "border-white/10 bg-white/[0.02] text-zinc-400 hover:border-white/20 hover:text-zinc-200 disabled:opacity-40"
              }`}
            >
              <span className="font-semibold text-xs text-zinc-100">Selecionadas no Grid</span>
              <span className="font-mono text-3xs text-zinc-400">
                {selectedImageIds && selectedImageIds.length > 0
                  ? `${selectedImageIds.length} selecionadas`
                  : "Nenhuma selecionada"}
              </span>
            </button>
          </div>

          {/* Seletor da classe quando scopeMode === 'class' */}
          {scopeMode === "class" && classes && classes.length > 0 && (
            <div className="mt-2.5 pt-2.5 border-t border-white/5 space-y-2">
              <Select
                id="autolabel-filter-class"
                label="Classe para Filtragem"
                options={classOptions}
                value={selectedClassId}
                onChange={(val) => setSelectedClassId(val)}
                disabled={busy}
                placeholder="Selecione uma classe para filtrar…"
                fontMono
                size="sm"
              />
              <div className="flex items-center gap-2 text-2xs text-zinc-400">
                <span>Tag dinâmica no prompt:</span>
                <button
                  type="button"
                  onClick={() => handleInsertTag("{class_name}")}
                  title="Clique para adicionar {class_name} ao prompt"
                  className="rounded border border-status-alert/30 bg-status-alert/15 px-1.5 py-0.5 font-mono text-3xs text-amber-300 hover:bg-status-alert/25 transition"
                >
                  + &#123;class_name&#125;
                </button>
                <span className="text-3xs text-zinc-500">
                  (substituída pelo nome da classe na execução)
                </span>
              </div>
            </div>
          )}
        </div>

        {/* 1. SELETOR DE MODELO VISION */}
        <div className="space-y-2">
          <span className="block text-xs font-medium text-zinc-200">
            Motor de Visão / Modelo VLM
          </span>
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-2.5">
            {/* Florence-2 */}
            <button
              type="button"
              disabled={busy}
              onClick={() => setModel("florence-2")}
              className={`flex flex-col text-left p-3 rounded-xl border transition duration-150 ${
                model === "florence-2"
                  ? "border-brand-500/60 bg-brand-500/10 text-white shadow-sm ring-1 ring-brand-500/30"
                  : "border-white/10 bg-white/[0.02] text-zinc-300 hover:border-white/20 hover:bg-white/[0.04]"
              }`}
            >
              <div className="flex items-center justify-between w-full mb-1">
                <span className="font-display font-semibold text-xs text-zinc-100 flex items-center gap-1.5">
                  <IconCpu className="size-3.5 text-brand-400" />
                  Florence-2
                </span>
                <span className="rounded-full bg-brand-500/20 border border-brand-500/30 px-1.5 py-0.5 font-mono text-4xs text-brand-300">
                  GPU Local
                </span>
              </div>
              <p className="font-mono text-2xs text-zinc-400 leading-snug">
                Microsoft Florence-2. Dense captioning rápido e rico em contornos visuais.
              </p>
            </button>

            {/* Qwen2-VL */}
            <button
              type="button"
              disabled={busy}
              onClick={() => setModel("qwen2-vl")}
              className={`flex flex-col text-left p-3 rounded-xl border transition duration-150 ${
                model === "qwen2-vl"
                  ? "border-brand-500/60 bg-brand-500/10 text-white shadow-sm ring-1 ring-brand-500/30"
                  : "border-white/10 bg-white/[0.02] text-zinc-300 hover:border-white/20 hover:bg-white/[0.04]"
              }`}
            >
              <div className="flex items-center justify-between w-full mb-1">
                <span className="font-display font-semibold text-xs text-zinc-100 flex items-center gap-1.5">
                  <IconZap className="size-3.5 text-amber-400" />
                  Qwen2-VL
                </span>
                <span className="rounded-full bg-status-alert/20 border border-status-alert/30 px-1.5 py-0.5 font-mono text-4xs text-amber-300">
                  GPU Local
                </span>
              </div>
              <p className="font-mono text-2xs text-zinc-400 leading-snug">
                Alibaba Qwen2-VL. Alto raciocínio analítico e aderência a instruções finas.
              </p>
            </button>

            {/* OpenAI / Compatível */}
            <button
              type="button"
              disabled={busy}
              onClick={() => setModel("openai")}
              className={`flex flex-col text-left p-3 rounded-xl border transition duration-150 ${
                model === "openai"
                  ? "border-sky-500/60 bg-sky-500/10 text-white shadow-sm ring-1 ring-sky-500/30"
                  : "border-white/10 bg-white/[0.02] text-zinc-300 hover:border-white/20 hover:bg-white/[0.04]"
              }`}
            >
              <div className="flex items-center justify-between w-full mb-1">
                <span className="font-display font-semibold text-xs text-zinc-100 flex items-center gap-1.5">
                  <IconServer className="size-3.5 text-sky-400" />
                  OpenAI / Compatível
                </span>
                <span className="rounded-full bg-sky-500/20 border border-sky-500/30 px-1.5 py-0.5 font-mono text-4xs text-sky-300">
                  API Vision
                </span>
              </div>
              <p className="font-mono text-2xs text-zinc-400 leading-snug">
                GPT-4o, GPT-4o-mini ou instâncias locais (Ollama, vLLM) via protocolo OpenAI.
              </p>
            </button>

            {/* Mock Determinístico */}
            <button
              type="button"
              disabled={busy}
              onClick={() => setModel("mock")}
              className={`flex flex-col text-left p-3 rounded-xl border transition duration-150 ${
                model === "mock"
                  ? "border-zinc-500/60 bg-zinc-500/10 text-white shadow-sm ring-1 ring-zinc-500/30"
                  : "border-white/10 bg-white/[0.02] text-zinc-300 hover:border-white/20 hover:bg-white/[0.04]"
              }`}
            >
              <div className="flex items-center justify-between w-full mb-1">
                <span className="font-display font-semibold text-xs text-zinc-100 flex items-center gap-1.5">
                  <IconSparkles className="size-3.5 text-zinc-400" />
                  Mock Determinístico
                </span>
                <span className="rounded-full bg-zinc-800 border border-zinc-700 px-1.5 py-0.5 font-mono text-4xs text-zinc-400">
                  Sem GPU
                </span>
              </div>
              <p className="font-mono text-2xs text-zinc-400 leading-snug">
                Geração sintética rápida para testes de fluxo ou ambientes sem GPU/internet.
              </p>
            </button>
          </div>
        </div>

        {/* 2. CAMPOS ADICIONAIS QUANDO OPENAI SELECIONADO */}
        {model === "openai" && (
          <div className="rounded-xl border border-sky-500/25 bg-sky-500/[0.03] p-3.5 space-y-3.5">
            <div className="flex items-center justify-between pb-2 border-b border-white/5">
              <span className="font-mono text-2xs font-semibold text-sky-300 uppercase tracking-caps flex items-center gap-1.5">
                <IconServer className="size-3 text-sky-400" />
                Configurações de Provedor & API Vision
              </span>
              <button
                type="button"
                onClick={handleResetProvider}
                title="Restaurar padrões do provedor ativo"
                className="font-mono text-3xs text-zinc-400 hover:text-sky-300 flex items-center gap-1 transition"
              >
                <IconRefresh className="size-3" />
                Restaurar padrões
              </button>
            </div>

            {/* Presets Rápidos de Provedor */}
            <div className="space-y-1.5">
              <span className="block font-mono text-2xs font-medium text-zinc-300">
                Provedor / Arquitetura de API
              </span>
              <div className="grid grid-cols-2 sm:grid-cols-5 gap-1.5">
                {PROVIDER_PRESETS.map((p) => {
                  const active = selectedProvider === p.id;
                  return (
                    <button
                      key={p.id}
                      type="button"
                      disabled={busy}
                      onClick={() => handleSelectProvider(p)}
                      className={`flex flex-col items-start p-2 rounded-lg border text-left transition ${
                        active
                          ? "border-sky-500/60 bg-sky-500/20 text-white shadow-sm ring-1 ring-sky-500/40"
                          : "border-white/10 bg-white/[0.02] text-zinc-400 hover:border-white/20 hover:text-zinc-200"
                      }`}
                    >
                      <span className="font-mono font-semibold text-2xs leading-tight block">
                        {p.label}
                      </span>
                      <span className="font-mono text-4xs text-zinc-500 mt-0.5">
                        {p.badge}
                      </span>
                    </button>
                  );
                })}
              </div>
            </div>

            {/* Dica / Contexto do Provedor Selecionado */}
            {(() => {
              const p = PROVIDER_PRESETS.find((x) => x.id === selectedProvider);
              if (!p) return null;
              return (
                <div className="flex items-start gap-2 rounded-lg border border-sky-500/20 bg-sky-500/10 px-2.5 py-1.5 text-2xs font-mono text-sky-200">
                  <IconInfo className="size-3.5 shrink-0 mt-0.5 text-sky-400" />
                  <span>{p.hint}</span>
                </div>
              );
            })()}

            {/* API Key */}
            <div className="space-y-1">
              <div className="flex items-center justify-between">
                <label
                  htmlFor="openai-key"
                  className="block font-mono text-2xs text-zinc-300"
                >
                  API Key
                  {(() => {
                    const p = PROVIDER_PRESETS.find((x) => x.id === selectedProvider);
                    return p?.keyRequired ? (
                      <span className="text-amber-400 ml-1">*</span>
                    ) : (
                      <span className="text-zinc-500 font-normal ml-1">(opcional)</span>
                    );
                  })()}
                </label>
                <button
                  type="button"
                  onClick={() => setShowApiKey(!showApiKey)}
                  className="text-3xs font-mono text-zinc-400 hover:text-zinc-200"
                >
                  {showApiKey ? "Ocultar" : "Mostrar"}
                </button>
              </div>
              <div className="relative">
                <Input
                  id="openai-key"
                  type={showApiKey ? "text" : "password"}
                  placeholder={
                    PROVIDER_PRESETS.find((x) => x.id === selectedProvider)?.keyPlaceholder ??
                    "sk-proj-... (opcional se no nó)"
                  }
                  value={apiKey}
                  onChange={(e) => {
                    setApiKey(e.target.value);
                    persistCustomConfig({ apiKey: e.target.value });
                  }}
                  onBlur={() => {
                    const cleaned = sanitizeParam(apiKey);
                    if (cleaned !== apiKey) {
                      setApiKey(cleaned);
                      persistCustomConfig({ apiKey: cleaned });
                    }
                  }}
                  disabled={busy}
                  className="font-mono text-xs pr-8"
                />
                <span className="absolute right-2.5 top-2 text-zinc-500 pointer-events-none">
                  <IconLock className="size-3.5" />
                </span>
              </div>
            </div>

            <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
              {/* API Base URL */}
              <div className="space-y-1">
                <label
                  htmlFor="openai-base"
                  className="block font-mono text-2xs text-zinc-300"
                >
                  Endpoint Base URL
                </label>
                <Input
                  id="openai-base"
                  type="text"
                  placeholder="https://api.openai.com/v1"
                  value={apiBase}
                  onChange={(e) => {
                    const val = e.target.value;
                    setApiBase(val);
                    persistCustomConfig({ apiBase: val });
                  }}
                  onBlur={() => {
                    const cleaned = sanitizeUrl(apiBase);
                    if (cleaned !== apiBase) {
                      setApiBase(cleaned);
                      persistCustomConfig({ apiBase: cleaned });
                    }
                  }}
                  disabled={busy}
                  className="font-mono text-xs"
                />
              </div>

              {/* Model ID */}
              <div className="space-y-1">
                <label
                  htmlFor="openai-model"
                  className="block font-mono text-2xs text-zinc-300"
                >
                  Modelo Remoto (Vision)
                </label>
                <Input
                  id="openai-model"
                  type="text"
                  placeholder="gpt-4o-mini"
                  value={openaiModel}
                  onChange={(e) => {
                    const val = e.target.value;
                    setOpenaiModel(val);
                    persistCustomConfig({ openaiModel: val });
                  }}
                  onBlur={() => {
                    const cleaned = sanitizeParam(openaiModel);
                    if (cleaned !== openaiModel) {
                      setOpenaiModel(cleaned);
                      persistCustomConfig({ openaiModel: cleaned });
                    }
                  }}
                  disabled={busy}
                  className="font-mono text-xs"
                />
              </div>
            </div>

            {/* Chips de Modelos Sugeridos */}
            {(() => {
              const p = PROVIDER_PRESETS.find((x) => x.id === selectedProvider);
              if (!p || p.suggestedModels.length === 0) return null;
              return (
                <div className="flex flex-wrap items-center gap-1.5 pt-0.5">
                  <span className="font-mono text-3xs text-zinc-500">Modelos sugeridos:</span>
                  {p.suggestedModels.map((sm) => (
                    <button
                      key={sm}
                      type="button"
                      disabled={busy}
                      onClick={() => {
                        setOpenaiModel(sm);
                        persistCustomConfig({ openaiModel: sm });
                      }}
                      className={`rounded border px-1.5 py-0.5 font-mono text-3xs transition ${
                        openaiModel === sm
                          ? "border-sky-500/60 bg-sky-500/20 text-sky-200"
                          : "border-white/10 bg-white/[0.03] text-zinc-400 hover:border-white/20 hover:text-zinc-200"
                      }`}
                    >
                      {sm}
                    </button>
                  ))}
                </div>
              );
            })()}

            {/* Toggle de Desativação de Reasoning (Fast Mode) */}
            <div className="flex items-center justify-between rounded-lg border border-white/10 bg-white/[0.02] p-2.5 transition hover:border-white/15">
              <div className="space-y-0.5 pr-3">
                <div className="flex items-center gap-2">
                  <span className="font-mono text-xs font-medium text-zinc-200">
                    Desativar Raciocínio (Fast Mode)
                  </span>
                  <span className="rounded border border-status-success/30 bg-status-success/20 px-1.5 py-0.2 font-mono text-4xs font-semibold text-status-success">
                    Recomendado
                  </span>
                </div>
                <p className="text-2xs text-zinc-400">
                  Suprime tokens de reflexão interna (<code className="rounded bg-black/40 px-1 py-0.5 font-mono text-3xs text-zinc-300">&lt;think&gt;</code> / <code className="rounded bg-black/40 px-1 py-0.5 font-mono text-3xs text-zinc-300">reasoning_effort: none</code>), acelerando a resposta e economizando tokens de contexto.
                </p>
              </div>
              <label className="relative inline-flex shrink-0 cursor-pointer items-center">
                <input
                  type="checkbox"
                  checked={disableReasoning}
                  disabled={busy}
                  onChange={(e) => {
                    const checked = e.target.checked;
                    setDisableReasoning(checked);
                    persistCustomConfig({ disableReasoning: checked });
                  }}
                  className="peer sr-only"
                />
                <div className="h-5 w-9 rounded-full bg-zinc-700 peer-focus:outline-none peer-focus:ring-2 peer-focus:ring-brand-500/40 peer-checked:bg-brand-500 peer-checked:after:translate-x-full peer-checked:after:border-white after:absolute after:left-[2px] after:top-[2px] after:h-4 after:w-4 after:rounded-full after:border after:border-zinc-300 after:bg-white after:transition-all after:content-['']"></div>
              </label>
            </div>
          </div>
        )}

        {/* 3. PROMPT & PRESETS */}
        <div className="space-y-1.5">
          <div className="flex items-center justify-between">
            <label
              htmlFor="al-prompt"
              className="block font-mono text-2xs font-medium text-zinc-300"
            >
              Instrução / Prompt de Legendagem
            </label>
            <span className="font-mono text-3xs text-zinc-500">
              {prompt.length}/8000
            </span>
          </div>

          <Input
            ref={inputRef}
            id="al-prompt"
            type="text"
            placeholder="Ex.: Descreva o sujeito principal, iluminação e estilo fotográfico…"
            value={prompt}
            onChange={(e) => setPrompt(e.target.value)}
            disabled={busy}
            className="font-mono text-xs"
          />

          {/* Presets rápidos */}
          <div className="flex flex-wrap items-center gap-1.5 pt-1">
            <span className="font-mono text-3xs text-zinc-500">Presets:</span>
            {PROMPT_PRESETS.map((preset) => (
              <button
                key={preset.label}
                type="button"
                disabled={busy}
                onClick={() => setPrompt(preset.text)}
                className="rounded-md border border-white/10 bg-white/[0.03] px-2 py-0.5 font-mono text-3xs text-zinc-300 transition hover:border-brand-500/40 hover:bg-brand-500/10 hover:text-brand-300"
              >
                {preset.label}
              </button>
            ))}
            {prompt && (
              <button
                type="button"
                disabled={busy}
                onClick={() => setPrompt("")}
                className="rounded-md border border-rose-500/20 bg-rose-500/5 px-1.5 py-0.5 font-mono text-3xs text-rose-400 hover:bg-rose-500/10"
              >
                Limpar
              </button>
            )}
          </div>
        </div>

        {/* 4. SELETOR DE NÓ DE EXECUÇÃO */}
        <NodeSelect
          value={selectedOrchestratorId}
          onChange={setSelectedOrchestratorId}
          disabled={busy}
          size="default"
        />

        {/* 5. AÇÕES */}
        <div className="flex items-center justify-end gap-2.5 pt-2 border-t border-white/10">
          <Button
            type="button"
            variant="secondary"
            size="sm"
            onClick={onClose}
            disabled={busy}
          >
            Cancelar
          </Button>
          <Button
            type="submit"
            variant="primary"
            size="md"
            loading={busy}
            leftIcon={<IconSparkles className="size-3.5" />}
          >
            {busy ? "Iniciando AutoLabel…" : "Iniciar AutoLabel"}
          </Button>
        </div>
      </form>
    </Modal>
  );
}
