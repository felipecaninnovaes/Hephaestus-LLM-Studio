# Gestão de Segredos, Credenciais & Segurança Operacional

Políticas de segregação de segredos, proteção de variáveis de ambiente e guardrails de segurança para o Hephaestus LLM Studio.

---

## 1. Política de Segredos e Versionamento

- **PROIBIDO** comitar credenciais, tokens de API (OpenAI, HuggingFace, RunPod), senhas de banco ou certificados privados no Git.
- **Hierarquia de Arquivos de Ambiente:**
  - `.env.example`: Versionado no repositório. Contém **apenas chaves com placeholders fictícios** e comentários documentando as variáveis necessárias.
  - `.env`, `.env.local`, `env.gpu`: Arquivos locais de execução real. **Obrigatoriamente ignorados pelo `.gitignore`**.
- **Detecção Prévia em Hooks:** O hook de `pre-commit` e os linters estáticos realizam varredura ativa para impedir commits com padrões comuns de secrets (chaves privadas, JWTs, strings de conexão com senhas embutidas).

---

## 2. Segregação de Segredos por Camada

| Camada | Escopo de Acesso a Segredos | Restrição Estrita |
| :--- | :--- | :--- |
| **`apps/` (Frontend)** | Apenas variáveis públicas com prefixo `NEXT_PUBLIC_*` (ex: `NEXT_PUBLIC_API_URL=http://localhost:8080`). | **NUNCA** tem acesso a credenciais de banco, segredos JWT ou chaves de provedores de IA. |
| **`services/` (Backend)** | Credenciais de autenticação interna, segredos de assinatura JWT, strings de conexão PostgreSQL e credenciais de bucket S3/SeaweedFS. | Não expõe segredos em respostas HTTP ou logs. |
| **`engines/` (Motores IA)** | Chaves de API de modelos externos (OpenAI VLM, etc.) injetadas estritamente via variáveis de ambiente internas do Docker Compose. | Sem acesso ao host da máquina; apenas rede interna. |

---

## 3. Sanitização de Logs & Telemetria

- **Headers e Payloads Sensíveis:** Headers de autorização (`Authorization: Bearer ...`) e tokens de sessão devem ser mascarados (`Bearer ***`) nos logs estruturados.
- **Erros de Banco de Dados:** Erros internos de infraestrutura ou SQL nunca devem ser refletidos textualmente em respostas HTTP para o cliente web; devem ser logados no servidor com respectivo `request_id` e devolvido um erro genérico estruturado.
