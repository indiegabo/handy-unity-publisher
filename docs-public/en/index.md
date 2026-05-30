# What is Handy Games Publisher

HGP is a desktop release operations tool for game projects. It gives you
one place to register projects, trigger release processes, run local builds,
review outputs, and publish artifacts.

Today, the fully supported automated workflow covers Unity projects, but the
product itself is about running game release pipelines from your own
workstation with repeatability, logs, and operator control.

<img src="../../assets/images/prints/main.png" alt="The HGP main feed" />

_The main view displaying a ongoing process_

## The Workflow: Simple & Powerful

##### Setting up your automated pipeline takes only three steps:

- Map Your Project: Create a project mapping that points to either a local workspace or a Git repository.

- Configure Safely & Set Targets: Provide your credentials (stored securely using your OS vaulting technologies or Git-specific strategies), map your build targets (Windows, Linux, Android, Web, etc.), and define your publishing destination.

- Trigger the Magic: You can trigger a publishing process manually, or—if you’re using a Git repo—set up a polling system. This is perfect for a true publishing workflow: just create a release tag in your Git repo, and HGP will automatically generate all target builds and send them where they need to go.

Well... time to understand how to create [Create Projects](create-projects.md).
