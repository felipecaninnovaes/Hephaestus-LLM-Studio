import type { JobMetrics } from "@/types/studio";

/**
 * Um ponto de métrica de treino (AC-006-B) tem ao menos um valor numérico de
 * perda/precisão não-nulo e maior que zero. Linhas de status do engine de
 * difusão (boot: phase/message com epoch=0 e step sequencial, sem loss) e as
 * linhas por-imagem do AutoLabel (zeros) NÃO são métricas de treino e não
 * devem alimentar gráfico, chips nem contagem de checkpoints.
 */
export function isTrainingMetric(m: JobMetrics): boolean {
  if (m.loss != null) return true;
  if ((m.map50 ?? 0) > 0) return true;
  if ((m.boxLoss ?? 0) > 0) return true;
  if ((m.clsLoss ?? 0) > 0) return true;
  return false;
}

/** Deriva apenas os pontos reais de métrica de treino a partir do array bruto. */
export function trainingMetrics(metrics: JobMetrics[]): JobMetrics[] {
  return metrics.filter(isTrainingMetric);
}

/** Última métrica de treino (ignora linhas de status/boot). */
export function latestTrainingMetric(
  metrics: JobMetrics[] | undefined | null,
): JobMetrics | null {
  if (!metrics || metrics.length === 0) return null;
  const real = trainingMetrics(metrics);
  return real.length > 0 ? real[real.length - 1] : null;
}
