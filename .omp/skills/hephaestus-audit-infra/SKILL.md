---
name: hephaestus-audit-infra
description: Auditoria somente leitura da infraestrutura (infra/ compose.yaml, compose.prod.yaml, compose.gpu.yaml, compose.integ.yaml, Caddy, SeaweedFS, Postgres, Dockerfiles, scripts, segredos) do Hephaestus LLM Studio para padronizar perfis, blindar segurança e resiliência e garantir reprodutibilidade. Produz tasks/infra-auditoria.md.
---

# Objetivo
Fazer uma AUDITORIA SOMENTE LEITURA da infraestrutura do Hephaestus LLM Studio (`infra/`: compose.yaml, compose.prod.yaml, compose.gpu.yaml, compose.integ.yaml, Caddyfile, scripts, Dockerfiles, seaweedfs-s3.json, runbooks, CI) para padronizar os perfis, eliminar duplicação entre overlays, fechar lacunas de segurança e resiliência e tornar dev, prod e nó GPU reproduzíveis e operáveis só com a documentação.
"Independência e autonomia" significa: qualquer pessoa sobe dev, prod ou um novo nó GPU seguindo o README, sem conhecimento tácito, e nenhum comando de rotina pode destruir dados por engano.

Documentação base: `docs/infra/overview.md`, `docs/infra/gpu-nodes.md`, `docs/infra/storage-and-persistence.md` — trate como HIPÓTESE. Registre toda divergência entre a documentação e os arquivos reais.

# Regras (mais rígidas que nas outras auditorias: aqui um comando errado apaga dados)
- NÃO execute NENHUM comando do runbook ou dos docs: nada de `docker compose up/down/restart/run/exec/build/pull`, NUNCA `down -v`, nada de `docker volume/rm/prune`, nada de `ssh` para o TrueNAS ou qualquer outra máquina, nenhum `curl` em serviço rodando, nenhum acesso ao Docker socket, ao banco, ao S3 ou a volumes.
- Única interação permitida com o Docker: `docker compose ... config -q` para validar sintaxe das 4 combinações (dev; dev+prod; gpu; dev+integ) e `docker compose ... config --no-interpolate > /tmp/...` para ver a estrutura mesclada. NUNCA imprima o `config` interpolado (ele expõe segredos).
- Teste de fail-fast permitido: `env -i PATH="$PATH" docker compose -f infra/compose.yaml -f infra/compose.prod.yaml config -q` (sem credenciais no ambiente) para verificar se prod realmente recusa subir com credenciais ausentes ou padrão.
- NÃO leia nem imprima `.env*`, `infra/env.gpu` ou qualquer arquivo com segredos reais (só `*.example`). Se achar credencial em texto claro em arquivo versionado (ex.: `seaweedfs-s3.json`, compose, scripts, docs, histórico git), cite só caminho:linha e o TIPO do segredo, NUNCA o valor.
- Comandos de análise permitidos: wc, rg, `git ls-files`, `git check-ignore`, `git log` (somente leitura), e ferramentas JÁ instaladas: hadolint, shellcheck, yamllint, trivy config, dockle, gitleaks, `caddy validate`. Ferramenta ausente vira recomendação. NÃO instale nada e NÃO altere nenhum arquivo do repositório.
- Não altere Dockerfiles, compose, Caddyfile nem scripts. Só o entregável em `tasks/` pode ser criado.
- Toda afirmação precisa de evidência: caminho:linha e números.

# Invariantes a preservar (verificar se os arquivos realmente os respeitam)
- Engines sem nenhuma porta publicada no host; cadeia obrigatória Web → api-principal → manager → orchestrator → engine
- Binds em 127.0.0.1 para db, seaweedfs (S3 e master), manager, orchestrator-local e embedder (sem auth); apenas web (:3000) e api-principal (:8080) abertos em dev
- Overlay de prod: ingress Caddy único (:80/:443), `ports: !override []` em web e principal, `/api/*` e `/metrics` → principal, resto → web, headers de segurança
- Fail-fast em prod contra credenciais padrão
- Imagem do Postgres pinada por digest; healthcheck do db; principal só sobe com `service_healthy`; migrações SQLx aplicadas no boot
- `s3-init` idempotente cria o bucket `heph-data`; identidade S3 do orchestrator com menor privilégio (prefixos packages/, artifacts/, models/)
- Nó GPU: `max_concurrent_jobs: 1`, `count: all` para telemetria, GPU por job via `ORCH_GPU_DEVICES`, porta 8082 acessível somente pelo IP do dev host
- Containers de aplicação não-root; rotação de logs em todos os serviços; limites de recursos declarados no overlay de prod

