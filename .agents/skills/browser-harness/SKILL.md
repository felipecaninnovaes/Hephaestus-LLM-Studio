---
name: browser-harness
description: Debugging web com o MCP browser-harness (substituto do antigo chrome-devtools). Use ao inspecionar DOM, erros de console, requests de rede ou capturar screenshots do frontend em apps/web. Anexa-se ao Chrome já em execução via CDP — não requer Chrome na porta 9222.
---

## MCP: browser-harness

Configurado em `opencode.json` (`mcp.browser-harness`, via `uvx --from browser-harness[mcp] browser-harness-mcp`). Ferramentas principais:

- `browser_list_tabs` / `browser_current_tab` / `browser_switch_tab` / `browser_new_tab` / `browser_close_tab`
- `browser_goto` — navega a aba ativa
- `browser_screenshot` — PNG (usa `full: true` para página inteira; `max_dim` p/ reduzir tokens)
- `browser_js` — avalia JS no tab (use `getComputedStyle`, leitura de DOM, console via expressão)
- `browser_page_info` — viewport e scroll sizes (útil p/ auditoria responsiva em 375/768/1280 px)
- `browser_http_get` — GET sem browser (headers custom)
- `browser_click`, `browser_type`, `browser_fill`, `browser_press`, `browser_scroll`
- `browser_wait_for_element` / `browser_wait_for_load`
- `browser_cdp` — escape hatch para qualquer método CDP cru (ex.: `Runtime.consoleAPICalled`, `Network.enable` + eventos)
- `browser_start_recording` / `browser_stop_recording` — registra a sessão de ações

Fluxo de verificação:
1. `browser_list_tabs` — se o harness não responder, PARE e reporte `MCP_AUSENTE` (não audite de memória).
2. Se não houver aba do app, `browser_new_tab` com `http://localhost:3000/...` (suba `npm run dev --workspace=web` em background se preciso; mate ao final).
3. Screenshots vão para arquivo temporário — leia-os com a ferramenta Read ou descreva-os via `@visao`.

Nota de harness (carreada do chrome-devtools): `browser_upload_file` usa CDP `setFileInputFiles` e pode entregar File FANTOMA (size 0) para arquivos grandes — falha de upload em teste automatizado não indica parser quebrado; validar com seleção real do usuário.
