# Processos de Release

Um processo de release é uma execução completa do HGP para um projeto. Ele
começa a partir de uma origem de entrada, passa pelo build e pela publicação e
deixa histórico suficiente para você entender exatamente o que aconteceu.

## Como os processos de release começam

Existem duas formas principais de um processo de release começar.

### Polling

Para projetos de repositório, o HGP pode observar o repositório em um
intervalo definido.

Quando o polling está habilitado, o HGP verifica tags ainda não vistas e as
enfileira da mais antiga para a mais recente. O HGP roda apenas um processo de
build de release por repositório de cada vez, então o polling daquele
repositório pausa enquanto a release atual ainda tiver builds enfileirados ou
em execução.

Esse é o fluxo sem intervenção: você envia ou cria a tag no Git, o HGP a
descobre e o processo de release começa sozinho.

### Start release

Você também pode disparar uma release manualmente pela ação `Start release` na
UI desktop.

Isso é útil quando você quer rodar uma release na hora, testar uma mudança sem
esperar o próximo intervalo de polling ou trabalhar com um projeto de workspace
local que não depende de polling de repositório.

Se a release começou por polling ou por `Start release`, o resto do processo
segue o mesmo pipeline de runtime.

## Como o processo continua se atualizando

Depois que a release existe, o HGP continua atualizando o processo enquanto o
trabalho avança pelos estágios.

Isso significa que a UI não mostra apenas se algo passou ou falhou no fim.
Enquanto a execução está ativa, o HGP continua exibindo mudanças de status
como:

- `Queued`
- `Running`
- `Succeeded`
- `Failed`

Essas atualizações aparecem no main feed, em visões intermediárias e nas
superfícies de detalhe do processo. Conforme a preparação do repositório, os
builds e a publicação avançam, o estado do processo avança junto.

## Como a visão principal mostra processos ativos

O main feed é o primeiro lugar para acompanhar trabalho ativo e atividade
recente de release.

<img src="../../assets/images/prints/main-running-polled-proccess.png" alt="Feed principal com atividade em execução" style="max-width:350px; width:100%; height:auto;" />

Use essa tela para responder três perguntas rápidas:

- qual processo acabou de começar ou mudar?
- ele está enfileirado, rodando ou já terminou?
- eu preciso abrir esse processo agora?

Pense no main feed como um quadro de despacho. É nele que você encontra
processos ativos rapidamente e pula para o que precisa de atenção.

## Use a página do processo

Quando você abre um processo específico de release, a página do processo vira
o lugar principal para acompanhar aquela execução.

<img src="../../assets/images/prints/process-detail-1.png" alt="Visão geral do detalhe do processo" style="max-width:350px; width:100%; height:auto;" />

É aqui que você sai do "algo está acontecendo" para "consigo ver em que etapa
esse processo está, o que já terminou e o que ainda precisa de atenção".

Enquanto o processo está ativo, essa página reflete o estado atual do runtime.
Depois que ele termina, ela vira a principal superfície de auditoria para tudo
o que o HGP manteve daquela execução.

## Como inspecionar tudo depois que um processo termina

Quando uma release termina, a página do processo vira sua fonte de verdade.

Comece pelo execution report.

<img src="../../assets/images/prints/process-detail-execution-report.png" alt="Visão do execution report" style="max-width:350px; width:100%; height:auto;" />

O execution report ajuda a reconstruir o que aconteceu durante a execução,
incluindo onde o processo falhou, quais etapas concluíram e o que aconteceu
antes do estado final ser alcançado.

Depois disso, vá para os outputs e para o material retido.

<img src="../../assets/images/prints/process-detail-outputs.png" alt="Outputs do detalhe do processo" style="max-width:350px; width:100%; height:auto;" />

<img src="../../assets/images/prints/process-detail-retained.png" alt="Visão de artefatos retidos" style="max-width:350px; width:100%; height:auto;" />

Para um processo concluído, o HGP pode manter material suficiente para você
avaliar a execução depois. Dependendo do que foi preservado naquele processo,
você pode inspecionar:

- o execution report
- logs retidos
- outputs registrados e localizações de artefatos
- artefatos retidos que ainda existem no armazenamento gerenciado pelo HGP

Isso significa que um processo finalizado não é só um badge final. É algo que
você pode reabrir e auditar depois, usando os logs e artefatos que permaneceram
disponíveis para aquela execução.

## Logs e artefatos disponíveis após a conclusão

Quando o HGP reteve material de execução de um processo concluído, você pode
revisar mais do que apenas o status final.

Os logs continuam importantes aqui. Em um processo finalizado, você pode usar
as superfícies de log disponíveis para entender o que Unity, a preparação do
repositório ou as etapas de publicação realmente fizeram enquanto o processo
estava rodando.

Os artefatos continuam importantes também. A visão de outputs mostra os
artefatos que o HGP registrou para aquele processo, e a visão de artefatos
retidos ajuda a confirmar o que ainda existe no armazenamento gerenciado pelo
HGP depois que a execução alcançou um estado terminal.

No fim de um processo, estas são as perguntas práticas que você deveria
conseguir responder:

- onde o processo falhou, se ele falhou?
- quais logs ainda estão disponíveis para inspeção?
- quais artefatos foram registrados para essa execução?
- quais artefatos foram publicados para fora e quais permaneceram no armazenamento gerenciado pelo HGP?
- eu tenho material retido suficiente para repetir com confiança ou depurar a próxima execução?

## O que acontece no Workspace Root

O `Workspace root` é a área específica do projeto que o HGP usa para o trabalho
gerenciado em runtime.

Para projetos de repositório, o HGP usa esse caminho para manter o checkout
gerenciado e os diretórios de trabalho de release e build. Um layout
simplificado fica assim:

```text
<workspace-root>/
    runs/
        release-run-<release-id>/
            source/
            builds/
                build-run-<build-id>[-attempt-token]/
                    logs/
                        unity-build.log
                    outputs/
```

Na prática, isso significa:

- `source/` guarda o checkout gerenciado para releases baseadas em repositório
- `builds/` contém uma área de trabalho por execução de build
- `logs/` guarda o log de build daquela execução
- `outputs/` é onde o HGP espera encontrar os artefatos de build antes que a
  etapa de publicação os mova para outro lugar

Para projetos de workspace local, o HGP não clona o código para essa pasta
`source/`. Ele usa o workspace local já existente como origem, mas ainda assim
mantém logs e outputs específicos da execução dentro do Workspace Root.

Então o Workspace Root não é só uma pasta de cache qualquer. Ele é a área de
trabalho de runtime daquele projeto: checkouts quando necessário, pastas de
build por execução, logs e outputs gerenciados.

## Entenda o status do processo em termos operacionais

Os status mais importantes são:

- **Queued**: o HGP aceitou o trabalho, mas ainda não começou a executá-lo.
- **Running**: o HGP está processando a release ou o target neste momento.
- **Succeeded**: o build produziu o output esperado.
- **Failed**: o operador precisa revisar o execution report ou os outputs.

Esses rótulos ficam mais úteis quando você os combina com o detalhe do
processo e com o execution report. É aí que um status simples vira uma próxima
ação clara.
