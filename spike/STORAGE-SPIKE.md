# SPIKE 3b.0 — matriz de resultados (aws-sdk-s3 × SeaweedFS)

> **Ramo descartável `spike/storage-seaweedfs`. NÃO mergear.** Este arquivo é a saída
> exigida pela ADR-0003 ("commit no ramo do spike com a matriz de resultados").
> Data: 2026-09-05. Ambiente: host linux, cargo/rustc 1.97.1, docker 29.7 / compose 5.5,
> imagem **`chrislusf/seaweedfs:4.45_full`** (pinnada), SDK **`aws-sdk-s3 1.145.0`**.

## Veredito: **7/7 critérios PASSAM.** D4 (crate) e D3 (presigned) ficam como aprovadas.

| # | Critério | Resultado | Evidência |
|---|---|---|---|
| 1 | `cargo check --workspace` verde com `aws-sdk-s3` **sem** `aws-config` | **PASS** | `Finished dev profile`; registry `index.crates.io` 200; `cargo add` resolveu só aws-sdk-s3 + tipos de credential/runtime (transitivos do SDK), nenhum `aws-config`. |
| 2 | PUT 1 KiB **e** 512 MiB ok, **sem** `aws-chunked`/`STREAMING-*` no wire | **PASS** | PUT 1 KiB 207 ms; PUT 512 MiB (`ByteStream::from_path`) 3.77 s. tcpdump no loopback :8333: `PUT …/wirebig.bin HTTP/1.1` + `content-length: 536870912` exato; `x-amz-content-sha256: 785b0751…` = **SHA real** (não sentinela); **0** ocorrências de `aws-chunked`/`STREAMING-`/`transfer-encoding: chunked` no pcap inteiro. → `RequestChecksumCalculation::WhenRequired` segura a trilha. |
| 3 | Round-trip byte-idêntico (sha256) + `head_object.bytes` igual | **PASS** | `sha_local=785b0751 sha_got=785b0751 idêntico=true head.bytes=1024 len_ok=true`. |
| 4 | Presigned GET funciona **no browser** (sem proxy) + `curl -f` host=200 | **PASS** | `<img src>` de página em `http://localhost:3000` → `http://localhost:8333/…pic.png?X-Amz-…`: Chrome real (CDP) → request `:8333` = `200 Image image/png`, `naturalWidth=8 complete=true`, **zero** console/log errors, **zero** `loadingFailed`. `curl -f` do host = `200 ctype=image/png`, byte-idêntico ao enviado. Cross-origin :3000→:8333 sem CORS (img sem `crossOrigin`). |
| 5 | 1500 objetos → `list_objects_v2` paginado + `delete_objects` ≤3 chamadas | **PASS** | 1500 listados em **2 páginas** (max_keys 1000); `delete_objects` em **2 chamadas** (lotes de 1000); prefixo vazio no re-list. |
| 6 | Boot axum 0.7.9/matchit com as 6 rotas novas sem panic; `/…/data` cai no handler | **PASS** | Router com os 10 paths (4 core + 6 novas) montou sem panic; sondas `oneshot`: as 6 novas → handler certo (`IMAGE_DATA` no `…/data`), `…/images/:imageId` (detail) cai no handler próprio (não no `data` nem no fallback), caminho não-roteado → fallback. |
| 7 | Servidor morto → `put_object` mapeável a `StorageError::Unavailable` em ≤5 s | **PASS** | `docker compose stop` → `put_object` retornou erro em **2.45 ms** (connection refused; `RetryConfig::disabled()` + `TimeoutConfig` 5 s). Porta mapeia a `Unavailable` → 503. |

## Achados que **corrigem o rascunho da ADR-0003** (o rascunho estava NÃO-CHECKED de propósito)

1. **Identidade NÃO é env var.** As envs do rascunho — `S3_ACCESS_KEY`, `S3_SECRET_KEY`,
   `S3_BUCKET_CREATE_OPTIONS` — **não existem** no SeaweedFS. Credenciais vêm de um arquivo
   JSON via flag **`-s3.config=<path>`**:
   ```json
   { "identities": [ { "name": "heph-admin",
       "credentials": [ { "accessKey": "heph", "secretKey": "heph-local-dev" } ],
       "actions": ["Admin","Read","Write","List","Tagging","UserManagement"] } ] }
   ```
   Sem o arquivo, **todo** request → `403 AccessDenied` (o spike provou isso antes de existir
   config). O log de boot reclama de STS (`no signing key found for STS service`) — **ignorar**:
   STS (credenciais temporárias) não é usado; as identidades estáticas do JSON funcionam.

2. **Bucket auto-cria no 1º PUT** → **não** precisa de init-container (confirmado: o PUT de
   1 KiB criou `heph-data` sem nenhum passo de criação). Requisito: a identidade ter ação
   **`Admin`** (a flag `-s3.autoCreateBucket` default `true` só vale para identidades admin).
   Isso é mais forte que o D0 esperava: elimina de vez o `minio-init-bucket`.

