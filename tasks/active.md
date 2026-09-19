# Memória Ativa — Hephaestus LLM Studio

- **Branch atual:** `fix/galeria-thumbnails-performance`
- **Fatia em andamento:** Correção de Performance e Miniaturas na Galeria — Endpoint `/api/generations/:id/thumb`, proxy e fallback on-demand de thumbnails leves, e otimização de renderização na web (CSS containment e memory reduction).

## Checklist Imediato da Sessão Ativa
- [x] Contratos (`packages/contracts/openapi.yaml`): Endpoint `GET /api/generations/{id}/thumb` e atualização do schema `Generation.thumbUrl`
- [x] Backend Rust (`services/api-principal`):
  - [x] Implementar `get_generation_thumb` com busca de `thumb_s3_key` e fallback gerador on-the-fly (`image::DynamicImage::thumbnail`)
  - [x] Atualizar `to_public` com fallback para `/api/generations/{id}/thumb` e `/api/generations/{id}/data` quando sem `public_endpoint`
  - [x] Registrar rota em `auth/routes.rs`
  - [x] Testes unitários cobrindo rota de thumbnail (200 existente, 200 on-the-fly, 404, 503)
- [x] Frontend Web (`apps/web`):
  - [x] Adicionar `getGenerationThumbUrl` em `apps/web/lib/generations.ts`
  - [x] Atualizar `GenerationGallery.tsx` para sempre usar `thumbUrl` / `getGenerationThumbUrl`
  - [x] Corrigir `content-visibility: auto` + `contain-intrinsic-size: 200px 200px` no container dos cards
  - [x] Corrigir `Select.tsx` com `zIndex: 500` (camada `--z-index-popover`), resolvendo dropdowns invisíveis/ocultos atrás de modais (`AutoLabelModal`, `AutoTrackerModal`, etc.)
- [x] Verificação verde: `cargo fmt --all -- --check`, `cargo check --workspace`, `cargo test -p api-principal` (519 passed) e `npm run build --workspace=web`
## Entregas Concluídas Recentemente
- [x] Roadmap de Hardening e Padronização da Infraestrutura (`tasks/infra-auditoria.md`) concluído e integrado.
- [x] Hotfix manager: `report_job` aceita `status: cancelled` pós-abort (commit 4bfb450).

## Protocolo de Retomada (3 Passos)

1. **Conferir Branch e Active:** Confirmar git branch atual (`git status`) e ler `tasks/active.md` para situar a fatia e checklist em andamento.
2. **Checar Integridade com Graft:** Executar `mcp__graft_check_freshness` para validar que o grafo de símbolos e dependências está sincronizado.
3. **Consultar REPO_MAP e Contratos:** Ler `docs/REPO_MAP.md` e `packages/contracts/openapi.yaml` antes de planejar alterações de código ou novos endpoints.
