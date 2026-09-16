"use client";

import { useCallback, useEffect, useState } from "react";
import {
  IconRefresh,
  IconServer,
  IconPlus,
} from "@/components/icons";
import { ApiError } from "@/lib/api";
import {
  listOrchestrators,
  adoptOrchestrator,
  revokeOrchestrator,
  orchestratorErrorMessage,
  STATUS_LABELS,
  STATUS_CLASSES,
  nodeMetrics,
  type Orchestrator,
} from "@/lib/monitoring";
import { Button } from "@/components/ui/Button";
import { GlassCard } from "@/components/ui/GlassCard";
import { MetricTile } from "@/components/ui/MetricTile";
import { TruncatedText } from "@/components/ui/TruncatedText";
import { EmptyState } from "@/components/ui/EmptyState";
import { Modal } from "@/components/ui/Modal";
import { Input } from "@/components/ui/Input";
import ConfirmDialog from "@/components/studio/ConfirmDialog";
import { showToast } from "@/components/studio/Toast";
import { formatRelativeTime } from "@/lib/format";

/* ── Helpers ────────────────────────────────────────────────── */

const fmt = (v: number | null | undefined, decimals = 1, suffix = ""): string =>
  v != null ? `${v.toFixed(decimals)}${suffix}` : "—";

/* ── Adopt Modal ────────────────────────────────────────────── */

type Kind = "local" | "remoto";

interface AdoptModalProps {
  open: boolean;
  onClose: () => void;
  onSuccess: () => void;
  /** Pré-preenche endpoint (re-adotar). */
  initialEndpoint?: string;
}

