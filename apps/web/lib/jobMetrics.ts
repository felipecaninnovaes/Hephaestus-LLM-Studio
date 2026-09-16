import type { JobMetrics } from "@/types/studio";

/**
 * Predicado de métrica de treino (espelha `is_training_metric` do orquestrador,
 * ADR-0024 D1): um ponto é métrica ⇔ tem ao menos um valor numérico FINITO
 * relevante — `loss`/`lr` (qualquer valor finito) OU `boxLoss`/`clsLoss`/
 * `dflLoss`/`map50`/`map5095` finitos e diferentes de zero. Linhas de status do
 * engine de difusão (boot: phase/message com epoch=0 e step sequencial, sem
 * loss) e as linhas por-imagem do AutoLabel (zeros) NÃO são métricas de treino
 * e não devem alimentar gráfico, chips nem contagem de checkpoints.
 */
export function isTrainingMetric(m: JobMetrics): boolean {
  if (Number.isFinite(m.loss)) return true;
  if (Number.isFinite(m.lr)) return true;
  if (Number.isFinite(m.boxLoss) && m.boxLoss !== 0) return true;
  if (Number.isFinite(m.clsLoss) && m.clsLoss !== 0) return true;
  if (Number.isFinite(m.dflLoss) && m.dflLoss !== 0) return true;
  if (Number.isFinite(m.map50) && m.map50 !== 0) return true;
  if (Number.isFinite(m.map5095) && m.map5095 !== 0) return true;
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
