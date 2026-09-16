# Governança de Versionamento & Boas Práticas de Git

Diretrizes obrigatórias de versionamento, padronização de commits e matriz de permissões para o ecossistema Hephaestus.

---

## 1. Estratégia de Branches

- **Branch Principal (`main`):**
  - Contém código estável, testado e pronto para produção.
  - **Bloqueio Estrito:** É expressamente **proibido realizar commits diretos na `main`**.
- **Taxonomia Obrigatória de Branches:**
  - `feat/<nome-kebab>`: Novas funcionalidades e fatias verticais.
  - `fix/<escopo>-<nome-kebab>`: Correção de bugs.
  - `chore/<escopo>-<nome-kebab>`: Build, infraestrutura, configs, dependências,
    documentação e regras/prompts.
- **Tronco de integração:** o fluxo atual do repo é
  `feat|fix|chore/… → develop → main`. Branches abrem de `develop`
  (ou `main` quando o plano da fatia exigir); merge em `main` só com ordem
  explícita do usuário.
- **Ciclo de Vida:**
  - Branches de curta duração (24h a 72h), deletadas após merge.
  - Merge para `develop` com merge commit descritivo (histórico não-linear);
    branches rebaseadas sobre `develop` atualizada antes do merge.

---

## 2. Padronização de Mensagens (Conventional Commits)

Todas as mensagens de commit devem seguir o padrão Conventional Commits com **escopo obrigatório**:

```text
<tipo>(<escopo>): <descrição em minúsculas>

[corpo opcional explicando o 'porquê' e decisões de design]

[rodapé opcional: referências a issues, breaking changes]
```

- **Tipos Permitidos:**
  - `feat`: Nova funcionalidade
  - `fix`: Correção de bug
  - `docs`: Documentação e guias
  - `style`: Formatação sem impacto na lógica
  - `refactor`: Refatoração estrutural sem alteração de comportamento
  - `perf`: Otimizações de desempenho
  - `test`: Testes unitários ou de integração
  - `build`: Alterações no build ou dependências
  - `ci`: Pipelines e automações de CI/CD
  - `chore`: Tarefas operacionais e manutenção
  - `revert`: Reversão de commits anteriores

- **Escopos Padronizados:**
  - `apps`, `services`, `engines`, `infra`, `packages`, `contracts`, `rules`, `docs`, `deps`, `root`
  - Escopos específicos do Hephaestus: `web`, `api-principal`, `manager`, `orchestrator`, `trainer-yolo`, `trainer-difusao`, `trainer-clip`, `datasets`, `models`, `jobs`.

---

## 3. Política de Commits Incrementais e Atomicidade

- **Faixa de Referência:** **100 a 300 linhas de diff** (adições + deleções por commit), teto orientativo de 400 linhas.
- **Fundamentação:**
  1. *Eficácia na Revisão:* Diffs até 300 linhas capturam 70% a 90% dos defeitos.
  2. *Reversibilidade Cirúrgica:* Permite `git revert <hash>` sem quebrar outras fatias.
  3. *Auditoria Otimizada por IA:* Cabe perfeitamente na janela de contexto de modelos sem degradar a atenção.
- **Exceções Pragmáticas:** Refatorações mecânicas automatizadas ou migrações de dados podem ter diffs maiores; lógicas concorrentes densas devem ter commits menores (50-100 linhas).

---

## 4. Matriz de Permissões para Agentes de IA

| Papel | Leitura Git (`status`, `diff`, `log`) | Commits Locais | `git push` / `git merge` | Resolução de Conflitos |
| :--- | :---: | :---: | :---: | :---: |
| **Subagentes (Workers)** | **SIM** | **NÃO (PROIBIDO)** | **NÃO (PROIBIDO)** | **NÃO** |
| **Orquestrador (Supervisor)** | **SIM** | **SIM (Validados)** | **NÃO (Exige usuário)** | **Apenas Sugere** |
| **Desenvolvedor Humano** | **SIM** | **SIM** | **SIM** | **Decisão Final** |

- **Subagentes (Workers):** Operam em modo estritamente **READ-ONLY** no Git. Bloqueio por permissão no runtime: qualquer comando `git commit`, `git push`, `git merge`, `git pull`, `git checkout` é negado.
- **Orquestrador (Supervisor):**
  - Executa commits locais na branch de trabalho APENAS após passar por todos os checks determinísticos (`cargo check`, `npm run build`, linters, testes).
  - **NUNCA** executa `git push` ou `git merge` sem autorização expressa do usuário.
  - Diante de conflitos de merge/rebase: analisa o diff, formula a recomendação técnica das divergências, mas **NUNCA** resolve autonomamente (flags `-X theirs` ou `-X ours` são proibidas).
