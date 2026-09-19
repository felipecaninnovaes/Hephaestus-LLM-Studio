"use client";

import {
  IconCopy,
  IconDownload,
  IconImage,
  IconSliders,
} from "@/components/icons";
import { Button, Modal, TruncatedText } from "@/components/ui";
import type { Generation } from "@/types/studio";

interface GenerationLightboxModalProps {
  lightboxItem: Generation | null;
  onClose: () => void;
  getFullImageUrl: (gen: Generation) => string;
  onDownloadSingle: (gen: Generation) => void;
  onUseConfigs: (gen: Generation) => void;
  onUseAsInit: (gen: Generation) => void;
  onCopyConfigs: (gen: Generation) => void;
  onCopyPrompt: (gen: Generation) => void;
}

export function GenerationLightboxModal({
  lightboxItem,
  onClose,
  getFullImageUrl,
  onDownloadSingle,
  onUseConfigs,
  onUseAsInit,
  onCopyConfigs,
  onCopyPrompt,
}: GenerationLightboxModalProps) {
  if (!lightboxItem) return null;

  return (
    <Modal
      open={!!lightboxItem}
      onClose={onClose}
      title={`Seed ${lightboxItem.seed}`}
      maxWidth="xl"
      bodyClassName="p-0"
      headerRight={
        <Button
          type="button"
          variant="ghost"
          size="sm"
          onClick={() => onDownloadSingle(lightboxItem)}
        >
          <IconDownload className="size-3.5" />
          <span className="ml-1">Baixar</span>
        </Button>
      }
    >
      <div className="flex flex-col gap-4">
        {/* Imagem */}
        {/* biome-ignore lint/performance/noImgElement: preview direto de URL de imagem de geracao */}
        <img
          src={getFullImageUrl(lightboxItem)}
          alt={lightboxItem.prompt}
          className="w-full rounded-lg object-contain max-h-[60vh]"
        />

        {/* Metadados */}
        <div className="grid grid-cols-2 gap-x-6 gap-y-2 text-xs px-1">
          <MetaRow label="Prompt" value={lightboxItem.prompt} />
          {lightboxItem.negativePrompt && (
            <MetaRow label="Negative" value={lightboxItem.negativePrompt} />
          )}
          <MetaRow
            label="Dimensões"
            value={`${lightboxItem.width}×${lightboxItem.height}`}
            mono
          />
          <MetaRow label="Seed" value={String(lightboxItem.seed)} mono />
          <MetaRow
            label="Base Model"
            value={String(
              lightboxItem.params?.baseModel ||
                lightboxItem.params?.base_model ||
                "—",
            )}
          />
          {lightboxItem.params?.custom_model_id ? (
            <MetaRow
              label="Custom Model"
              value={String(lightboxItem.params.custom_model_id)}
            />
          ) : null}
          {lightboxItem.params?.text_encoder_model_id ? (
            <MetaRow
              label="Text Encoder"
              value={String(lightboxItem.params.text_encoder_model_id)}
            />
          ) : null}
          {lightboxItem.params?.steps ? (
            <MetaRow
              label="Steps"
              value={String(lightboxItem.params.steps)}
              mono
            />
          ) : null}
          {lightboxItem.params?.guidance_scale != null ? (
            <MetaRow
              label="CFG"
              value={String(lightboxItem.params.guidance_scale)}
              mono
            />
          ) : null}
          {lightboxItem.params?.quantization ? (
            <MetaRow
              label="Quantização"
              value={String(lightboxItem.params.quantization)}
            />
          ) : null}
          {lightboxItem.params?.sampler ? (
            <MetaRow
              label="Sampler"
              value={String(lightboxItem.params.sampler)}
              mono
            />
          ) : null}
          {lightboxItem.params?.upscale != null &&
          typeof lightboxItem.params.upscale === "object" &&
          "scale" in lightboxItem.params.upscale ? (
            <MetaRow
              label="Upscale"
              value={`Real-ESRGAN 4x · ${String(lightboxItem.params.upscale.scale)}x`}
              mono
            />
          ) : null}
          {lightboxItem.params?.distilled != null ? (
            <MetaRow
              label="Destilado"
              value={lightboxItem.params.distilled ? "Sim" : "Não"}
            />
          ) : null}
          {lightboxItem.params?.loras &&
          Array.isArray(lightboxItem.params.loras) &&
          lightboxItem.params.loras.length > 0 ? (
            <MetaRow
              label="LoRAs"
              value={`${lightboxItem.params.loras.length} adaptador(es)`}
            />
          ) : null}
          <MetaRow
            label="Criado em"
            value={new Date(lightboxItem.createdAt).toLocaleString("pt-BR")}
          />
        </div>

        {/* Ações */}
        <div className="flex flex-wrap gap-2 px-1 pb-1">
          <Button
            type="button"
            variant="primary"
            size="sm"
            onClick={() => onUseConfigs(lightboxItem)}
            aria-label={`Usar configs da geração seed ${lightboxItem.seed} no gerador`}
          >
            <IconSliders className="size-3.5" />
            <span className="ml-1">Usar estas configs</span>
          </Button>
          <Button
            type="button"
            variant="secondary"
            size="sm"
            onClick={() => onUseAsInit(lightboxItem)}
            aria-label={`Usar geração seed ${lightboxItem.seed} como imagem inicial do img2img`}
            title="Carrega esta imagem como entrada do img2img na aba Gerar"
          >
            <IconImage className="size-3.5" />
            <span className="ml-1">Usar como imagem inicial</span>
          </Button>
          <Button
            type="button"
            variant="secondary"
            size="sm"
            onClick={() => onCopyConfigs(lightboxItem)}
            aria-label={`Copiar configs da geração seed ${lightboxItem.seed} em JSON`}
          >
            <IconCopy className="size-3.5" />
            <span className="ml-1">Copiar configs</span>
          </Button>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            onClick={() => onCopyPrompt(lightboxItem)}
            aria-label={`Copiar prompt da geração seed ${lightboxItem.seed}`}
          >
            Copiar prompt
          </Button>
        </div>
      </div>
    </Modal>
  );
}

function MetaRow({
  label,
  value,
  mono = false,
}: {
  label: string;
  value: string;
  mono?: boolean;
}) {
  return (
    <div className="flex flex-col gap-0.5">
      <span className="font-mono text-3xs uppercase tracking-caps text-zinc-500">
        {label}
      </span>
      <TruncatedText
        text={value}
        lines={3}
        as="span"
        className={`text-zinc-200 ${mono ? "font-mono" : ""}`}
      />
    </div>
  );
}
