# Release Processes

A release process is one full HGP run for a project. It starts from one input
source, moves through build and publish work, and leaves behind enough history
for you to understand exactly what happened.

## How release processes start

There are two main ways a release process can begin.

### Polling

For repository projects, HGP can watch the repository on a schedule.

When polling is enabled, HGP checks for unseen tags and queues them from the
oldest unseen tag to the newest one. HGP only runs one release build process
per repository at a time, so polling for that repository pauses while the
current release still has queued or running build work.

This is the hands-off flow: push or create the tag in Git, let HGP discover
it, and the release process starts on its own.

### Start release

You can also trigger a release manually through the `Start release` action in
the desktop UI.

This is useful when you want to run a release right away, test a change
without waiting for the next polling interval, or work with a local workspace
project that does not depend on repository polling.

Whether the release started through polling or through `Start release`, the
rest of the process follows the same runtime pipeline.

## How the process keeps updating

Once a release exists, HGP keeps updating it while work moves through its
stages.

That means the UI does not just tell you whether something eventually passed or
failed. While the run is active, HGP keeps surfacing status changes such as:

- `Queued`
- `Running`
- `Succeeded`
- `Failed`

Those updates appear across the main feed, project view, workers view, and the
process detail surfaces. As repository preparation, builds, and publication
advance, the process state keeps moving with them.

## How the main view shows active processes

The main feed is your first stop for active work and recent release activity.

<img src="../../assets/images/prints/main-running-polled-proccess.png" alt="Main feed with running activity" style="max-width:350px; width:100%; height:auto;" />

Use it to answer three quick questions:

- Which process just started or changed?
- Is it queued, running, or already finished?
- Do I need to open that process now?

Think of the main feed as the dispatch board. It is where you spot active
processes quickly and jump into the one that needs attention.

## Use the process page

Once you open a specific release process, the process page becomes the main
place to follow that run.

<img src="../../assets/images/prints/process-detail-1.png" alt="Process detail overview" style="max-width:350px; width:100%; height:auto;" />

This is where you move from "something is happening" to "I can see what stage
this process is in, what already finished, and what still needs attention."

While the process is active, this page reflects the current runtime state.
After the process finishes, it becomes the main audit surface for everything
HGP kept about that run.

## How to inspect everything after a process finishes

When a release ends, the process page becomes your source of truth.

Start with the execution report.

<img src="../../assets/images/prints/process-detail-execution-report.png" alt="Execution report view" style="max-width:350px; width:100%; height:auto;" />

The execution report helps you reconstruct what happened during the run,
including where the process failed, which stages completed, and what happened
before the final state was reached.

After that, move into outputs and retained material.

<img src="../../assets/images/prints/process-detail-outputs.png" alt="Process detail outputs" style="max-width:350px; width:100%; height:auto;" />

<img src="../../assets/images/prints/process-detail-retained.png" alt="Retained artifacts view" style="max-width:350px; width:100%; height:auto;" />

For a completed process, HGP can keep enough retained material for you to
evaluate the run afterward. Depending on what was preserved for that process,
you can inspect:

- the execution report
- retained logs
- registered outputs and artifact locations
- retained artifacts that still exist in HGP-managed storage

That means a finished process is not just a final badge. It is something you
can reopen and audit later, using the logs and artifacts that remained
available for that run.

## Logs and artifacts available after completion

When HGP retained execution material for a completed process, you can review
more than just the final status.

Logs stay important here. For a finished process, you can use the available log
surfaces to understand what Unity, repository preparation, or publish steps
actually did while the process was running.

Artifacts stay important too. The outputs view shows the artifacts HGP
recorded for that process, and the retained artifact view helps you confirm
what still exists in HGP-managed storage after the run reached a terminal
state.

At the end of a process, these are the practical questions you should be able
to answer:

- Where did the process fail, if it failed at all?
- Which logs are still available for inspection?
- Which artifacts were registered for this run?
- Which artifacts were published away, and which ones stayed in HGP-managed storage?
- Do I have enough retained material to retry confidently or debug the next run?

## What happens in the Workspace Root

The `Workspace root` is the project-specific area HGP uses for managed runtime
work.

For repository projects, HGP uses it to keep the managed checkout and the
release/build working directories. A simplified layout looks like this:

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

What that means in practice:

- `source/` holds the managed checkout for repository-backed releases
- `builds/` contains one working area per build run
- `logs/` stores the build log for that run
- `outputs/` is where HGP expects the build artifacts to appear before any
  publish step moves them elsewhere

For local workspace projects, HGP does not clone the source into that `source/`
folder. It uses the existing local workspace as the source, but it still keeps
run-specific logs and outputs under the Workspace Root.

So the Workspace Root is not just a random cache folder. It is the runtime work
area for that project: checkouts when needed, per-run build folders, logs, and
managed outputs.

## Understand process status in operator terms

The most important statuses are:

- **Queued**: HGP has accepted the work but has not started it yet.
- **Running**: HGP is currently processing the release or target.
- **Succeeded**: The build produced the expected output.
- **Failed**: The operator needs to review the execution report or outputs.

Those labels matter most when you combine them with the process detail and the
execution report. That is where a plain status turns into a clear next action.
