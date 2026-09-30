import type { JobMetrics, JobTelemetryEvent } from "@/types/studio";

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

/**
 * Janela rolante (em deltas) usada pela ETA de treino: só os deltas mais
 * recentes alimentam a estimativa, para que o custo do load/pré-compute da
 * época 1 saia do cálculo assim que o treino estabiliza.
 */
export const TRAINING_ETA_WINDOW = 30;

/**
 * Teto de delta aceito entre dois eventos de treino: gaps maiores que isso
 * são sample/checkpoint (o step congela enquanto a engine grava/amostra) e
 * inflariam a estimativa se entrassem no cálculo.
 */
export const TRAINING_ETA_STALL_MS = 120_000;

/**
 * Estima os milissegundos restantes de treino a partir da janela de eventos
 * de telemetria (SSE `/api/jobs/:id/events`).
 *
 * Algoritmo: filtra eventos com `phase === "training"` (exclui load de modelo
 * e pré-compute, que superestimariam a época 1), ordena por `timestamp`
 * (tolera reordenação), deriva deltas step/tempo descartando retrocesso de
 * step, step congelado e stalls > `TRAINING_ETA_STALL_MS` (sample/checkpoint),
 * e multiplica a MEDIANA (P50) do ms/step dos últimos `TRAINING_ETA_WINDOW`
 * deltas pelos `totalSteps - step` restantes. A mediana — e não a média —
 * impede que um sample isolado quebre a estimativa.
 *
 * Retorna `null` com <2 pontos válidos, sem `totalSteps` ou com o step já
 * completo. Nunca lança: eventos com campos ausentes/inválidos são ignorados.
 */
export function estimateTrainingEtaMs(
  events: readonly JobTelemetryEvent[] | undefined | null,
): number | null {
  if (!events || events.length < 2) return null;

  const points: { t: number; step: number; totalSteps: number | null }[] = [];
  for (const ev of events) {
    if (!ev || typeof ev.phase !== "string") continue;
    if (ev.phase.toLowerCase() !== "training") continue;
    const t = Date.parse(ev.timestamp);
    if (!Number.isFinite(t)) continue;
    if (typeof ev.step !== "number" || !Number.isFinite(ev.step)) continue;
    points.push({
      t,
      step: ev.step,
      totalSteps:
        typeof ev.totalSteps === "number" && Number.isFinite(ev.totalSteps)
          ? ev.totalSteps
          : null,
    });
  }
  if (points.length < 2) return null;
  points.sort((a, b) => a.t - b.t);

  let totalSteps: number | null = null;
  for (let i = points.length - 1; i >= 0; i--) {
    const total = points[i].totalSteps;
    if (total !== null && total > 0) {
      totalSteps = total;
      break;
    }
  }
  if (totalSteps === null && events) {
    for (let i = events.length - 1; i >= 0; i--) {
      const ev = events[i];
      if (!ev) continue;
      let text: string | null = null;
      if (typeof ev.phaseMessage === "string") {
        text = ev.phaseMessage;
      } else if (
        typeof ev === "object" &&
        "message" in ev &&
        typeof ev.message === "string"
      ) {
        text = ev.message;
      }
      if (text) {
        const m = text.match(/Step\s+\d+\s*\/\s*(\d+)/i);
        if (m) {
          const parsed = Number.parseInt(m[1], 10);
          if (Number.isFinite(parsed) && parsed > 0) {
            totalSteps = parsed;
            break;
          }
        }
      }
    }
  }
  if (totalSteps === null) return null;

  const remaining = totalSteps - points[points.length - 1].step;
  if (!(remaining > 0)) return null;

  const msPerStep: number[] = [];
  const start = Math.max(1, points.length - TRAINING_ETA_WINDOW);
  for (let i = start; i < points.length; i++) {
    const dt = points[i].t - points[i - 1].t;
    const ds = points[i].step - points[i - 1].step;
    if (!(dt > 0) || !(ds > 0)) continue;
    if (dt > TRAINING_ETA_STALL_MS) continue;
    msPerStep.push(dt / ds);
  }
  if (msPerStep.length === 0) return null;
  msPerStep.sort((a, b) => a - b);
  const mid = Math.floor(msPerStep.length / 2);
  const median =
    msPerStep.length % 2 === 1
      ? msPerStep[mid]
      : (msPerStep[mid - 1] + msPerStep[mid]) / 2;
  const eta = median * remaining;
  return Number.isFinite(eta) ? Math.round(eta) : null;
}