# Fase 1 — Reconhecimento (agente principal)
1. Ler AGENTS.md, todos os compose, Caddyfile, `infra/scripts/*`, README.md e README-gpu.md, `*.example` de env, Dockerfiles de todos os serviços e engines, `.dockerignore`, `.gitignore`, pipelines de CI (Gitea etc.) e `packages/policies/vram-table.yaml`.
2. Inventário quantitativo: nº de serviços por perfil; linhas por arquivo; blocos repetidos entre compose (environment, healthcheck, logging, limites, volumes); variáveis de ambiente definidas em mais de um lugar; imagens e tags (digest, versão fixa, `latest`, `:local`); portas publicadas por perfil e seu bind.
3. Descobrir o que a documentação NÃO diz: como as imagens dos trainers chegam ao nó GPU, quem é dono das migrações e da ordem de boot (principal × manager), políticas de restart, `stop_grace_period`, PID 1 (`init`), retenção dos volumes `models` e `outputs`, monitoramento de disco, alertas, automação de backup.
4. Propor o padrão-alvo, partindo do que já existe:
   - Template de serviço reutilizável (âncoras `x-` ou `extends`/`include`): logging, restart, init, stop_grace_period, `no-new-privileges`, `cap_drop`, usuário não-root, healthcheck, limites
   - Uma fonte só por conceito: versões e digests, portas e binds (variáveis com default seguro), URLs internas e públicas
   - Overlays contendo só o delta: prod = dev + diferenças, sem copiar serviços inteiros
   - Segredos por arquivo/Docker secret, com `${VAR:?}` em prod e nenhum default fraco fora de dev
   - Scripts POSIX idempotentes (`set -eu`), passando no shellcheck
   - Operação por alvo único (Makefile/justfile/script) com guardrails, em especial `down -v` exigindo confirmação explícita

# Fase 2 — Varredura paralela (um subagente por fatia)
a) `compose.yaml` (dev): serviços, âncoras, healthchecks, depends_on, restart, binds, volumes, defaults de env, ordem de boot (principal × manager × orchestrator)
b) `compose.prod.yaml` + Caddyfile + Dockerfile do web: coerência do overlay, `!override`, TLS e `X-Forwarded-*`, headers de segurança (HSTS, CSP, política de referrer), `/metrics` (como é protegido?), SSE sem buffer nem compressão, timeouts e limite de corpo para uploads longos, fail-fast, limites de recursos, standalone
c) `compose.gpu.yaml` + README-gpu.md + `env.gpu.example`: pareamento, GPUs, socket, redes (`ENGINE_NETWORK` no projeto `-p gpu`), volumes (o trecho do doc não mostra cache `models` no nó GPU), distribuição das imagens dos trainers, coerência entre firewall documentado e binds reais
d) `compose.integ.yaml` + CI: hermeticidade, `ENGINE_MOCK=1`, credenciais de teste isoladas de prod, se o exit code do CI reflete a falha dos testes (ex.: `--exit-code-from`), cache, tempo, gates de lint/scan
e) Dockerfiles (web, serviços Rust, engines, trainers): base pinada, multi-stage, non-root, `.dockerignore`, cache mounts (o binário é copiado para fora do cache mount dentro do mesmo RUN?), tamanho de imagem, HEALTHCHECK, `USER`, uid/gid coerente nos volumes compartilhados entre orchestrator e trainers, tag `:local`
f) Armazenamento: Postgres (pin, tuning vs limite de RAM e nº de conexões dos pools sqlx, healthcheck, estratégia de upgrade de major), SeaweedFS (`seaweedfs-s3.json`, `ensure-bucket.sh`, GC, `S3_ENDPOINT_URL` vs `S3_PUBLIC_ENDPOINT_URL`), volumes e retenção de `datasets`, `models` e `outputs`
g) Backup, restauração e desastre: automação, retenção, criptografia, destino fora da máquina, consistência PG↔S3 (dumps em momentos diferentes), `rclone sync` espelhando deleções, restore testado, credenciais admin expostas em linha de comando, guardrails contra `down -v`, RPO/RTO implícitos
h) Segredos e superfície de ataque: inventário SEM valores (onde nasce, default, quem consome, rotação), cobertura real do fail-fast (a lista do doc cita STUDIO_PASSWORD, STUDIO_MASTER_KEY e MANAGER_TOKEN; e `AUTH_SECRET`, `POSTGRES_PASSWORD` e as chaves S3?), segredos versionados, Docker secrets, `AUTO_ADOPT_LOCAL` ausente de prod, tokens por nó, usuários, capabilities, redes, Docker socket
i) Observabilidade, scripts e docs: rotação de logs em TODOS os serviços (inclui `ingress`, `embedder`, `s3-init`), healthchecks, restart, `/metrics`, alertas, disco, shellcheck, drift do runbook e dos "comandos canônicos" em relação ao que existe, IPs e paths de máquinas hardcoded em arquivos que deveriam ser configuráveis

