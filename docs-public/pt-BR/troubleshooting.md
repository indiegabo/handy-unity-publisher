# Solução de Problemas

Quando algo dá errado, comece pela tela que mostra o resultado real da
execução. O HGP já expõe a maior parte das informações que o operador precisa
para decidir o próximo passo.

## Comece pelo execution report

Abra o execution report antes de mudar qualquer outra coisa.

<img src="../../assets/images/prints/process-detail-execution-report.png" alt="Visão do execution report" />

Use-o para responder a estas perguntas:

- a etapa de repositório falhou ou o build em si falhou?
- o build terminou, mas a publicação falhou depois?
- o output existia, mas foi parar em um lugar inesperado?

## Problemas comuns do operador

### O projeto não consegue acessar o repositório

Verifique primeiro as configurações de repositório do projeto e depois revise
as credenciais na área de configurações se o repositório for privado.

### Uma release não apareceu

Abra a visão do projeto e revise o histórico recente e a atividade atual. Se
nenhum trabalho novo estiver visível, confirme se o projeto está configurado do
jeito esperado e se o HGP está monitorando a origem correta.

### Um target de build falhou

Use o execution report e a visão de outputs juntos. No fluxo atual baseado em
Unity, confirme o target selecionado, o editor escolhido e o estado final do
output.

### O artefato existe, mas não foi publicado

Abra a configuração de destino de publicação e confirme se o target de build
certo está vinculado ao destino esperado.

### Não consigo encontrar o arquivo final no disco

Use as visões de outputs e de retained artifacts para determinar se o arquivo
ainda está na localização gerenciada pelo HGP ou se já foi movido para o
destino de publicação.

## Quando repetir a execução e quando editar o projeto

Repita a execução quando a configuração estiver correta e a falha parecer
temporária, como um problema curto de acesso.

Edite o projeto quando a falha apontar para a própria definição do projeto,
como endereço de repositório errado, target de build errado ou binding de
publicação ausente.

Se ainda estiver em dúvida, volte para [Processos de Release](release-processes.md)
e siga o estado do projeto desde o feed principal até as telas de detalhe da
release.