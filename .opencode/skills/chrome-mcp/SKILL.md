---
name: chrome-mcp
description: Debugging web com Chrome DevTools MCP. Use ao inspecionar DOM, erros de console, requests de rede ou capturar screenshots do frontend em apps/web. Garante que o Chrome com remote-debugging esteja ativo na porta 9222 antes de invocar as ferramentas chrome-devtools.
---

## MCP: Google Chrome (chrome-devtools)

Sempre que for utilizar as ferramentas do MCP `chrome-devtools`:
1. Verifique se a porta 9222 está respondendo (`curl -s http://127.0.0.1:9222/json/version`).
2. Se não estiver ativa, inicie o Chrome em segundo plano como tarefa antes de invocar o MCP:
   - **Modo Headless (em segundo plano):**
     ```bash
     flatpak run --share=network com.google.Chrome --remote-debugging-port=9222 --user-data-dir=/home/felipecn/.var/app/com.google.Chrome/chrome-mcp --headless=new "about:blank"
     ```
   - **Modo Visual (com janela aberta na tela):**
     ```bash
     flatpak run --share=network com.google.Chrome --remote-debugging-port=9222 --user-data-dir=/home/felipecn/.var/app/com.google.Chrome/chrome-mcp "about:blank"
     ```
3. Aguarde a confirmação de conexão antes de executar comandos como `list_pages`, `navigate_page` ou `take_snapshot`.