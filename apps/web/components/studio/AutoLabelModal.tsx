"use client";

import { useEffect, useRef, useState } from "react";
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
import { ApiError } from "@/lib/api";
import { startAutolabelJob } from "@/lib/autolabel";
import { autolabelErrorMessage, type AutolabelModel } from "@/types/studio";
import { showToast } from "./Toast";
import { openActionCenter } from "@/lib/events";
import NodeSelect from "./NodeSelect";

interface Props {
  open: boolean;
  datasetId: string;
  datasetTitle: string;
  onClose: () => void;
  onJobCreated: () => void;
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
}

const PROMPT_PRESETS = [
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

export default function AutoLabelModal({
  open,
  datasetId,
  datasetTitle,
  onClose,
  onJobCreated,
}: Props) {
  const router = useRouter();

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
      };
      localStorage.setItem(AUTOLABEL_STORAGE_KEY, JSON.stringify(next));
    } catch {
      // Ignora erro em ambientes restritos
    }
  }

  useEffect(() => {
    if (!open) return;
    setTopError(null);
    setBusy(false);

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

      if (model === "openai") {
        const provider = PROVIDER_PRESETS.find((p) => p.id === selectedProvider);
        if (provider?.keyRequired && !apiKey.trim()) {
          setTopError(`O provedor ${provider.label} requer uma API Key para autenticação.`);
          setBusy(false);
          return;
        }

        if (apiKey.trim()) payload.apiKey = apiKey.trim();
        if (apiBase.trim()) payload.apiBase = apiBase.trim();
        if (openaiModel.trim()) payload.openaiModel = openaiModel.trim();

        // Salva as configurações utilizadas para conveniência futura
        persistCustomConfig({
          provider: selectedProvider,
          apiBase: apiBase.trim(),
          openaiModel: openaiModel.trim(),
          apiKey: apiKey.trim(),
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
          className="block truncate font-mono text-[11px] text-zinc-400"
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

        {/* 1. SELETOR DE MODELO VISION */}
        <div className="space-y-2">
          <label className="block text-xs font-medium text-zinc-200">
            Motor de Visão / Modelo VLM
          </label>
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
                <span className="rounded-full bg-brand-500/20 border border-brand-500/30 px-1.5 py-0.5 font-mono text-[9px] text-brand-300">
                  GPU Local
                </span>
              </div>
              <p className="font-mono text-[11px] text-zinc-400 leading-snug">
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
                <span className="rounded-full bg-amber-500/20 border border-amber-500/30 px-1.5 py-0.5 font-mono text-[9px] text-amber-300">
                  GPU Local
                </span>
              </div>
              <p className="font-mono text-[11px] text-zinc-400 leading-snug">
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
                <span className="rounded-full bg-sky-500/20 border border-sky-500/30 px-1.5 py-0.5 font-mono text-[9px] text-sky-300">
                  API Vision
                </span>
              </div>
              <p className="font-mono text-[11px] text-zinc-400 leading-snug">
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
                <span className="rounded-full bg-zinc-800 border border-zinc-700 px-1.5 py-0.5 font-mono text-[9px] text-zinc-400">
                  Sem GPU
                </span>
              </div>
              <p className="font-mono text-[11px] text-zinc-400 leading-snug">
                Geração sintética rápida para testes de fluxo ou ambientes sem GPU/internet.
              </p>
            </button>
          </div>
        </div>

        {/* 2. CAMPOS ADICIONAIS QUANDO OPENAI SELECIONADO */}
        {model === "openai" && (
          <div className="rounded-xl border border-sky-500/25 bg-sky-500/[0.03] p-3.5 space-y-3.5">
            <div className="flex items-center justify-between pb-2 border-b border-white/5">
              <span className="font-mono text-[11px] font-semibold text-sky-300 uppercase tracking-caps flex items-center gap-1.5">
                <IconServer className="size-3 text-sky-400" />
                Configurações de Provedor & API Vision
              </span>
              <button
                type="button"
                onClick={handleResetProvider}
                title="Restaurar padrões do provedor ativo"
                className="font-mono text-[10px] text-zinc-400 hover:text-sky-300 flex items-center gap-1 transition"
              >
                <IconRefresh className="size-3" />
                Restaurar padrões
              </button>
            </div>

            {/* Presets Rápidos de Provedor */}
            <div className="space-y-1.5">
              <label className="block font-mono text-[11px] font-medium text-zinc-300">
                Provedor / Arquitetura de API
              </label>
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
                      <span className="font-mono font-semibold text-[11px] leading-tight block">
                        {p.label}
                      </span>
                      <span className="font-mono text-[9px] text-zinc-500 mt-0.5">
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
                <div className="flex items-start gap-2 rounded-lg border border-sky-500/20 bg-sky-500/10 px-2.5 py-1.5 text-[11px] font-mono text-sky-200">
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
                  className="block font-mono text-[11px] text-zinc-300"
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
                  className="text-[10px] font-mono text-zinc-400 hover:text-zinc-200"
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
                  className="block font-mono text-[11px] text-zinc-300"
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
                  disabled={busy}
                  className="font-mono text-xs"
                />
              </div>

              {/* Model ID */}
              <div className="space-y-1">
                <label
                  htmlFor="openai-model"
                  className="block font-mono text-[11px] text-zinc-300"
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
                  <span className="font-mono text-[10px] text-zinc-500">Modelos sugeridos:</span>
                  {p.suggestedModels.map((sm) => (
                    <button
                      key={sm}
                      type="button"
                      disabled={busy}
                      onClick={() => {
                        setOpenaiModel(sm);
                        persistCustomConfig({ openaiModel: sm });
                      }}
                      className={`rounded border px-1.5 py-0.5 font-mono text-[10px] transition ${
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
          </div>
        )}

        {/* 3. PROMPT & PRESETS */}
        <div className="space-y-1.5">
          <div className="flex items-center justify-between">
            <label
              htmlFor="al-prompt"
              className="block font-mono text-[11px] font-medium text-zinc-300"
            >
              Instrução / Prompt de Legendagem
            </label>
            <span className="font-mono text-[10px] text-zinc-500">
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
            <span className="font-mono text-[10px] text-zinc-500">Presets:</span>
            {PROMPT_PRESETS.map((preset) => (
              <button
                key={preset.label}
                type="button"
                disabled={busy}
                onClick={() => setPrompt(preset.text)}
                className="rounded-md border border-white/10 bg-white/[0.03] px-2 py-0.5 font-mono text-[10px] text-zinc-300 transition hover:border-brand-500/40 hover:bg-brand-500/10 hover:text-brand-300"
              >
                {preset.label}
              </button>
            ))}
            {prompt && (
              <button
                type="button"
                disabled={busy}
                onClick={() => setPrompt("")}
                className="rounded-md border border-rose-500/20 bg-rose-500/5 px-1.5 py-0.5 font-mono text-[10px] text-rose-400 hover:bg-rose-500/10"
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
