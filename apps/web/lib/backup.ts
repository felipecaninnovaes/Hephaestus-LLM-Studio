import { ApiError, apiFetch } from "@/lib/api";
import type { Dataset } from "@/types/studio";

function filenameFromDisposition(
  header: string | null,
  fallback: string,
): string {
  if (header) {
    const m = /filename\s*=\s*"?([^";]+)"?/i.exec(header);
    if (m && m[1].trim()) return m[1].trim();
  }
  return fallback;
}

/** Exporta o dataset como .zip: fetch binário raw (cookie) + download via <a>. */
export async function exportDataset(
  id: string,
  slugFallback: string,
): Promise<void> {
  const res = await fetch(`/api/datasets/${id}/export`, {
    method: "POST",
    credentials: "same-origin",
  });
  if (!res.ok) {
    let code = "internal";
    try {
      const envelope = (await res.clone().json()) as {
        code?: string;
        message?: string;
      };
      if (typeof envelope.code === "string" && envelope.code)
        code = envelope.code;
    } catch {
      // Corpo pode ser binário/ilegível — mantém code padrão.
    }
    throw new ApiError(res.status, code, "");
  }
  const blob = await res.blob();
  const filename = filenameFromDisposition(
    res.headers.get("content-disposition"),
    `${slugFallback}.zip`,
  );
  const url = URL.createObjectURL(blob);
  try {
    const a = document.createElement("a");
    a.href = url;
    a.download = filename;
    document.body.appendChild(a);
    a.click();
    a.remove();
  } finally {
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  }
}

export interface ImportDatasetOpts {
  title?: string;
  replace?: boolean;
}

/** Importa um pacote .zip de backup; 201 = Dataset criado. */
export function importDataset(
  file: File,
  opts?: ImportDatasetOpts,
): Promise<Dataset> {
  const form = new FormData();
  form.append("file", file);
  if (opts?.title) form.append("title", opts.title);
  if (opts?.replace) form.append("replace", "true");
  return apiFetch<Dataset>("/api/datasets/import", {
    method: "POST",
    body: form,
  });
}

/** Copy honesta por code — nunca exibe o message cru do servidor. */
export function importErrorMessage(code: string): string {
  switch (code) {
    case "import_invalid":
    case "invalid_request":
      return "Pacote de backup inválido ou corrompido.";
    case "storage_unavailable":
      return "Armazenamento indisponível.";
    default:
      return "Falha ao importar backup.";
  }
}

export function exportErrorMessage(code: string): string {
  switch (code) {
    case "not_found":
      return "Dataset não encontrado.";
    case "storage_unavailable":
      return "Armazenamento indisponível.";
    default:
      return "Falha ao exportar dataset.";
  }
}