function AdoptModal({
  open,
  onClose,
  onSuccess,
  initialEndpoint,
}: AdoptModalProps) {
  const [name, setName] = useState("");
  const [endpoint, setEndpoint] = useState(initialEndpoint ?? "");
  const [kind, setKind] = useState<Kind>("local");
  const [pairingCode, setPairingCode] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Reset ao abrir/fechar
  useEffect(() => {
    if (open) {
      setEndpoint(initialEndpoint ?? "");
      setName("");
      setKind("local");
      setPairingCode("");
      setError(null);
    }
  }, [open, initialEndpoint]);

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (!name.trim() || !endpoint.trim() || !pairingCode.trim()) return;
    if (!/^https?:\/\//i.test(endpoint.trim())) return;
    setBusy(true);
    setError(null);
    try {
      await adoptOrchestrator({
        name: name.trim(),
        endpoint: endpoint.trim(),
        kind,
        pairingCode: pairingCode.trim(),
      });
      showToast("Orquestrador adotado com sucesso.", "success");
      onSuccess();
    } catch (err) {
      if (err instanceof ApiError) {
        setError(orchestratorErrorMessage(err.status, err.code));
      } else {
        setError("Falha ao adotar orquestrador.");
      }
    } finally {
      setBusy(false);
    }
  }

  return (
    <Modal
      open={open}
      onClose={onClose}
      title={initialEndpoint ? "Re-adotar Orquestrador" : "Adotar Orquestrador"}
      description="Registre um orquestrador remoto ou local no manager."
      icon={<IconServer className="size-4" />}
      maxWidth="md"
      busy={busy}
    >
      <form onSubmit={handleSubmit} className="space-y-4">
        <Input
          label="Nome"
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="Ex: TrueNAS-3060"
          disabled={busy}
          required
        />

        <Input
          label="Endpoint"
          value={endpoint}
          onChange={(e) => setEndpoint(e.target.value)}
          placeholder="http://10.15.1.2:8082"
          fontMono
          disabled={busy || Boolean(initialEndpoint)}
          required
        />

        {/* Kind pills */}
        <div>
          <label className="tracking-caps mb-1.5 block font-mono text-2xs font-medium uppercase text-zinc-300">
            Tipo
          </label>
          <div className="flex gap-2">
            {(["local", "remoto"] as Kind[]).map((k) => (
              <button
                key={k}
                type="button"
                onClick={() => setKind(k)}
                disabled={busy}
                className={`h-8 rounded-lg border px-3 text-xs font-medium transition active:scale-[0.985] cursor-pointer ${
                  kind === k
                    ? "border-brand-500/30 bg-brand-500/[0.12] text-white"
                    : "border-white/10 bg-white/[0.03] text-zinc-400 hover:text-zinc-200 hover:bg-white/[0.06]"
                }`}
              >
                {k === "local" ? "Local" : "Remoto"}
              </button>
            ))}
          </div>
        </div>

        <Input
          label="Código de Pareamento"
          value={pairingCode}
          onChange={(e) => setPairingCode(e.target.value)}
          placeholder="cole o código do log do orquestrador"
          fontMono
          disabled={busy}
          required
        />

        {error && (
          <p className="rounded-lg border border-status-danger/30 bg-status-danger/[0.08] px-3 py-2 font-mono text-2xs text-status-danger">
            {error}
          </p>
        )}

        <div className="flex justify-end gap-2 pt-2">
          <Button
            type="button"
            variant="ghost"
            size="md"
            onClick={onClose}
            disabled={busy}
          >
            Cancelar
          </Button>
          <Button
            type="submit"
            variant="primary"
            size="lg"
            loading={busy}
            disabled={!name.trim() || !endpoint.trim() || !pairingCode.trim() || !/^https?:\/\//i.test(endpoint.trim())}
          >
            {busy ? "Adotando…" : "Adotar"}
          </Button>
        </div>
      </form>
    </Modal>
  );
}

/* ── Page ───────────────────────────────────────────────────── */

export default function EnvironmentsPage() {
  const [orchestrators, setOrchestrators] = useState<Orchestrator[]>([]);
  const [loading, setLoading] = useState(true);
  const [unavailable, setUnavailable] = useState(false);
  const [adoptOpen, setAdoptOpen] = useState(false);
  const [adoptEndpoint, setAdoptEndpoint] = useState<string | undefined>();
  const [revokeTarget, setRevokeTarget] = useState<Orchestrator | null>(null);
  const [revokeBusy, setRevokeBusy] = useState(false);

  const fetchData = useCallback(async () => {
    if (
      typeof document !== "undefined" &&
      document.visibilityState === "hidden"
    ) {
      return;
    }
    try {
      const data = await listOrchestrators();
      setOrchestrators(data.items);
      setUnavailable(false);
    } catch (err) {
      if (
        err instanceof ApiError &&
        err.code === "queue_unavailable"
      ) {
        setUnavailable(true);
      }
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    fetchData();
    const interval = setInterval(fetchData, 3000);
    const handleVisibilityChange = () => {
      if (
        typeof document !== "undefined" &&
        document.visibilityState === "visible"
      ) {
        fetchData();
      }
    };
    document.addEventListener("visibilitychange", handleVisibilityChange);
    return () => {
      clearInterval(interval);
      document.removeEventListener("visibilitychange", handleVisibilityChange);
    };
  }, [fetchData]);

  async function handleRevoke() {
    if (!revokeTarget) return;
    setRevokeBusy(true);
    try {
      await revokeOrchestrator(revokeTarget.id);
      showToast("Orquestrador revogado.", "success");
      setRevokeTarget(null);
      await fetchData();
    } catch (err) {
      if (err instanceof ApiError) {
        showToast(orchestratorErrorMessage(err.status, err.code), "error");
      } else {
        showToast("Falha ao revogar orquestrador.", "error");
      }
    } finally {
      setRevokeBusy(false);
    }
  }

  function handleReAdopt(orch: Orchestrator) {
    setAdoptEndpoint(orch.endpoint);
    setAdoptOpen(true);
  }

  function handleAdoptClose() {
    setAdoptOpen(false);
    setAdoptEndpoint(undefined);
  }

  return (
    <div className="min-h-full space-y-6 p-4 sm:p-6 lg:p-8">
      {/* Header */}
      <div className="flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <div className="font-mono text-2xs font-semibold uppercase tracking-[0.08em] text-zinc-400">
            Infraestrutura
          </div>
          <h1 className="font-display text-2xl font-bold tracking-tight text-white sm:text-3xl">
            Orquestradores
          </h1>
          <p className="mt-1 text-xs text-zinc-400">
            Nós de computação registrados — local e remotos.
          </p>
        </div>

        <div className="flex items-center space-x-2.5">
          <Button
            type="button"
            variant="secondary"
            size="md"
            onClick={() => void fetchData()}
            disabled={loading}
          >
            <IconRefresh
              className={`size-4 ${loading ? "animate-spin text-brand-400" : "text-zinc-400"}`}
            />
            <span>Atualizar</span>
          </Button>
          <Button
            type="button"
            variant="primary"
            size="lg"
            onClick={() => {
              setAdoptEndpoint(undefined);
              setAdoptOpen(true);
            }}
          >
            <IconPlus className="size-4" />
            <span>Adotar</span>
          </Button>
        </div>
      </div>

      {/* Content */}
      {loading && orchestrators.length === 0 ? (
        <div className="glass-card rounded-2xl p-8 text-center text-xs text-zinc-400 font-mono border border-white/10">
          Carregando orquestradores…
        </div>
      ) : orchestrators.length === 0 ? (
        <EmptyState
          icon={<IconServer className="size-5" />}
          title={unavailable ? "Indisponível (manager fora)" : "Nenhum orquestrador adotado"}
          description={
            unavailable
              ? "O manager está fora do ar. Tente novamente mais tarde."
              : "Adote um orquestrador local ou remoto para começar a executar jobs de treino."
          }
          actionLabel={unavailable ? undefined : "Adotar Orquestrador"}
          onAction={unavailable ? undefined : () => setAdoptOpen(true)}
          actionVariant="primary"
        />
      ) : (
        <div className="grid grid-cols-1 gap-5 xl:grid-cols-2">
          {orchestrators.map((node) => {
            const m = nodeMetrics(node);
            const isRevoked = node.status === "revoked";
            return (
              <GlassCard key={node.id} className="p-0 overflow-hidden">
                <div className="space-y-4 p-5 sm:p-6">
                  {/* Header do Card */}
                  <div className="flex flex-col gap-3 border-b border-white/10 pb-4 sm:flex-row sm:items-start sm:justify-between">
                    <div className="min-w-0 space-y-1.5">
                      <div className="flex min-w-0 flex-wrap items-center gap-2">
                        <div className="max-w-full min-w-0 text-lg font-semibold tracking-tight text-white break-words">
                          <TruncatedText text={node.name} as="span" />
                        </div>

                        {/* Badge Kind */}
                        <span className="inline-flex w-fit shrink-0 items-center justify-center gap-1 overflow-hidden whitespace-nowrap rounded-lg border font-medium text-brand-300 bg-brand-500/15 border-brand-500/30 backdrop-blur-sm px-2 py-0.5 text-2xs">
                          {node.kind === "local" ? "Local" : "Remoto"}
                        </span>

                        {/* Status badge */}
                        <div
                          className={`inline-flex w-fit shrink-0 items-center justify-center gap-1 overflow-hidden whitespace-nowrap rounded-lg border font-medium backdrop-blur-sm px-2 py-0.5 text-2xs font-mono ${
                            STATUS_CLASSES[node.status] ?? STATUS_CLASSES.unknown
                          }`}
                        >
                          <span>{STATUS_LABELS[node.status] ?? node.status}</span>
                          {(node.status === "online" || node.status === "degraded") && (
                            <span className="relative ml-1.5 flex h-2 w-2">
                              <span
                                className={`absolute inline-flex h-full w-full animate-ping rounded-full opacity-75 motion-reduce:animate-none ${
                                  node.status === "online" ? "bg-status-success" : "bg-status-alert"
                                }`}
                              />
                              <span
                                className={`relative inline-flex h-2 w-2 rounded-full ${
                                  node.status === "online" ? "bg-status-success" : "bg-status-alert"
                                }`}
                              />
                            </span>
                          )}
                        </div>
                      </div>

                      <div className="flex flex-wrap items-center gap-x-2 gap-y-0.5 text-2xs text-zinc-400">
                        <TruncatedText
                          text={node.endpoint}
                          className="font-mono"
                          as="span"
                        />
                        {node.lastHeartbeat && (
                          <>
                            <span>·</span>
                            <span className="font-mono">
                              visto há {formatRelativeTime(node.lastHeartbeat)}
                            </span>
                          </>
                        )}
                      </div>
                    </div>

                    {/* Ações */}
                    <div className="flex shrink-0 items-center gap-2">
                      {isRevoked ? (
                        <Button
                          type="button"
                          variant="primary"
                          size="sm"
                          onClick={() => handleReAdopt(node)}
                        >
                          Re-adotar
                        </Button>
                      ) : (
                        <Button
                          type="button"
                          variant="destructive"
                          size="sm"
                          onClick={() => setRevokeTarget(node)}
                        >
                          Revogar
                        </Button>
                      )}
                    </div>
                  </div>

                  {/* Capacidade declarada */}
                  {node.vramTotalGb != null && (
                    <div className="flex flex-wrap items-center gap-x-2 gap-y-0.5 text-sm text-zinc-400">
                      <span className="font-medium text-zinc-200">
                        capacidade {node.vramTotalGb} GB
                      </span>
                      {node.jobsActive > 0 && (
                        <>
                          <span className="text-zinc-500">·</span>
                          <span className="font-medium text-zinc-200">
                            {node.jobsActive} job(s) ativo(s)
                          </span>
                        </>
                      )}
                    </div>
                  )}

                  {node.vramTotalGb == null && node.jobsActive > 0 && (
                    <div className="flex flex-wrap items-center gap-x-2 gap-y-0.5 text-sm text-zinc-400">
                      <span className="font-medium text-zinc-200">
                        {node.jobsActive} job(s) ativo(s)
                      </span>
                    </div>
                  )}

                  {/* Medidores de Recursos — por nó (D7) */}
                  {node.measured ? (
                    <div className="border-t border-white/10 pt-4">
                      <div className="grid grid-cols-1 gap-2 sm:grid-cols-3">
                        {/* GPU */}
                        <MetricTile
                          label="USO DA GPU"
                          value={m.gpuLabel ?? "sem GPU"}
                          highlightColor={m.gpuLabel ? "brand" : "default"}
                          subtext={m.hasGpu ? "nome da placa" : "sem GPU instalada"}
                        />

                        {/* VRAM */}
                        <MetricTile
                          label="USO DA VRAM"
                          value={
                            m.vramUsedGb && m.vramTotalGb
                              ? `${m.vramUsedGb} / ${m.vramTotalGb} GB`
                              : "—"
                          }
                          highlightColor="cyan"
                          subtext={fmt(m.vramPct, 1, "%")}
                        />

                        {/* Sistema & Host */}
                        <MetricTile
                          label="SISTEMA & HOST"
                          value={fmt(m.cpuPct, 1, "%")}
                          highlightColor="default"
                          subtext={
                            m.ramUsedGb && m.ramTotalGb
                              ? `${m.ramUsedGb} / ${m.ramTotalGb} GB RAM`
                              : "RAM: —"
                          }
                        />
                      </div>

                      {/* GPUs detalhadas */}
                      {m.hasGpu && node.gpus.length > 0 && (
                        <div className="mt-2 flex flex-wrap gap-1.5">
                          {node.gpus.map((gpu) => (
                            <span
                              key={gpu}
                              className="rounded-full border border-white/10 bg-black/40 px-2.5 py-0.5 font-mono text-2xs text-zinc-300"
                            >
                              {gpu}
                            </span>
                          ))}
                        </div>
                      )}
                    </div>
                  ) : (
                    <div className="border-t border-white/10 pt-4 text-xs text-zinc-500">
                      Telemetria indisponível (heartbeat ausente).
                    </div>
                  )}
                </div>
              </GlassCard>
            );
          })}
        </div>
      )}

      {/* Adopt Modal */}
      <AdoptModal
        open={adoptOpen}
        onClose={handleAdoptClose}
        onSuccess={() => {
          handleAdoptClose();
          void fetchData();
        }}
        initialEndpoint={adoptEndpoint}
      />

      {/* Revoke ConfirmDialog */}
      <ConfirmDialog
        open={Boolean(revokeTarget)}
        title="Revogar orquestrador"
        body={
          <p className="text-xs text-zinc-300">
            Tem certeza de que deseja revogar{" "}
            <strong className="text-white font-mono">{revokeTarget?.name}</strong>{" "}
            (<span className="font-mono text-zinc-400">{revokeTarget?.endpoint}</span>)?
            O nó não receberá mais jobs. Você pode re-adotá-lo depois.
          </p>
        }
        confirmLabel="Sim, revogar"
        danger
        busy={revokeBusy}
        onConfirm={handleRevoke}
        onClose={() => setRevokeTarget(null)}
      />
    </div>
  );
}
