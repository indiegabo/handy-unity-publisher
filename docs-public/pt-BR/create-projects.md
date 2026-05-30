# Criar Projetos

A criação de projetos é onde você diz ao HGP o que observar, o que compilar e
para onde os outputs bem-sucedidos devem ir.

Existem dois tipos de projeto:

##### Projeto de Repositório

Um projeto que aponta para um repositório Git remoto. O HGP baixa o código do
repositório e pode disparar builds automaticamente por polling ou reagindo a
eventos do repositório, como tags de release e commits. Use esse tipo para
fluxos mais próximos de CI e pipelines de release automatizados.

##### Projeto de Workspace Local

Um projeto que aponta para um workspace local no sistema de arquivos. O HGP
roda os builds com os arquivos que já estão disponíveis na máquina do operador,
sem depender de um repositório Git remoto. Isso é útil para desenvolvimento
iterativo, debugging ou projetos que existem só localmente.

## Assistente de criação de projeto

O assistente adapta campos, validações e padrões sugeridos dependendo se você
escolhe um `Repository Project` ou um `Local Workspace Project`. Aqui está o
que muda em cada etapa.

### Etapa 1: Defina a identidade do projeto

Essa é direta: escolha o nome, o tipo de projeto e a engine. No momento, Unity
é a única opção disponível, mas outras engines estão nos planos.

<img src="../../assets/images/prints/create-project-wizard/step-1-identity.png" alt="Etapa de identidade do projeto" style="max-width:350px; width:100%; height:auto;" />

### Etapa 2: Conecte o repositório ou aponte para um workspace

Se você estiver criando um projeto de repositório, é aqui que entram os dados
básicos do repo. Se o repositório for privado, o HGP também vai pedir as
credenciais do GitHub.

<img src="../../assets/images/prints/create-project-wizard/step-2-repository.png" alt="Tela do repositório Git" style="max-width:350px; width:100%; height:auto;" />

<img src="../../assets/images/prints/create-project-wizard/step-2-git-url.png" alt="Exemplo de Git URL" style="max-width:350px; width:100%; height:auto;" />

_Exemplo do que significa "Git URL"_

<img src="../../assets/images/prints/create-project-wizard/step-2-pivate-repo-credentials.png" alt="Credenciais do repositório Git" style="max-width:350px; width:100%; height:auto;" />

Se o seu projeto for local, basta apontar o HGP para a pasta onde ele vive:

<img src="../../assets/images/prints/create-project-wizard/step-2-workspace-path.png" alt="Caminho do workspace local" style="max-width:350px; width:100%; height:auto;" />

### Etapa 3: Adicione targets de build

<img src="../../assets/images/prints/create-project-wizard/step-3-build-targets.png" alt="Visão geral dos targets de build" style="max-width:350px; width:100%; height:auto;" />

Nesta etapa você define quais plataformas devem receber artefatos de build. As
opções disponíveis reagem à engine que você selecionou. Como Unity é a única
engine suportada agora, o que importa é o seguinte:

Você precisa definir um Unity Editor. O assistente tenta detectar instalações
de Unity na sua máquina, mas você também pode apontar uma diretamente pelo
campo "Unity Executable".

Depois disso, adicione um ou mais targets de build que descrevem o que deve
ser gerado para cada plataforma. Cada target inclui a plataforma que você quer
compilar e o nome de um método de build. Veja a seção abaixo, "Como a seleção
de plataforma e o método de build funcionam", para mais detalhes.

O job de build roda dentro do workspace selecionado. Depois que os builds
terminam, o job de publish assume e envia os artefatos gerados para os
destinos configurados, como uma pasta local ou o itch.io.

<img src="../../assets/images/prints/create-project-wizard/step-3-add-build-target.png" alt="Adicionar target de build" style="max-width:350px; width:100%; height:auto;" />

<img src="../../assets/images/prints/create-project-wizard/step-3-add-build-target-sucess.png" alt="Target de build adicionado com sucesso" style="max-width:350px; width:100%; height:auto;" />

#### Como a seleção de plataforma e o método de build funcionam

Escolher uma plataforma diz ao HGP que tipo de build esse target deve
produzir, como um build desktop para Windows ou um pacote Android.

O método de build é o ponto de entrada que o HGP pede para o seu projeto Unity
executar naquele target. O HGP sugere um nome de método padrão para cada
plataforma para que as equipes possam usar uma convenção previsível e, em
muitos casos, esse padrão já basta.

Se o seu projeto usa outro esquema de nomes ou pontos de entrada separados,
você pode editar o nome do método naquele target. Seja qual for o nome que
ficar aqui, ele precisa corresponder a um método de build que já exista no
projeto Unity para aquela plataforma.

Veja [Compilando Projetos Unity](specifics/unity/building-unity-projects.md)
para o contrato específico de Unity e uma classe de exemplo que você pode
adaptar no seu projeto.

Esse exemplo é só um ponto de partida. A lógica de build real continua sendo
responsabilidade do seu projeto, incluindo configurações específicas por
plataforma, etapas extras de build, assinatura, estrutura de saída e qualquer
outro requisito de release.

### Etapa 4: Configure destinos de publicação

<img src="../../assets/images/prints/create-project-wizard/step-4-publish-destinations.png" alt="Visão geral dos destinos de publicação" style="max-width:350px; width:100%; height:auto;" />

Aqui é onde você decide para onde os artefatos de build devem ir. No momento,
você pode adicionar dois tipos de destino:

- um diretório local na sua máquina
- um projeto no itch.io

Nada muito extravagante aqui. A ideia é simples: registrar um destino e depois
mapear cada target de build para a configuração certa de publicação.

Se o destino for um diretório local, você só escolhe uma pasta no seu sistema.
Publicar nesse destino significa soltar os artefatos nessa pasta.

<img src="../../assets/images/prints/create-project-wizard/step-4-publish-destination-add-folder-bindindg.png" alt="Vínculo com diretório" style="max-width:350px; width:100%; height:auto;" />

Se o destino for um projeto no itch.io, publicar significa enviar os
artefatos de cada target de build para o canal correspondente no itch.

<img src="../../assets/images/prints/create-project-wizard/step-4-publish-destination-add-itch-target.png" alt="Vínculo com canal do itch.io" style="max-width:350px; width:100%; height:auto;" />

<img src="../../assets/images/prints/create-project-wizard/step-4-publish-destination-mapped.png" alt="Canal do itch.io mapeado" style="max-width:350px; width:100%; height:auto;" />

### Etapa 5: Caminhos

<img src="../../assets/images/prints/create-project-wizard/step-5-paths.png" alt="Visão geral dos caminhos" style="max-width:350px; width:100%; height:auto;" />

Essa etapa agora gira principalmente em torno do workspace root.

O HGP sugere um caminho de workspace padrão dentro da pasta de usuário do seu
sistema operacional, em `HGPWorkspaces/<project-name>`. No Windows, por
exemplo, isso normalmente fica como `C:/Users/<user>/HGPWorkspaces/<project-name>`.

Se esse padrão funcionar para você, mantenha. Se não funcionar, você pode
sobrescrevê-lo para esse projeto.
