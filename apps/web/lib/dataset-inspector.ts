import type { DatasetType } from "@/types/studio";

export interface InspectionResult {
  title: string;
  category: DatasetType;
  classes: string[];
  imagesCount: number;
  isBackupZip: boolean;
  file?: File;
  folderFiles?: File[];
  sourceLabel: string;
}

const IMAGE_EXTENSIONS = new Set(["jpg", "jpeg", "png", "webp", "bmp", "tiff", "tif", "gif"]);

export function isImageFile(filename: string): boolean {
  const ext = filename.split(".").pop()?.toLowerCase() ?? "";
  return IMAGE_EXTENSIONS.has(ext);
}

function cleanTitle(raw: string): string {
  return raw
    .replace(/\.[^/.]+$/, "")
    .replace(/[-_]+/g, " ")
    .trim()
    .slice(0, 96);
}

function cleanClassName(name: string): string {
  return name
    .toLowerCase()
    .trim()
    .replace(/[^a-z0-9_]/g, "_")
    .replace(/^_+|_+$/g, "")
    .slice(0, 64);
}

interface ZipEntry {
  filename: string;
  compression: number;
  compressedSize: number;
  uncompressedSize: number;
  localOffset: number;
}

async function readZipEntries(file: File): Promise<ZipEntry[]> {
  const buffer = await file.arrayBuffer();
  const view = new DataView(buffer);

  // Search for EOCD signature (0x06054b50) in the last 65535 + 22 bytes
  const maxSearch = Math.min(buffer.byteLength, 65535 + 22);
  const minOffset = buffer.byteLength - maxSearch;
  let eocdOffset = -1;

  for (let i = buffer.byteLength - 22; i >= minOffset; i--) {
    if (view.getUint32(i, true) === 0x06054b50) {
      eocdOffset = i;
      break;
    }
  }

  if (eocdOffset === -1) {
    throw new Error("Arquivo ZIP inválido ou corrompido (EOCD não encontrado).");
  }

  const totalEntries = view.getUint16(eocdOffset + 10, true);
  const cdSize = view.getUint32(eocdOffset + 12, true);
  const cdOffset = view.getUint32(eocdOffset + 16, true);

  const entries: ZipEntry[] = [];
  let p = cdOffset;

  for (let i = 0; i < totalEntries && p < cdOffset + cdSize; i++) {
    if (view.getUint32(p, true) !== 0x02014b50) break;

    const compression = view.getUint16(p + 10, true);
    const compressedSize = view.getUint32(p + 20, true);
    const uncompressedSize = view.getUint32(p + 24, true);
    const nameLen = view.getUint16(p + 28, true);
    const extraLen = view.getUint16(p + 30, true);
    const commentLen = view.getUint16(p + 32, true);
    const localOffset = view.getUint32(p + 42, true);

    const nameBytes = new Uint8Array(buffer, p + 46, nameLen);
    const filename = new TextDecoder().decode(nameBytes);

    entries.push({
      filename,
      compression,
      compressedSize,
      uncompressedSize,
      localOffset,
    });

    p += 46 + nameLen + extraLen + commentLen;
  }

  return entries;
}

async function extractZipEntryText(buffer: ArrayBuffer, entry: ZipEntry): Promise<string> {
  const view = new DataView(buffer);
  const localOffset = entry.localOffset;
  const localNameLen = view.getUint16(localOffset + 26, true);
  const localExtraLen = view.getUint16(localOffset + 28, true);
  const dataStart = localOffset + 30 + localNameLen + localExtraLen;
  const slice = buffer.slice(dataStart, dataStart + entry.compressedSize);

  if (entry.compression === 0) {
    return new TextDecoder().decode(slice);
  }

  if (entry.compression === 8) {
    const ds = new DecompressionStream("deflate-raw");
    const stream = new Response(slice).body?.pipeThrough(ds);
    if (!stream) throw new Error("Decompression stream falhou");
    return await new Response(stream).text();
  }

  throw new Error(`Método de compressão não suportado (${entry.compression})`);
}

