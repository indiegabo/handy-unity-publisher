# O que é o Handy Games Publisher

HGP é uma ferramenta desktop de operações de release para projetos de jogos.
Ela reúne em um só lugar o cadastro de projetos, o disparo de processos de
release, a execução de builds locais, a revisão de outputs e a publicação de
artefatos.

Hoje, o fluxo automatizado com suporte completo cobre projetos Unity, mas o
produto em si é sobre rodar pipelines de release de jogos a partir da sua
própria workstation, com repetibilidade, logs e controle operacional.

<img src="../../assets/images/prints/main.png" alt="O feed principal do HGP" />

_A tela principal exibindo um processo em andamento_

## O fluxo: simples e poderoso

##### Configurar seu pipeline automatizado leva só três passos:

- Mapeie seu projeto: crie um mapeamento que aponte para um workspace local
  ou para um repositório Git.

- Configure com segurança e defina os targets: forneça suas credenciais
  (armazenadas com segurança usando o cofre do seu sistema operacional ou
  estratégias específicas de Git), mapeie os targets de build (Windows,
  Linux, Android, Web etc.) e defina o destino de publicação.

- Dispare a release: você pode iniciar um processo manualmente ou, se estiver
  usando um repositório Git, configurar polling. Isso encaixa bem em um fluxo
  de publicação de verdade: basta criar uma tag de release no repositório e o
  HGP gera todos os builds necessários e envia cada um para o lugar certo.

Agora bora entender como criar [projetos](create-projects.md).