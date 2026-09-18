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

// Faixas de leitura (nunca ler o arquivo inteiro — Chromium limita Blob/FileReader a 2GB).
const TAIL_READ_SIZE = 65557; // 65535 (comentário máx.) + 22 (EOCD mín.)
const CD_READ_CAP = 32 * 1024 * 1024; // defesa: CD de ~865 entradas ≈ 100KB
const ZIP64_EOCD_RECORD_SIZE = 56; // 48 + 8 (cdOffset u64)
const MAX_TEXT_ENTRY_BYTES = 32 * 1024 * 1024; // manifest/yaml são textos pequenos

async function readZipEntries(file: File): Promise<ZipEntry[]> {
  // a. Cauda do arquivo (≤64KB+22) — EOCD mora nos últimos bytes.
  const tailSize = Math.min(file.size, TAIL_READ_SIZE);
  const tailStart = file.size - tailSize;
  const tail = await file.slice(tailStart, file.size).arrayBuffer();
  const view = new DataView(tail);

  // Busca a assinatura EOCD (0x06054b50) de trás para frente (offsets relativos à cauda).
  let eocdRel = -1;
  for (let i = tail.byteLength - 22; i >= 0; i--) {
    if (view.getUint32(i, true) === 0x06054b50) {
      eocdRel = i;
      break;
    }
  }

  if (eocdRel === -1) {
    throw new Error("Arquivo ZIP inválido ou corrompido (EOCD não encontrado).");
  }

  const eocdAbs = tailStart + eocdRel;
  const entriesThisDisk = view.getUint16(eocdRel + 8, true);
  let totalEntries = view.getUint16(eocdRel + 10, true);
  let cdSize = view.getUint32(eocdRel + 12, true);
  let cdOffset = view.getUint32(eocdRel + 16, true);

  // b. ZIP64 obrigatório: qualquer campo sentinel força a leitura do ZIP64 EOCD.
  if (
    entriesThisDisk === 0xffff ||
    totalEntries === 0xffff ||
    cdSize === 0xffffffff ||
    cdOffset === 0xffffffff
  ) {
    // Locator de 20 bytes (assinatura 0x07064b50) imediatamente antes do EOCD.
    const locatorAbs = eocdAbs - 20;
    if (locatorAbs < 0) {
      throw new Error("Arquivo ZIP inválido ou corrompido (ZIP64 fora do alcance).");
    }
    const locRel = eocdRel - 20;
    const locBuf =
      locRel >= 0
        ? tail.slice(locRel, locRel + 20)
        : await file.slice(locatorAbs, locatorAbs + 20).arrayBuffer();
    if (locBuf.byteLength < 20) {
      throw new Error("Arquivo ZIP inválido ou corrompido (ZIP64 locator truncado).");
    }
    const locView = new DataView(locBuf);
    if (locView.getUint32(0, true) !== 0x07064b50) {
      throw new Error("Arquivo ZIP inválido ou corrompido (ZIP64 locator não encontrado).");
    }
    const zip64Offset = Number(locView.getBigUint64(8, true));

    // ZIP64 EOCD record (assinatura 0x06064b50, 56+ bytes).
    const recBuf = await file
      .slice(zip64Offset, zip64Offset + ZIP64_EOCD_RECORD_SIZE)
      .arrayBuffer();
    if (recBuf.byteLength < ZIP64_EOCD_RECORD_SIZE) {
      throw new Error("Arquivo ZIP inválido ou corrompido (ZIP64 EOCD truncado).");
    }
    const recView = new DataView(recBuf);
    if (recView.getUint32(0, true) !== 0x06064b50) {
      throw new Error("Arquivo ZIP inválido ou corrompido (ZIP64 EOCD inválido).");
    }
    totalEntries = Number(recView.getBigUint64(32, true));
    cdSize = Number(recView.getBigUint64(40, true));
    cdOffset = Number(recView.getBigUint64(48, true));
  }

  // c. Central directory por faixa (offsets absolutos do arquivo).
  const cdReadSize = Math.min(cdSize, CD_READ_CAP);
  const cdBuf = await file.slice(cdOffset, cdOffset + cdReadSize).arrayBuffer();
  const cdView = new DataView(cdBuf);

  const entries: ZipEntry[] = [];
  let p = 0;

  for (let i = 0; i < totalEntries && p + 46 <= cdBuf.byteLength; i++) {
    if (cdView.getUint32(p, true) !== 0x02014b50) break;

    const compression = cdView.getUint16(p + 10, true);
    let compressedSize = cdView.getUint32(p + 20, true);
    let uncompressedSize = cdView.getUint32(p + 24, true);
    const nameLen = cdView.getUint16(p + 28, true);
    const extraLen = cdView.getUint16(p + 30, true);
    const commentLen = cdView.getUint16(p + 32, true);
    const diskStart = cdView.getUint16(p + 34, true);
    let localOffset = cdView.getUint32(p + 42, true);

    if (p + 46 + nameLen + extraLen + commentLen > cdBuf.byteLength) break;

    // Campo extra ZIP64 (tag 0x0001): u64 na ordem
    // [uncompressed][compressed][localOffset][startDisk], presentes apenas
    // para os campos com sentinel 0xFFFFFFFF/0xFFFF no registro.
    if (
      compressedSize === 0xffffffff ||
      uncompressedSize === 0xffffffff ||
      localOffset === 0xffffffff ||
      diskStart === 0xffff
    ) {
      let ep = p + 46 + nameLen;
      const extraEnd = ep + extraLen;
      while (ep + 4 <= extraEnd) {
        const tag = cdView.getUint16(ep, true);
        const sz = cdView.getUint16(ep + 2, true);
        if (tag === 0x0001) {
          let fp = ep + 4;
          const readU64 = () => {
            if (fp + 8 > extraEnd) {
              throw new Error("Arquivo ZIP inválido ou corrompido (extra ZIP64 truncado).");
            }
            const n = Number(cdView.getBigUint64(fp, true));
            fp += 8;
            return n;
          };
          if (uncompressedSize === 0xffffffff) uncompressedSize = readU64();
          if (compressedSize === 0xffffffff) compressedSize = readU64();
          if (localOffset === 0xffffffff) localOffset = readU64();
          // startDisk (u32 no extra) ignorado: assume-se disco único.
          break;
        }
        ep += 4 + sz;
      }
    }

    const nameBytes = new Uint8Array(cdBuf, p + 46, nameLen);
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

async function extractZipEntryText(file: File, entry: ZipEntry): Promise<string> {
  if (entry.compressedSize > MAX_TEXT_ENTRY_BYTES) {
    throw new Error(`Entrada "${entry.filename}" grande demais para inspeção como texto.`);
  }
  // Header local por faixa: 30 bytes fixos → nameLen/extraLen.
  const headerBuf = await file.slice(entry.localOffset, entry.localOffset + 30).arrayBuffer();
  if (headerBuf.byteLength < 30) {
    throw new Error("Arquivo ZIP inválido ou corrompido (header local truncado).");
  }
  const headerView = new DataView(headerBuf);
  if (headerView.getUint32(0, true) !== 0x04034b50) {
    throw new Error("Arquivo ZIP inválido ou corrompido (header local inválido).");
  }
  const localNameLen = headerView.getUint16(26, true);
  const localExtraLen = headerView.getUint16(28, true);
  const dataStart = entry.localOffset + 30 + localNameLen + localExtraLen;

  // Faixa do prefixo completo (valida que nome+extra estão íntegros).
  const prefix = await file.slice(entry.localOffset, dataStart).arrayBuffer();
  if (prefix.byteLength < 30 + localNameLen + localExtraLen) {
    throw new Error("Arquivo ZIP inválido ou corrompido (header local truncado).");
  }

  // Faixa dos dados (compressedSize já resolvido via ZIP64 no passo do CD).
  const slice = await file.slice(dataStart, dataStart + entry.compressedSize).arrayBuffer();
  if (slice.byteLength < entry.compressedSize) {
    throw new Error("Arquivo ZIP inválido ou corrompido (dados truncados).");
  }

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

  // 1. Procurar por manifest.json (Backup nativo Hephaestus)
  const manifestEntry = entries.find((e) => e.filename === "manifest.json");
  if (manifestEntry) {
    try {
      const text = await extractZipEntryText(file, manifestEntry);
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
      const text = await extractZipEntryText(file, yamlEntry);
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

export async function extractFilesFromDataTransfer(dataTransfer: DataTransfer): Promise<File[]> {
  const result = await inspectDataTransfer(dataTransfer);
  if (result?.folderFiles && result.folderFiles.length > 0) {
    return result.folderFiles;
  }
  const files: File[] = [];
  for (let i = 0; i < dataTransfer.files.length; i++) {
    const f = dataTransfer.files[i];
    if (isImageFile(f.name)) files.push(f);
  }
  return files;
}
