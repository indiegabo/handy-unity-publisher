# Create Projects

Project creation is where you tell HGP what to watch, what to build, and where
successful outputs should end up.

There are two types of projects:

##### Repository Project

A project that points to a remote Git repository. HGP pulls
source code from that repository and can trigger builds automatically by
polling or by reacting to repository events such as release tags and
commits. Use this type for CI-like workflows and automated release
pipelines.

##### Local Workspace Project

A project that points to a local filesystem workspace. HGP runs
builds against the files already available on the operator's machine without
requiring a remote Git repository. This is useful for iterative
development, debugging, or when working with local-only projects.

## Project Creation Wizard

The creation wizard adapts fields, validation, and suggested defaults depending
on whether you choose a `Repository Project` or a `Local Workspace Project`.
Here is what changes in each step.

### Step 1: Define the project identity

This one is straightforward: pick a name, the project type, and the engine.
Right now Unity is the only available option, but more engines are planned.

<img src="../../assets/images/prints/create-project-wizard/step-1-identity.png" alt="Project identity step" style="max-width:350px; width:100%; height:auto;" />

### Step 2: Connect the repository or point to a workspace

If you are creating a repository project, this is where you enter the basic
details for your repo. If the repository is private, HGP will also ask for
GitHub credentials.

<img src="../../assets/images/prints/create-project-wizard/step-2-repository.png" alt="Git repository screen" style="max-width:350px; width:100%; height:auto;" />

<img src="../../assets/images/prints/create-project-wizard/step-2-git-url.png" alt="Git URL example" style="max-width:350px; width:100%; height:auto;" />

_Example of what "Git URL" means_

<img src="../../assets/images/prints/create-project-wizard/step-2-pivate-repo-credentials.png" alt="Git repository credentials" style="max-width:350px; width:100%; height:auto;" />

If your project is local, just point HGP to the folder where it lives:

<img src="../../assets/images/prints/create-project-wizard/step-2-workspace-path.png" alt="Local workspace path" style="max-width:350px; width:100%; height:auto;" />

### Step 3: Add build targets

<img src="../../assets/images/prints/create-project-wizard/step-3-build-targets.png" alt="Build target overview" style="max-width:350px; width:100%; height:auto;" />

In this step you define which platforms should receive build artifacts. The
available options react to the engine you selected. Since Unity is the only
engine supported right now, here is what matters:

You need to define a Unity Editor. The wizard tries to detect Unity installs on
your machine, but you can also point to one directly through the
"Unity Executable" field.

Then you add one or more build targets that describe what should be generated
for each platform. Each target includes the platform you want to build for and
a build method name. See
[How platform selection and build method work](#how-platform-selection-and-build-method-work)
for more details.

The build job runs inside the selected workspace. After the builds finish, the
publish job takes over and sends the generated artifacts to their configured
destinations, such as a local folder or itch.io.

<img src="../../assets/images/prints/create-project-wizard/step-3-add-build-target.png" alt="Build target overview" style="max-width:350px; width:100%; height:auto;" />

<img src="../../assets/images/prints/create-project-wizard/step-3-add-build-target-sucess.png" alt="Build target overview" style="max-width:350px; width:100%; height:auto;" />

#### How platform selection and build method work

Choosing a platform tells HGP what kind of build this target should produce,
such as a Windows desktop build or an Android package.

The build method is the entry point HGP asks your Unity project to run for that
target. HGP suggests a standard method name for each platform so teams can use
a predictable convention, and in many cases that default is enough.

If your project uses a different naming scheme or separate build entry points,
you can edit the method name for that target. Whatever name you keep here must
match a build method that already exists in the Unity project for that
platform.

See [Building Unity Projects](specifics/unity/building-unity-projects.md)
for the Unity-specific contract and a sample class you can adapt in your
project.

That example is just a starting point. You are still responsible for the real
build logic in your project, including platform-specific settings, extra build
steps, signing, output structure, and any other release requirements.

### Step 4: Configure publish destinations

<img src="../../assets/images/prints/create-project-wizard/step-4-publish-destinations.png" alt="Publish destinations overview" style="max-width:350px; width:100%; height:auto;" />

This is where you decide where build artifacts should go. Right now you can add
two destination types:

- A local directory on your machine
- An itch.io project

Nothing too fancy here. The idea is simple: register a destination, then map
each build target to the right destination settings.

If the destination is a local directory, you just choose a folder on your
system. Publishing to that destination means dropping the artifacts into that
folder.

<img src="../../assets/images/prints/create-project-wizard/step-4-publish-destination-add-folder-bindindg.png" alt="Directory Binding" style="max-width:350px; width:100%; height:auto;" />

If the destination is an itch.io project, publishing means sending the
artifacts for each build target to the matching itch channel.

<img src="../../assets/images/prints/create-project-wizard/step-4-publish-destination-add-itch-target.png" alt="Itch Channel Binding" style="max-width:350px; width:100%; height:auto;" />

<img src="../../assets/images/prints/create-project-wizard/step-4-publish-destination-mapped.png" alt="Itch Channel Binding" style="max-width:350px; width:100%; height:auto;" />

### Step 5: Paths

<img src="../../assets/images/prints/create-project-wizard/step-5-paths.png" alt="Paths overview" style="max-width:350px; width:100%; height:auto;" />

This step is now mostly about the workspace root.

HGP suggests a default workspace path inside your operating system user folder,
under `HGPWorkspaces/<project-name>`. On Windows, for example, that usually
looks like `C:/Users/<user>/HGPWorkspaces/<project-name>`.

If that default works for you, keep it. If not, you can override it for this
project.
