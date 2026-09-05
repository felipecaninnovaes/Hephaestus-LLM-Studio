# Declaração de Intenção — Hephaestus Studio (MVP)

- **Outcome:** Um estúdio local de visão computacional capaz de gerenciar e despachar tarefas para múltiplos nós de execução (GPU local e múltiplos pods remotos RunPod/VPS), centrado no ciclo completo de **YOLO**: ingestão de dados brutos, pré-filtragem e auto-anotação com detectores, refinamento visual de BBoxes e execução de treinos com retorno automático dos artefatos.
- **User:** Você (para projetos diários de visão computacional e detecção), com o objetivo futuro de torná-lo um projeto open-source público.
- **Why now:** A triagem e anotação manual de grandes volumes de imagens frame a frame é um gargalo lento que pré-filtragem por IA resolve; somado ao atrito constante de configurar UIs pesadas, portas e transferências manuais em instâncias remotas descartáveis.
- **Success:** Adicionar imagens/vídeos brutos no estúdio local, pré-filtrar em lote (descarte + pré-caixas), revisar BBoxes no editor e despachar o treino de YOLO para qualquer nó conectado (local ou pods em paralelo), recebendo o modelo (`.pt`) direto na máquina local.
- **Constraint:** Desacoplamento estrito entre o mestre local (dados, banco, interface) e os nós executores (Rust + containers Python efêmeros), garantindo que os nós de computação não retenham dados essenciais ao serem destruídos.
- **Out of scope (MVP):** Treino de Difusão (LoRA), OpenCLIP, LLMs textuais, integração com ComfyUI e treinamento distribuído DDP (um único treino particionado em várias GPUs).
