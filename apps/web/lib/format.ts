const UNITS = ["B", "KB", "MB", "GB", "TB"] as const;

export function formatBytes(n: number): string {
  if (!Number.isFinite(n) || n <= 0) return "0 B";
  const idx = Math.min(
    Math.floor(Math.log(n) / Math.log(1024)),
    UNITS.length - 1,
  );
  if (idx === 0) return `${Math.floor(n)} B`;
  const value = n / 1024 ** idx;
  const rounded = Math.round(value * 10) / 10;
  const str = Number.isInteger(rounded)
    ? String(rounded)
    : rounded.toFixed(1).replace(".", ",");
  return `${str} ${UNITS[idx]}`;
}

export function formatRelativeTime(iso: string): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return "";
  const diffMs = Date.now() - date.getTime();
  if (diffMs < 0) return "agora mesmo";
  const seconds = Math.floor(diffMs / 1000);
  if (seconds < 45) return "agora mesmo";
  const minutes = Math.floor(seconds / 60);
  if (minutes < 1) return "agora mesmo";
  if (minutes === 1) return "há 1 minuto";
  if (minutes < 60) return `há ${minutes} minutos`;
  const hours = Math.floor(minutes / 60);
  if (hours === 1) return "há 1 hora";
  if (hours < 24) return `há ${hours} horas`;
  const days = Math.floor(hours / 24);
  if (days === 1) return "há 1 dia";
  if (days <= 30) return `há ${days} dias`;
  return new Intl.DateTimeFormat("pt-BR", {
    day: "2-digit",
    month: "2-digit",
    year: "numeric",
  }).format(date);
}

export function formatPercent(part: number, total: number): string {
  if (!Number.isFinite(part) || !Number.isFinite(total) || total <= 0) return "0%";
  return `${Math.round((part / total) * 100)}%`;
}

export function formatDuration(start: string, end: string | null): string {
  const ms =
    (end ? new Date(end).getTime() : Date.now()) - new Date(start).getTime();
  if (ms < 0) return "—";
  const s = Math.floor(ms / 1000);
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ${s % 60}s`;
  const h = Math.floor(m / 60);
  return `${h}h ${m % 60}m`;
}