export async function inspectZipFile(file: File): Promise<InspectionResult> {
  const entries = await readZipEntries(file);
  const buffer = await file.arrayBuffer();

  // 1. Procurar por manifest.json (Backup nativo Hephaestus)
  const manifestEntry = entries.find((e) => e.filename === "manifest.json");
  if (manifestEntry) {
    try {
      const text = await extractZipEntryText(buffer, manifestEntry);
      const manifest = JSON.parse(text);
      const rawTitle = manifest.dataset?.title || cleanTitle(file.name);
      const category: DatasetType = manifest.dataset?.category || "yolo_bbox";
      const classes = Array.isArray(manifest.classes)
        ? manifest.classes
            .map((c: unknown) => (typeof c === "string" ? c : (c as { name?: string }).name))
            .filter((name: unknown): name is string => typeof name === "string" && name.length > 0)
        : [];
      const imagesCount = Array.isArray(manifest.images) ? manifest.images.length : 0;

      return {
        title: rawTitle,
        category,
        classes,
        imagesCount,
        isBackupZip: true,
        file,
        sourceLabel: "Backup Hephaestus (.zip)",
      };
    } catch {
      // Ignora erro no manifest e tenta alternativas abaixo
    }
  }

  // 2. Procurar por data.yaml / dataset.yaml (Padrão YOLO)
  const yamlEntry = entries.find((e) => e.filename.endsWith(".yaml") || e.filename.endsWith(".yml"));
  if (yamlEntry) {
    try {
      const text = await extractZipEntryText(buffer, yamlEntry);
      const classes: string[] = [];
      // names: ['cat', 'dog'] ou names:\n  0: cat\n  1: dog
      const listMatch = text.match(/names:\s*\[([^\]]+)\]/);
      if (listMatch && listMatch[1]) {
        for (const item of listMatch[1].split(",")) {
          const c = cleanClassName(item.replace(/['"]/g, ""));
          if (c) classes.push(c);
        }
      } else {
        const dictMatches = text.matchAll(/^\s*(?:\d+|-)\s*:\s*['"]?([a-zA-Z0-9_-]+)['"]?/gm);
        for (const m of dictMatches) {
          if (m[1]) {
            const c = cleanClassName(m[1]);
            if (c) classes.push(c);
          }
        }
      }

      const imageEntries = entries.filter((e) => isImageFile(e.filename));

      return {
        title: cleanTitle(file.name),
        category: "yolo_bbox",
        classes,
        imagesCount: imageEntries.length,
        isBackupZip: false,
        file,
        sourceLabel: "Dataset YOLO (.zip)",
      };
    } catch {
      // Continua para fallback
    }
  }

  // 3. Fallback: extrair contagem de imagens e classes de diretórios do ZIP
  const imageEntries = entries.filter((e) => isImageFile(e.filename));
  const classSet = new Set<string>();

  for (const entry of imageEntries) {
    const parts = entry.filename.split("/");
    if (parts.length > 1) {
      const parentDir = parts[parts.length - 2]?.toLowerCase();
      if (parentDir && !["images", "labels", "train", "val", "test", "data"].includes(parentDir)) {
        const clean = cleanClassName(parentDir);
        if (clean) classSet.add(clean);
      }
    }
  }

  return {
    title: cleanTitle(file.name),
    category: "yolo_bbox",
    classes: Array.from(classSet),
    imagesCount: imageEntries.length,
    isBackupZip: false,
    file,
    sourceLabel: "Pacote de Imagens (.zip)",
  };
}

async function traverseDirectory(
  dir: FileSystemDirectoryEntry,
  path: string,
  files: File[],
  classSet: Set<string>,
) {
  const reader = dir.createReader();
  const readAllEntries = async (): Promise<FileSystemEntry[]> => {
    let all: FileSystemEntry[] = [];
    while (true) {
      const batch: FileSystemEntry[] = await new Promise((resolve, reject) => {
        reader.readEntries(resolve, reject);
      });
      if (batch.length === 0) break;
      all = all.concat(batch);
    }
    return all;
  };

  const entries = await readAllEntries();
  for (const entry of entries) {
    if (entry.isDirectory) {
      const lower = entry.name.toLowerCase();
      if (!["images", "labels", "train", "val", "test", "data"].includes(lower)) {
        const clean = cleanClassName(lower);
        if (clean) classSet.add(clean);
      }
      await traverseDirectory(entry as FileSystemDirectoryEntry, `${path}/${entry.name}`, files, classSet);
    } else if (entry.isFile) {
      const file: File = await new Promise((resolve, reject) => {
        (entry as FileSystemFileEntry).file(resolve, reject);
      });
      if (isImageFile(file.name)) {
        files.push(file);
      }
    }
  }
}

export async function inspectDataTransfer(dataTransfer: DataTransfer): Promise<InspectionResult | null> {
  const items = Array.from(dataTransfer.items);
  if (items.length === 0) return null;

  // 1. Caso seja um arquivo .zip
  if (items.length === 1 && items[0].kind === "file") {
    const file = items[0].getAsFile();
    if (file && file.name.toLowerCase().endsWith(".zip")) {
      return await inspectZipFile(file);
    }
  }

  // 2. Caso seja uma pasta ou múltiplos arquivos
  const files: File[] = [];
  const classSet = new Set<string>();
  let rootDirName = "";

  for (const item of items) {
    const entry = item.webkitGetAsEntry?.();
    if (!entry) {
      const file = item.getAsFile();
      if (file && isImageFile(file.name)) files.push(file);
      continue;
    }

    if (entry.isDirectory) {
      if (!rootDirName) rootDirName = entry.name;
      await traverseDirectory(entry as FileSystemDirectoryEntry, "", files, classSet);
    } else if (entry.isFile) {
      const file = item.getAsFile();
      if (file && isImageFile(file.name)) files.push(file);
    }
  }

  if (files.length === 0 && classSet.size === 0) return null;

  return {
    title: rootDirName ? cleanTitle(rootDirName) : "Novo Dataset",
    category: "yolo_bbox",
    classes: Array.from(classSet),
    imagesCount: files.length,
    isBackupZip: false,
    folderFiles: files,
    sourceLabel: rootDirName ? `Pasta "${rootDirName}"` : `${files.length} imagens`,
  };
}
