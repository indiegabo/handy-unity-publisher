# Troubleshooting

When something goes wrong, start with the screen that shows the actual run
result. HGP already exposes most of the information operators need to diagnose
the next action.

## Start with the execution report

Open the execution report before you change anything else.

<img src="../../assets/images/prints/process-detail-execution-report.png" alt="Execution report view" />

Use it to answer these questions:

- Did the repository step fail, or did the build itself fail?
- Did the build complete but publication fail afterward?
- Did the output exist, but land somewhere unexpected?

## Common operator issues

### The project cannot reach its repository

Check the project repository settings first, then review credentials in the
settings area if the repository is private.

### A release did not appear

Open the project view and review the recent history and current activity. If no
new work is visible, confirm that the project is configured the way you expect
and that HGP is monitoring the right source.

### A build target failed

Use the execution report and outputs view together. In the current Unity-based
workflow, confirm the selected build target, the chosen editor, and the
resulting output state.

### The artifact exists but was not published

Open the publish destination configuration and confirm that the correct build
target is bound to the expected destination.

### I cannot find the final file on disk

Use the outputs and retained artifact views to determine whether the file is
still in the HGP-managed location or has already moved to the publication
destination.

## When to retry and when to edit the project

Retry the run when the configuration is correct and the failure looks
temporary, such as a short-lived access problem.

Edit the project when the failure points to the project definition itself, such
as the wrong repository address, the wrong build target, or a missing publish
binding.

If you are still unsure, return to [Release Processes](release-processes.md)
and trace the project state from the main feed into the release detail views.
