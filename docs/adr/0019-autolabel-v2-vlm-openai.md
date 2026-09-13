# ADR-0019: AutoLabel v2 — Modelos VLM (Florence-2 / Qwen2-VL) e Suporte à API OpenAI

- **Status:** Aceito
- **Data:** 2026-09-12
- **Autor:** Coordenador Hephaestus
- **Fatia:** AutoLabel v2 (Vision-Language Models & OpenAI-Compatible API)
- **Contrato OpenAPI:** `0.18.0`

---

## 1. Contexto & Motivação

O **AutoLabel v1** (introduzido na ADR-0016) estabeleceu o pipeline de geração de legendas em lote para datasets de imagens através do subcomando `autolabel`, gravando `captions.jsonl` e integrando com o endpoint `POST /api/jobs/:id/autolabel/apply`. No entanto, o v1 operava estritamente sob o modelo `"mock"`.

Com a introdução da **Forja de Difusão LoRA** (ADR-0018), a qualidade das legendas tornou-se um fator crítico para a convergência e fidelidade dos adaptadores treinados. Para atender à demanda de legendagem profissional em escala, o AutoLabel v2 expande os recursos para:
1. **Modelos Vision-Language Locais / Open-Weights**:
   - `florence-2` (Microsoft): dense captioning ultra-rápido e detalhado.
   - `qwen2-vl` (Alibaba Cloud): raciocínio visual avançado e forte aderência a prompts textuais complexos.
2. **Provedores de API Vision Compatíveis com OpenAI**:
   - `openai`: integração via HTTP REST compatível com OpenAI Chat Completions Vision (`gpt-4o`, `gpt-4o-mini`, ou instâncias locais/remotas como Ollama `http://localhost:11434/v1` e vLLM).

---

## 2. Decisões Arquiteturais

### D0: Especificação do Contrato OpenAPI (`0.18.0`)

O schema `AutolabelJobRequest` no `packages/contracts/openapi.yaml` é expandido de forma retrocompatível:
```yaml
AutolabelJobRequest:
  type: object
  required:
    - datasetId
  properties:
    datasetId:
      type: string
      format: uuid
    model:
      type: string
      default: "mock"
      enum: ["mock", "florence-2", "qwen2-vl", "openai"]
      description: "Modelo VLM local ou provedor de API para legendagem"
    prompt:
      type: string
      maxLength: 8000
      description: "Instrução ou prompt direcionador repassado ao VLM"
    apiKey:
      type: string
      maxLength: 512
      description: "Chave de autenticação (opcional se fornecida no ambiente)"
    apiBase:
      type: string
      maxLength: 512
      default: "https://api.openai.com/v1"
      description: "URL base para endpoint compatível com OpenAI"
    openaiModel:
      type: string
      maxLength: 128
      default: "gpt-4o-mini"
      description: "Nome do modelo remoto a invocar no endpoint OpenAI"
    orchestratorId:
      type: string
      format: uuid
      nullable: true
```

### D1: Validação e Geração de `config.yaml` no Backend API Principal

Em `services/api-principal/src/jobs/models.rs`:
- `ALLOWED_AUTOLABEL_MODELS = &["mock", "florence-2", "qwen2-vl", "openai"]`.
- Validação: `apiBase` deve iniciar com `http://` ou `https://` se informado; limites de tamanho aplicados.
- Geração de `config.yaml`: a seção `autolabel` serializa de forma limpa `prompt`, `api_key`, `api_base` e `openai_model`.

### D2: Execução no Engine Python (`engines/trainer-yolo/src/trainer_yolo/autolabel.py`)

- **Modo `openai`**:
  - Lê `api_key` da configuração ou do ambiente `OPENAI_API_KEY`.
  - Processa cada imagem do pacote de dataset codificando-a em base64.
  - Dispara requisição HTTP POST para `{api_base}/chat/completions` com payload padrão OpenAI multimodal:
    `messages: [{"role": "user", "content": [{"type": "text", "text": prompt}, {"type": "image_url", "image_url": {"url": "data:image/webp;base64,..."}}]}]`.
  - Extrai a resposta `choices[0].message.content` como a legenda da imagem.
  - Emite `captions.jsonl` contendo `{"filename": "...", "caption": "..."}`.
  - Suporta fallback determinístico quando executado em ambiente de teste sem rede/chaves.
- **Modos `florence-2` e `qwen2-vl`**:
  - No modo real (GPU disponível e pesos presentes), executa inferência direta.
  - Sob `ENGINE_MOCK=1` (testes/CI), gera legendas contextuais sintéticas avançadas no estilo de cada arquitetura, mantendo determinismo por seed e filename.

### D3: Formato de Saída e Aplicação de Legendas (100% Retrocompatível)

O orquestrador continua coletando os mesmos artefatos canônicos:
- `captions.jsonl` (`kind='captions'`)
- `metrics.jsonl` (`kind='metrics'`)

O endpoint existente `POST /api/jobs/:id/autolabel/apply` permanece idêntico, permitindo aplicar as legendas geradas ao dataset com ou sem sobrescrita de legendas manuais.

### D4: Interface Web Vidro Óptico (`AutoLabelModal.tsx`)

- Modal com design responsivo Vidro Óptico:
  - Seletor de Modelo com cards/pills para:
    1. **Florence-2** (Microsoft) — Rápido e denso.
    2. **Qwen2-VL** (Alibaba) — Alto raciocínio e fidelidade.
    3. **OpenAI / Compatível** (API Externa) — GPT-4o, Ollama local, vLLM.
    4. **Mock** (Local Rápido) — Testes e CI sem GPU.
  - Quando **OpenAI** for selecionado:
    - Campo de Chave de API (`apiKey`) com toggle de visualização.
    - Campo de URL do Endpoint (`apiBase`), pré-preenchido com `https://api.openai.com/v1`, mas permitindo endpoints locais como `http://localhost:11434/v1`.
    - Campo de Modelo (`openaiModel`), com opções rápidas (`gpt-4o-mini`, `gpt-4o`, custom).
  - Presets de Prompt prontos para uso ("Difusão LoRA", "Inspeção de Detalhes", "Tags / Booru") e campo aberto para customização.
  - Seletor de Nó de Execução `<NodeSelect />`.

### D5: Segurança e Tratamento de Segredos

A chave `apiKey` opcional trafega via canal protegido SSL/TLS até a API Principal, é encapsulada na configuração efêmera do job no storage isolado do workspace do container, e nunca é logada nem exposta nas respostas públicas de consulta a jobs.
