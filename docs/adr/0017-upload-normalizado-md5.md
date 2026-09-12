# ADR-0017 — Upload Normalizado para WebP, Higienização de Metadados e Nomenclatura por Hash MD5

- **Status:** ACEITA (2026-09-12 — aceite do usuário em sessão).
- **Data:** 2026-09-12
- **Componentes:**
  - `services/api-principal`: expansão de codecs de decodificação no `Cargo.toml` (`image`), sniffer estendido para detecção de entrada (JPEG, PNG, WebP, BMP, TIFF, GIF), pipeline de normalização em `normalize.rs` com decodificação, correção de orientação EXIF, descarte de metadados sensíveis (GPS/EXIF/ICC), recodificação para WebP, geração do nome `<md5>.webp`, persistência no S3 e banco.
  - `apps/web`: filtragem prévia de arquivos não-imagem no leitor de pastas de `CreateDatasetModal.tsx`, pipeline de upload com pool de concorrência em `apps/web/lib/images.ts` (até 2 lotes paralelos) para eliminar gargalo de latência sequencial.
- **Fontes:**
  - `docs/adr/0003-object-storage-s3.md` (D2 upload, D5 keys legíveis e extensão do sniffing).
  - `docs/backend.md` §10 (schema `images`).
  - `docs/frontend.md` §10 (contrato de upload).

---

## 1. Contexto e Problema

No pipeline de upload de imagens para datasets (`POST /api/datasets/:id/upload`):
1. **Lentidão:** O frontend fragmenta os envios em lotes pequenos (`BATCH_MAX_FILES = 20`) e os despacha de modo estritamente sequencial (`for ... await`). No backend, cada imagem do lote reabre e relê o arquivo do disco quatro vezes sucessivas (gravação tempfile, 12 bytes do sniffer, decodificação de dimensões e streaming de hashing), além de disparar o indexador CLIP a cada 20 imagens.
2. **Perda de Imagens e Colisão de Nomes:**
   - O sniffer atual aceita apenas JPEG, PNG e WebP. Arquivos em BMP, TIFF, GIF ou imagens sem marcador JPEG padrão são sumariamente descartados (`unsupported_media`).
   - A sanitização de nomes colapsa caracteres especiais em `-`. Ao importar pastas com subpastas de classes (ex: `gatos/001.jpg` e `cachorros/001.jpg`), ambos chegam como `001.jpg`, colidindo no banco em `UNIQUE (dataset_id, filename) WHERE deleted_at IS NULL`. A segunda imagem é descartada silenciosamente como `duplicate` e deletada do storage.
3. **Privacidade e Segurança Digital:** Imagens originais contêm tags EXIF com coordenadas GPS, dados de câmera e identificadores que vazam dados pessoais e de ambiente.

---

## 2. Decisões Numeradas

### D0 — Nome Canônico por Hash MD5 (`<md5>.webp`)
- O nome de armazenamento no S3 e a coluna `filename` no Postgres passam a ser:
  ```
  {md5}.webp
  ```
  onde `{md5}` são os 32 caracteres hexadecimais em minúsculas calculados sobre os bytes da imagem **normalizada final**.
- A chave no S3 (`object_key`) torna-se:
  ```
  datasets/{dataset_id}/images/{image_id}/{md5}.webp
  ```
- **Consequências:**
  - Imagens com conteúdos visuais diferentes que possuíam o mesmo nome local (ex: `001.jpg` em subpastas diferentes) geram hashes distintos e sobem sem colisão.
  - O reenvio do mesmo arquivo gera hash idêntico e é capturado de forma transparente por `ON CONFLICT (dataset_id, filename) WHERE deleted_at IS NULL`, sendo retornado como `duplicate` sem duplicar bytes no storage.

### D1 — Formatos de Entrada Expandidos
- A crate `image` em `api-principal/Cargo.toml` adiciona as features:
  `features = ["jpeg", "png", "webp", "bmp", "tiff", "gif"]`.
- O sniffer reconhece:
  - PNG (`\x89PNG\r\n\x1a\n`)
  - WebP (`RIFF....WEBP`)
  - JPEG (`\xFF\xD8\xFF` ou `\xFF\xD8`)
  - BMP (`BM`)
  - TIFF (`II*\x00` ou `MM\x00*`)
  - GIF (`GIF87a` ou `GIF89a` — primeiro frame)

### D2 — Formato Canônico WebP e Higienização de Metadados
- Ao receber qualquer imagem suportada:
  1. A imagem é decodificada na memória para `DynamicImage`.
  2. A orientação EXIF é normalizada fisicamente nos pixels.
  3. Metadados sensíveis (EXIF, GPS, ICC, XMP) são descartados.
  4. A imagem é codificada para **WebP** (RGB com qualidade 90 para fotos; RGBA lossless quando houver canal de transparência).
- O tipo de mídia persistido em `images.media_type` é sempre `"webp"`.
- Zero necessidade de migration: o schema existente da migration 0003 já aceita `"webp"` no check constraint `CHECK (media_type IN ('jpeg','png','webp'))`.

### D3 — Pipeline Otimizado no Backend
- A leitura, decodificação, normalização e cálculo de hash (`md5` e `sha256`) são feitos em fluxo unificado, eliminando 4 operações redundantes de abertura/leitura de disco por arquivo.
- O upload para o storage S3 recebe diretamente o arquivo normalizado com content-length exato.

### D4 — Concorrência no Frontend e Filtragem Prévia
- `apps/web/lib/images.ts`: o executor de lotes passa a permitir concorrência (2 requisições em voo com pool de promessas).
- `apps/web/components/studio/CreateDatasetModal.tsx`: seleção de pastas filtra arquivos não-imagem (`.txt`, `.yaml`, `.DS_Store`, etc.) no cliente antes de enfileirar.

---

## 3. Plano de Implementação

1. **ADR-0017:** Registro desta especificação.
2. **Backend Codecs & Normalizer:**
   - Adicionar features `bmp`, `tiff`, `gif` ao `image` no `Cargo.toml`.
   - Implementar módulo `normalize.rs` em `api-principal/src/storage/` ou `src/datasets/` para higienização e conversão para WebP.
   - Atualizar sniffer em `storage/sniff.rs`.
   - Atualizar `upload` em `datasets/handlers.rs` para gravar `{md5}.webp` e processar a normalização.
3. **Frontend Otimizações:**
   - Adicionar filtro de arquivos não-imagem em `CreateDatasetModal.tsx`.
   - Concorrência de 2 lotes em paralelo no `uploadImages` em `images.ts`.
4. **Testes & Verificação:**
   - Testes unitários de normalização, decodificação de múltiplos formatos e cálculo de MD5.
   - Testes de integração de upload com colisão de nome local e deduplicação de conteúdo.
   - Build e verificação de integridade no workspace.
