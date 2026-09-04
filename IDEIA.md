Seguindo o mesmo estilo de design do HTML que envie mas apenas organizando e separando melhor de acordo com a estrutura de proposta do sistema, não reinvente o design apenas separe e organize. 

## 1. Camadas e linguagens (responsabilidade de cada uma)

**Frontend — Next.js / TypeScript**
- Interface do studio, com abas separadas por tipo de modelo/tarefa
- Visualização e upload de imagens
- Telas de AutoLabel e AutoTracker
- Visualização Dataset separado em lista ou grade. O usuario tem que clicar no dataset para abrir a galeria e acessar as funções de AutoLabel e AutoTracker. Suporte a Exportar o dataset e importar(uma forma de backup estruturado)

**Backend principal — Rust**
- API central
- Gestão de arquivos (upload/download)
- Geração e gestão dos JSON/YAML de configuração de treino
- Ponto único de comunicação entre a UI e o restante do sistema
- Connecta com o orquestador via API

**Backend orquestrador — Rust**
- Recebe o que o backend principal estruturou
- Decide/gerencia onde o treino vai rodar (local ou remoto)
- Orquestra a comunicação com o motor de treino

**Motor de treino — Python**
- Execução do treino em si (PyTorch e outros engines)
- Roda tanto localmente quanto em VPS/RunPod


## 2. Módulos de treinamento (abas por tipo de modelo)

| Categoria | Modelos citados | Ferramenta de preparo de dados |
|---|---|---|
| Difusão | Flux, SDXL, SD1.5, etc. | AutoLabel(Modelo local ou usando o formato Openai API) |
| Descrição/embedding de imagem | OpenCLIP | AutoLabel(Modelo local ou usando o formato Openai API) |
| Detecção/tracking | YOLO | AutoTracker(Video e Imagem) |

Cada categoria tem sua própria aba de treino, já que os parâmetros e fluxos de trabalho são diferentes entre elas.
Tanto o AutoLabel quanto o AutoTracker tera suporte a upload de modelo pela interface caso o usuario tenha um modelo personalizado.

## 3. Ferramentas de preparo de dados

- **AutoLabel** → gera descrições/labels de imagens, usado para os modelos de difusão e para o OpenCLIP
- **AutoTracker** → gera bounding boxes automaticamente, usado para o YOLO
- Ficam em abas separadas porque exigem tecnologia e visualização próprias
- Ambas podem usar:
  - um modelo **local** (na GPU do usuário), ou
  - um modelo enviado por **upload** para rodar em outra GPU (VPS/RunPod)

## 4. Infraestrutura de execução

- **Docker** como base de toda a arquitetura (cada componente containerizado)
- Duas opções de onde treinar:
  - **Local**: usa a GPU do próprio usuário
  - **Remoto**: sobe um serviço em VPS ou RunPod com GPUs mais potentes
- O backend principal, que roda localmente, estrutura os dados e configs e repassa ao orquestrador, que decide/gerencia a execução — local ou remota

---