Verificações cruzadas obrigatórias (cada uma vira item no entregável, confirmada ou refutada, lendo o lado Rust/web só para comparar):
1. O nó GPU precisa alcançar `manager:8081` e `seaweedfs:8333` pelo IP da LAN, mas o padrão é bind em 127.0.0.1 (`MANAGER_PUBLISH`, `SEAWEED_PUBLISH`). Como isso é resolvido e documentado, e que exposição resulta (manager com token compartilhado e S3 em HTTP na LAN)?
2. Heartbeat de 5s (padrão do orchestrator) vs nó considerado `stale` após 10s (manager): os dois valores estão acoplados por configuração ou são números soltos em dois serviços?
3. O cookie `heph_session` só recebe `Secure` "em conexões TLS": atrás do Caddy o api-principal vê HTTP. Ele respeita `X-Forwarded-Proto`?
4. Dev passa `/api/*` pelo rewrite do Next (com limite de corpo e timeout próprios), enquanto prod vai direto Caddy → principal. Limites de corpo, timeouts e SSE têm paridade entre os dois caminhos?
5. O doc recomenda `docker-socket-proxy` "bloqueando privileged e volumes". Em geral esses proxies filtram por endpoint/verbo, não pelo corpo do `containers/create`. Levantar quais chamadas à API do Docker o orchestrator realmente faz (código Rust) e o que um proxy conseguiria de fato impor.
6. Como as imagens `hephaestus/trainer-*:local` chegam ao nó GPU (o runbook só faz `up -d` do orchestrator)? E como as engines alcançam o S3 fora da rede `infra_default`?
7. Quem aplica as migrações SQLx (só o principal?) e o manager pode subir antes do schema existir? O banco é backupado antes de migrar?
8. Existe pinagem heterogênea (Postgres por digest, `alpine:3.20`, Caddy 2.8 e trainers `:local` por tag)? Qual é a política?

Cada subagente devolve um relatório estruturado, sem implementar, procurando:
- Blocos duplicados entre compose e serviços sem o template padrão; overlays que reescrevem serviço inteiro em vez de delta
- Serviços sem healthcheck, restart, limites, logging, `init`, `stop_grace_period` ou usuário não-root
- Portas publicadas fora da regra do perfil; `0.0.0.0` implícito (porta sem bind explícito)
- Defaults inseguros fora de dev; credenciais estáticas versionadas; segredos passados por env em vez de secret
- Imagens sem pin, `latest`, `:local`; imagens grandes; falta de `.dockerignore`
- Volumes sem política de retenção ou limite; caminhos e IPs de máquinas hardcoded
- Scripts sem `set -eu`, não idempotentes ou com achados de shellcheck
- Documentação que descreve uma coisa e arquivos que fazem outra

# Fase 3 — Consolidação (agente principal)
- Deduplicar entre subagentes e validar por amostragem abrindo os arquivos citados
- Classificar cada item por: categoria (segurança, resiliência, consistência, performance, operação, docs), perfil(is) afetado(s), impacto (A/M/B), esforço (P/M/G), risco de regressão, prioridade (P0–P3), tipo (quick win / estrutural), e três flags: "altera comportamento de produção?", "exige downtime ou recriação de volume/imagem?" e "afeta o nó GPU remoto?"
- Ordem sugerida: guardrails contra perda de dados e segredos padrão → template de serviço e overlays enxutos → segurança de rede e socket → backup e restore → observabilidade → otimização de build
- Toda mudança em prod ou no nó GPU deve trazer plano de rollout e de rollback

# Entregável
Criar `tasks/infra-auditoria.md`, seguindo as convenções de tasks do AGENTS.md, com:
1. Resumo executivo + métricas (nº de serviços por perfil; % com healthcheck, limites, logging padrão, usuário não-root, pin por digest; portas publicadas por perfil; linhas duplicadas entre compose)
2. Divergências entre a documentação e os arquivos reais
3. Matriz de conformidade serviço × perfil (dev, prod, gpu, integ): imagem e pin, portas e bind, healthcheck, restart, limites, logging, usuário, depends_on, volumes, origem dos segredos
4. Mapa REAL de rede e fluxos (portas, binds, redes, quem fala com quem) vs. as regras inegociáveis
5. Inventário de segredos e credenciais (sem valores): origem, default, consumidor, rotação e cobertura do fail-fast
6. Verificação dos invariantes e das 8 verificações cruzadas: respeitado, parcial ou violado, com evidência
7. Resiliência e recuperação de desastre: backup, restore, retenção, disco, falhas, RPO/RTO atuais vs. desejados (apenas mapeado)
8. Achados: tabela geral + cada tarefa com ID, evidência (caminho:linha), problema, proposta, critério de aceite (incluindo `config -q` verde nas 4 combinações e o teste de fail-fast), esforço, risco, plano de rollout/rollback e dependências
9. Roadmap em fases (cada fase = um PR independente e verificável) e o que NÃO mudar
10. Texto sugerido de "Convenções de Infra" para o AGENTS.md, incluindo um checklist de "adicionar um novo serviço ao compose" (só proposta; não edite o AGENTS.md)

No chat, responda só com os 5 achados mais críticos e o caminho do arquivo.
