"use client";

import { useEffect, useRef, useState } from "react";
import { useRouter } from "next/navigation";
import {
  IconSparkles,
  IconCpu,
  IconZap,
  IconServer,
  IconLock,
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

  // Estados de OpenAI / Compatível
  const [apiKey, setApiKey] = useState("");
  const [showApiKey, setShowApiKey] = useState(false);
  const [apiBase, setApiBase] = useState("https://api.openai.com/v1");
  const [openaiModel, setOpenaiModel] = useState("gpt-4o-mini");

  const [busy, setBusy] = useState(false);
  const [topError, setTopError] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!open) return;
    setModel("florence-2");
    setPrompt("");
    setSelectedOrchestratorId(null);
    setApiKey("");
    setApiBase("https://api.openai.com/v1");
    setOpenaiModel("gpt-4o-mini");
    setTopError(null);
    setBusy(false);
    const t = setTimeout(() => inputRef.current?.focus(), 40);
    return () => clearTimeout(t);
  }, [open]);

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
        if (apiKey.trim()) payload.apiKey = apiKey.trim();
        if (apiBase.trim()) payload.apiBase = apiBase.trim();
        if (openaiModel.trim()) payload.openaiModel = openaiModel.trim();
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
          <div className="rounded-xl border border-sky-500/25 bg-sky-500/[0.03] p-3.5 space-y-3">
            <div className="flex items-center justify-between pb-1 border-b border-white/5">
              <span className="font-mono text-[11px] font-semibold text-sky-300 uppercase tracking-caps flex items-center gap-1.5">
                <IconServer className="size-3 text-sky-400" />
                Configurações da API OpenAI
              </span>
              <span className="font-mono text-[10px] text-zinc-400">
                HTTPS / Ollama / vLLM
              </span>
            </div>

            {/* API Key */}
            <div className="space-y-1">
              <div className="flex items-center justify-between">
                <label
                  htmlFor="openai-key"
                  className="block font-mono text-[11px] text-zinc-300"
                >
                  API Key
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
                  placeholder="sk-proj-... (opcional se configurada via env no nó)"
                  value={apiKey}
                  onChange={(e) => setApiKey(e.target.value)}
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
                  onChange={(e) => setApiBase(e.target.value)}
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
                  Modelo Remoto
                </label>
                <Input
                  id="openai-model"
                  type="text"
                  placeholder="gpt-4o-mini"
                  value={openaiModel}
                  onChange={(e) => setOpenaiModel(e.target.value)}
                  disabled={busy}
                  className="font-mono text-xs"
                />
              </div>
            </div>
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