3. **Healthcheck precisa de `-ip.bind=0.0.0.0`.** Por default o `weed` amarra o S3 só ao IP da
   interface do container (`172.x:8333`), **não** ao loopback → `wget localhost:8333` do
   healthcheck dá *connection refused* (marca `unhealthy` mesmo servindo 200 do host). Fix
   adotado: `-ip.bind=0.0.0.0` no command **e** healthcheck em `127.0.0.1`. O bind externo
   local-first continua garantido pelo mapeamento `127.0.0.1:8333:8333` (R1 respeitado).

4. **API do SDK (sem `aws-config`) — nomes reais** (evita o loop de tentativa do próximo dev):
   - credenciais: **`aws_sdk_s3::config::Credentials::new(ak, sk, None, None, "provider")`**
     (`from_keys` existe mas é gateado pela feature `hardcoded-credentials`; `new` não precisa).
     `Credentials` **implementa `ProvideCredentials`** → vai direto em `.credentials_provider(..)`;
     **não** há `StaticCredentialsProvider` nesse caminho.
   - `BehaviorVersion`, `Region`, `RequestChecksumCalculation` re-exportados de `aws_sdk_s3::config`.
   - retry/timeout vêm de **`aws_smithy_types`**: `.retry_config(RetryConfig::disabled())`,
     `.timeout_config(TimeoutConfig::builder()…build())` (método `timeout_config`, **não**
     `runtime_config`).
   - presign: **`.presigned(PresigningConfig::expires_in(dur)?)`** na operação `get_object()`
     → `PresignedRequest { .method(), .uri() }` (não há `client.presigner()`).
   - outputs do SDK têm campos privados → usar getters (`.content_length() -> Option<i64>`,
     `.contents() -> &[Object]`, `.key() -> Option<&str>`, `.next_continuation_token()`).

5. **`aws-lc-sys` entra no grafo (atenção ao Docker da 3b.4).** `aws-sdk-s3` → `rustls 0.23` →
   provedor default **`aws-lc-rs` → `aws-lc-sys 0.45`**, cuja `build.rs` compila C/asm. No host
   **buildou só com cc/gcc (sem cmake)**, mas **é lento** (limpar e rebuildar `aws-lc-sys`
   sozinho levou **>10 min**). O `Dockerfile` atual usa `rust:1.97.1-slim` → **confirmar na
   3b.4** que `cargo build --release -p api-principal` linka `aws-lc-sys` nesse image; se
   exigir cmake/nasm, **`apt-get install -y cmake make`** no estágio `build` (ou `mise use -g
   cmake@latest` local para reproduzir), **ou** fixar provedor `ring` nas features do SDK.
   Registrado aqui para não virar surpresa de build na fatia de produção.

6. **Pin de versão:** imagem **`4.45_full`** usada (≥ correção do #6884 sobre presigned-via-proxy,
   que fechou em 2025-05-30; 4.45 é de ago/2026). Manter pin explícito; nunca `:latest`/`:dev`.
   Reexecutar critério 4 se um dia ligar TLS reverse-proxy HTTPS→HTTP na frente do bucket.

## Impacto nas decisões da ADR-0003

- **D4 (crate):** CONFIRMADA. Nenhum critério forçou inversão. `aws-sdk-s3` sem `aws-config`
  compila, faz PUT length-exato sem trilha trailer (2), round-trip (3), presigned (4),
  list/delete em lote (5), erro rápido e mapeável (7).
- **D3 (leitura híbrida presigned):** CONFIRMADA, sem proxy, sem CORS no caminho `<img>`.
- **D8 (porta/`StorageError::Unavailable`):** o erro do SDK no caso "servidor morto" é
  distinguível (connection/dispatch failure) e sub-5s → mapeia a 503 como desenhado.
- **R2** (trilha de checksum do SDK Rust): **desriscado** — provado não-STREAMING no wire até
  512 MiB.
- **R3** (presigned no browser): **desriscado** — Chrome real sem erro de console.
- **Novo risco a registrar (R10):** custo de build do `aws-lc-sys` no `rust:slim` (item 5).

## Arquivos descartáveis deste spike (morrem com o ramo)

- `infra/compose.spike.yaml`, `infra/spike-s3.json` — servidor S3 p/ os critérios.
- `services/api-principal/examples/storage_spike.rs` — harness C2/C3/C4/C5/C7
  (modos por env: default=live, `SPIKE_DEAD=1`=C7, `SPIKE_PUT_ONLY`/`SPIKE_PUT_BIG`=wire-check,
  `SPIKE_UPLOAD_FILE=<png>`=presigned p/ browser).
- `services/api-principal/examples/storage_spike_router.rs` — C6 (roteamento axum/matchit).
- dev-dependencies adicionadas em `services/api-principal/Cargo.toml` + `Cargo.lock`.

**Nada de produção foi escrito.** Os achados duráveis (1–6) serão appêndados à ADR-0003 em
`main` para guiarem a 3b.4 (S3Storage + compose + Dockerfile).
