# task/impruvements/CorrigirImplementar.md

## Explicação

Arquivo responsavel por reunir todas as melhorias e correçoes de forma organizada e os items serão excluidos conforme forem concluido, é um arquivo fixo com conteudo temporario.

---

- Corrigir/Implementar:
  - Geração/Galeria (implementado/validar):
    - [ ] 002: A galeria não atualiza com uma nova foto, forçando o usuario atualizar. Caso de uso Usuario divide a tela em duas abas uma para a galeria e outra na geração ele tem que atualizar a pagina para ver a nova foto e bate no problema 001.
    - [ ] 003: A Geração não salva o estado da pagina, se troca para a galeria tudo que foi modificado se perde, e tambem se atualizar a pagina tambem (se usuario quizer limpar um botão de reset resolve).
    - [ ] 004: Não está reportando por steps o progresso na barra fica "0/4" e só muda quando está prota a imagem e depois da primeira ele fica fixo com o mesmo texto "Geração finalizada com sucesso! 1/1 imagens."
    - [ ] 005: Qual a viabilidade de gravar na imagem os dados de geração dela nos metadados.
    - [ ] 006 Possibilidade de clicar na foto para copiar as configs.
    - [ ] 007: Na galeria não tem como selecionar todas ou como usar shift para selecionar uma porção.
    - [ ] 008: Quando selecionado um modelo que foi salvo pelo treiner(arquivo final do treinamento) ele diz que os argumentos são invalidos, sendo necessario baixar o modelo e efetuar o upload novamente para utilizalo.

  - Infra:
    - [ ] Limitar o CI a apenas a branch main e develop as demais não devem ter CI.
