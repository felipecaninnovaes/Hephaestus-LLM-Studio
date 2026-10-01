import type { JobArtifact } from "@/types/jobs";

/**
 * Helpers puros para interpretar os nomes de artefatos de samples de difusão
 * emitidos pelo `engines/trainer-difusao` (ex.: `samples/sample_epoch_000.png`,
 * `samples/sample_epoch_015.png`) e coletados por
 * `services/orchestrator/src/app/stages/collector.rs` (prefixo `samples/`,
 * `kind: "sample"`).
 *
 * Suporta também um sufixo opcional de prompt (`sample_epoch_015_catA.png`)
 * para quando mais de uma amostra for emitida por época — hoje o trainer
 * emite uma única amostra por época, então o agrupamento por prompt colapsa
 * num único grupo "default".
 */

const EPOCH_PATTERN = /epoch_(\d+)/i;
const EPOCH_PROMPT_SUFFIX_PATTERN = /epoch_\d+_([a-zA-Z0-9-]+)\.[a-zA-Z0-9]+$/i;

export function parseSampleEpoch(path: string): number | null {
  const match = path.match(EPOCH_PATTERN);
  return match ? parseInt(match[1], 10) : null;
}

export function parseSamplePromptKey(path: string): string {
  const fname = path.split("/").pop() ?? path;
  const match = fname.match(EPOCH_PROMPT_SUFFIX_PATTERN);
  return match?.[1] ?? "default";
}

export function formatSampleLabel(path: string): string {
  const epoch = parseSampleEpoch(path);
  if (epoch != null) {
    return epoch === 0 ? "Baseline (Época 0)" : `Época ${epoch}`;
  }
  const fname = path.split("/").pop() || path;
  return fname.replace(/\.[^/.]+$/, "");
}

export function isSampleArtifact(art: JobArtifact): boolean {
  return (
    art.kind === "sample" ||
    art.path.startsWith("samples/") ||
    art.path.includes("sample_epoch_") ||
    (art.path.endsWith(".png") && art.kind !== "model" && art.kind !== "metrics")
  );
}

export interface SamplePromptGroup {
  promptKey: string;
  art: JobArtifact;
}

export interface SampleStep {
  epoch: number;
  label: string;
  isBaseline: boolean;
  groups: SamplePromptGroup[];
}

export interface SampleTimeline {
  steps: SampleStep[];
  untagged: JobArtifact[];
}

/**
 * Agrupa artefatos de samples em uma timeline ordenada por época (baseline
 * primeiro), agrupando amostras da mesma época por prompt quando houver mais
 * de uma. Artefatos sem época reconhecível ficam em `untagged`, preservando
 * a ordem recebida do servidor.
 */
export function buildSampleTimeline(artifacts: JobArtifact[]): SampleTimeline {
  const filtered = artifacts.filter(isSampleArtifact);
  const byEpoch = new Map<number, SamplePromptGroup[]>();
  const untagged: JobArtifact[] = [];

  for (const art of filtered) {
    const epoch = parseSampleEpoch(art.path);
    if (epoch == null) {
      untagged.push(art);
      continue;
    }
    const promptKey = parseSamplePromptKey(art.path);
    const list = byEpoch.get(epoch) ?? [];
    list.push({ promptKey, art });
    byEpoch.set(epoch, list);
  }

  const steps: SampleStep[] = Array.from(byEpoch.entries())
    .sort((a, b) => a[0] - b[0])
    .map(([epoch, groups]) => ({
      epoch,
      label: formatSampleLabel(groups[0].art.path),
      isBaseline: epoch === 0,
      groups: [...groups].sort((a, b) => a.promptKey.localeCompare(b.promptKey)),
    }));

  return { steps, untagged };
}
