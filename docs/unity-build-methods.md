# Unity Build Method Guide

This guide is for human developers and AI agents that create or update build
targets for HGP.

## Why This Guide Exists

This project executes Unity through a host-native runtime contract.

Each repository must declare the build method that the runtime should execute
for every target.

The runtime executes Unity like this:

```text
unity-editor -batchmode -quit -nographics -buildTarget <target> -executeMethod <build_method>
```

That means `build_method` is required for executable build runs. A build target
definition with only `platform` is not enough.

AI agents should not guess a method name. They must either:

1. reuse a real method already committed in the Unity repository
2. add a new Editor script to the Unity repository and then reference it from
   the build target

## Runtime Contract

When one build run starts, the runtime provides these inputs to the Unity
Editor process:

- `platform` is converted into the Unity `BuildTarget`
- `build_method` is passed through `-executeMethod`
- the runtime computes a canonical output path inside the prepared artifact
  root and exposes that path through both `HGB_OUTPUT_PATH` and the
  `-hgbOutputPath` command-line argument
- `output_path_template` from repository configuration is only a requested
  build path hint; for `archive` outputs it should be a staging path without a
  `.zip` suffix
- `output_kind` is exposed through `HGB_OUTPUT_KIND`
- metadata such as `HGB_BUILD_RUN_ID`, `HGB_RELEASE_RUN_ID`,
  `HGB_BUILD_TARGET_ID`, `HGB_TARGET_PLATFORM`, and `HGB_UNITY_VERSION` are
  also available as environment variables

The build worker succeeds only when the build leaves at least one regular file
inside the artifact root. Producing a directory tree is fine, but that tree
must contain real files.

For operator-facing storage, artifacts are grouped under:

```text
artifacts/<repository-name>.<git-tag>/
```

The final output inside that directory is normalized to:

```text
<repository-name>.<git-tag>.<build-target><ext>
```

The repository-name component is slugged before storage: lowercase, spaces as
`-`, and no accents or special characters. Example:

```text
Meu Repositório -> meu-repositorio
```

So the Unity script must honor the provided output path and must not assume it
controls the final release filename through a hardcoded `Builds/...` value.

## Platform Mapping

These are the common `contract.unity.targetPlatform` values accepted by the
current manifest contract and the recommended method names used in this guide.

| Unity Contract `targetPlatform` | Unity BuildTarget     | Recommended build method |
| ------------------------------- | --------------------- | ------------------------ |
| `StandaloneLinux64`             | `StandaloneLinux64`   | `HGPBuilder.PerformLinux64`   |
| `StandaloneWindows64`           | `StandaloneWindows64` | `HGPBuilder.PerformWindows64` |
| `StandaloneOSX`                 | `StandaloneOSX`       | `HGPBuilder.PerformMacOS`     |
| `WebGL`                         | `WebGL`               | `HGPBuilder.PerformWebGL`     |
| `Android`                       | `Android`             | `HGPBuilder.PerformAndroid`   |

HGP now suggests these method names automatically in the build-target UI based
on the selected `targetPlatform`. This suggestion is only a convention for a
smooth setup flow. The Unity project must still implement the referenced static
methods in its own Editor scripts.

Operators can use the `Override method name` action in the build-target
overlay whenever a project uses a non-standard method path.

## Recommended Unity Script Location

Place the build script inside the Unity project at:

```text
Assets/Editor/HGPBuilder.cs
```

Unity loads static build entrypoints from Editor assemblies, so the methods
must be inside an `Editor` folder or an Editor-only assembly definition.

## Complete Unity Example

The script below is a complete example that can be committed into a Unity
project and referenced by HGP build targets.

It supports:

- Linux
- Windows
- macOS
- WebGL
- Android
- optional `.zip` packaging for directory-style outputs such as WebGL and
  standalone desktop players

```csharp
using System;
using System.Collections.Generic;
using System.IO;
using System.IO.Compression;
using System.Linq;
using UnityEditor;
using UnityEditor.Build.Reporting;

public static class HGPBuilder
{
    private static readonly string[] ArchiveExclusionSuffixes =
    {
        "_DoNotShip",
        "_BackUpThisFolder_ButDontShipItWithYourGame",
    };

    private static readonly string[] WindowsArchiveFileExclusions =
    {
        ".pdb",
    };

    private static readonly string[] MacOSArchivePathExclusions =
    {
        ".dSYM",
    };

    private static readonly string[] WebGLArchiveFileExclusions =
    {
        ".symbols.json",
    };

    public static void PerformLinux64()
    {
        BuildStandalone(BuildTarget.StandaloneLinux64, ".x86_64", "Linux");
    }

    public static void PerformWindows64()
    {
        BuildStandalone(BuildTarget.StandaloneWindows64, ".exe", "Windows");
    }

    public static void PerformMacOS()
    {
        BuildStandalone(BuildTarget.StandaloneOSX, ".app", "macOS");
    }

    public static void PerformWebGL()
    {
        BuildDirectoryTarget(BuildTarget.WebGL, "WebGL");
    }

    public static void PerformAndroid()
    {
        string requestedPath = ResolveOutputPath(
            Path.Combine("Builds", "Android", SafeProductName() + ".apk")
        );

        string extension = Path.GetExtension(requestedPath).ToLowerInvariant();
        if (string.IsNullOrWhiteSpace(extension))
        {
            requestedPath += ".apk";
            extension = ".apk";
        }

        if (extension != ".apk" && extension != ".aab")
        {
            throw new Exception(
                "Android output must end with .apk or .aab: " + requestedPath
            );
        }

        bool previousBuildAppBundle = EditorUserBuildSettings.buildAppBundle;
        EditorUserBuildSettings.buildAppBundle = extension == ".aab";

        try
        {
            RunBuild(BuildTarget.Android, requestedPath);
        }
        finally
        {
            EditorUserBuildSettings.buildAppBundle = previousBuildAppBundle;
        }
    }

    private static void BuildStandalone(
        BuildTarget target,
        string playerExtension,
        string defaultFolderName
    )
    {
        string requestedPath = ResolveOutputPath(
            Path.Combine("Builds", defaultFolderName)
        );

        if (ShouldArchive(requestedPath))
        {
            string tempRoot = CreateTemporaryDirectory(defaultFolderName);
            string playerPath = Path.Combine(
                tempRoot,
                SafeProductName() + playerExtension
            );

            RunBuild(target, playerPath);
            CreateZipFromDirectory(tempRoot, requestedPath, target);
            Directory.Delete(tempRoot, true);
            return;
        }

        string playerOutputPath = NormalizeStandaloneOutputPath(
            requestedPath,
            playerExtension
        );

        RunBuild(target, playerOutputPath);
    }

    private static void BuildDirectoryTarget(BuildTarget target, string defaultFolderName)
    {
        string requestedPath = ResolveOutputPath(
            Path.Combine("Builds", defaultFolderName)
        );

        if (ShouldArchive(requestedPath))
        {
            string tempRoot = CreateTemporaryDirectory(defaultFolderName);
            RunBuild(target, tempRoot);
            CreateZipFromDirectory(tempRoot, requestedPath, target);
            Directory.Delete(tempRoot, true);
            return;
        }

        EnsureDirectoryExists(requestedPath);
        RunBuild(target, requestedPath);
    }

    private static void RunBuild(BuildTarget target, string locationPathName)
    {
        if (string.IsNullOrWhiteSpace(locationPathName))
        {
            throw new Exception("Build output path must not be empty.");
        }

        string directory = Path.GetDirectoryName(locationPathName);
        if (!string.IsNullOrWhiteSpace(directory))
        {
            EnsureDirectoryExists(directory);
        }

        BuildPlayerOptions options = new BuildPlayerOptions
        {
            scenes = GetEnabledScenes(),
            target = target,
            locationPathName = locationPathName,
            options = BuildOptions.None,
        };

        BuildReport report = BuildPipeline.BuildPlayer(options);
        if (report.summary.result != BuildResult.Succeeded)
        {
            throw new Exception(
                "Unity build failed for " + target + ": " + report.summary.result
            );
        }
    }

    private static string[] GetEnabledScenes()
    {
        string[] scenes = EditorBuildSettings.scenes
            .Where(scene => scene.enabled)
            .Select(scene => scene.path)
            .ToArray();

        if (scenes.Length == 0)
        {
            throw new Exception(
                "No enabled scenes were found in Build Settings."
            );
        }

        return scenes;
    }

    private static string ResolveOutputPath(string fallbackPath)
    {
        string fromEnvironment = Environment.GetEnvironmentVariable("HGB_OUTPUT_PATH");
        if (!string.IsNullOrWhiteSpace(fromEnvironment))
        {
            return NormalizePath(fromEnvironment);
        }

        string fromCommandLine = ReadNamedArgument("-hgbOutputPath");
        if (!string.IsNullOrWhiteSpace(fromCommandLine))
        {
            return NormalizePath(fromCommandLine);
        }

        return NormalizePath(fallbackPath);
    }

    private static string ReadNamedArgument(string argumentName)
    {
        string[] arguments = Environment.GetCommandLineArgs();
        for (int index = 0; index < arguments.Length - 1; index++)
        {
            if (arguments[index] == argumentName)
            {
                return arguments[index + 1];
            }
        }

        return null;
    }

    private static string NormalizeStandaloneOutputPath(
        string requestedPath,
        string playerExtension
    )
    {
        if (requestedPath.EndsWith(playerExtension, StringComparison.OrdinalIgnoreCase))
        {
            return requestedPath;
        }

        return Path.Combine(requestedPath, SafeProductName() + playerExtension);
    }

    private static bool ShouldArchive(string path)
    {
        string outputKind = Environment.GetEnvironmentVariable("HGB_OUTPUT_KIND");
        return string.Equals(outputKind, "archive", StringComparison.OrdinalIgnoreCase)
            || path.EndsWith(".zip", StringComparison.OrdinalIgnoreCase);
    }

    private static void CreateZipFromDirectory(
        string sourceDirectory,
        string requestedPath,
        BuildTarget target
    )
    {
        string zipPath = requestedPath.EndsWith(".zip", StringComparison.OrdinalIgnoreCase)
            ? requestedPath
            : requestedPath + ".zip";

        string parentDirectory = Path.GetDirectoryName(zipPath);
        if (!string.IsNullOrWhiteSpace(parentDirectory))
        {
            EnsureDirectoryExists(parentDirectory);
        }

        if (File.Exists(zipPath))
        {
            File.Delete(zipPath);
        }

        using (FileStream zipStream = File.Create(zipPath))
        using (ZipArchive archive = new ZipArchive(zipStream, ZipArchiveMode.Create))
        {
            foreach (string filePath in Directory.GetFiles(
                sourceDirectory,
                "*",
                SearchOption.AllDirectories
            ))
            {
                string relativePath = ToArchiveRelativePath(sourceDirectory, filePath);
                if (ShouldExcludeFromArchive(relativePath, target))
                {
                    continue;
                }

                archive.CreateEntryFromFile(
                    filePath,
                    relativePath,
                    CompressionLevel.Optimal
                );
            }
        }
    }

    private static bool ShouldExcludeFromArchive(
        string relativePath,
        BuildTarget target
    )
    {
        string[] segments = relativePath
            .Split(new[] { '/' }, StringSplitOptions.RemoveEmptyEntries);

        foreach (string segment in segments)
        {
            if (ArchiveExclusionSuffixes.Any(suffix =>
                segment.EndsWith(suffix, StringComparison.OrdinalIgnoreCase)))
            {
                return true;
            }
        }

        string fileName = Path.GetFileName(relativePath);
        if (target == BuildTarget.StandaloneWindows64
            && WindowsArchiveFileExclusions.Any(suffix =>
                fileName.EndsWith(suffix, StringComparison.OrdinalIgnoreCase)))
        {
            return true;
        }

        if (target == BuildTarget.StandaloneOSX
            && segments.Any(segment =>
                MacOSArchivePathExclusions.Any(suffix =>
                    segment.EndsWith(suffix, StringComparison.OrdinalIgnoreCase))))
        {
            return true;
        }

        if (target == BuildTarget.WebGL
            && WebGLArchiveFileExclusions.Any(suffix =>
                fileName.EndsWith(suffix, StringComparison.OrdinalIgnoreCase)))
        {
            return true;
        }

        return false;
    }

    private static string ToArchiveRelativePath(string sourceDirectory, string filePath)
    {
        string fullRoot = Path.GetFullPath(sourceDirectory)
            .TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar);
        string fullPath = Path.GetFullPath(filePath);
        string relativePath = fullPath.Substring(fullRoot.Length)
            .TrimStart(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar);

        return relativePath.Replace('\\', '/');
    }

    private static void EnsureDirectoryExists(string path)
    {
        if (string.IsNullOrWhiteSpace(path))
        {
            return;
        }

        Directory.CreateDirectory(path);
    }

    private static string NormalizePath(string rawPath)
    {
        return Path.GetFullPath(rawPath.Trim());
    }

    private static string CreateTemporaryDirectory(string label)
    {
        string path = Path.Combine(
            "Temp",
            "hgb-" + label + "-" + Guid.NewGuid().ToString("N")
        );
        Directory.CreateDirectory(path);
        return Path.GetFullPath(path);
    }

    private static string SafeProductName()
    {
        char[] invalidCharacters = Path.GetInvalidFileNameChars();
        return new string(
            PlayerSettings.productName
                .Select(character => invalidCharacters.Contains(character) ? '_' : character)
                .ToArray()
        );
    }
}
```

## Archive Filtering Strategy

When the target output is a `.zip`, do not build a handcrafted whitelist of
files. Unity runtime outputs vary by platform, scripting backend, compression,
development flags, and player settings.

The safe policy is:

- keep the full build tree generated by Unity
- exclude only paths that Unity explicitly marks as non-shippable
- exclude platform-specific debug symbol sidecars that Unity documents as
  optional debug outputs instead of runtime dependencies
- keep the exclusion list narrow and name-based instead of platform-wide file
  whitelists

The reference script above excludes path segments that end with:

- `_DoNotShip`
- `_BackUpThisFolder_ButDontShipItWithYourGame`

That covers the common Unity-generated directories such as Burst debug
information folders while avoiding risky guesses about required runtime files.

For archive outputs, HGP should also exclude these debug-only
sidecars when Unity generates them:

- macOS standalone: `*.dSYM` symbol bundles
- Windows standalone: `*.pdb` when Copy PDB Files is enabled
- WebGL: `*.symbols.json` when Debug Symbols is enabled

Platform notes:

- Windows and Linux standalone builds must usually keep the player executable,
  the `*_Data` directory, and any sibling engine/runtime files that Unity emits
- macOS standalone archive outputs must keep the `.app` bundle, but should drop
  sibling `*.dSYM` bundles because those are symbol bundles for debugging and
  crash analysis, not runtime payload
- Windows standalone builds may emit `*.pdb` symbol files for debugging; those
  files are useful for investigation, but they are not required to ship the
  player archive
- WebGL builds must usually keep `index.html`, the `Build/` directory,
  `TemplateData/`, and any additional root files Unity generates
- WebGL may emit `*.symbols.json` for demangled stack traces when debug symbols
  are enabled; treat that file as debug payload, not shipping payload

The runtime archive packager must enforce the same exclusions even if a Unity
repository forgets to update its custom zip routine. Operator-facing build
artifacts must not depend on every project remembering the denylist by hand.

Do not replace this denylist with a short allowlist unless you fully control
every backend and player setting in the repository. That is how people ship a
beautiful zip with half the runtime missing.

## Build Target Manifest Examples

These examples use declarative pipeline manifests because YAML under
`pipelines/` is the supported configuration path.

### Linux

```yaml
spec:
    build:
        targets:
            - name: linux-player
                enabled: true
                buildKind: player
                contract:
                    unity:
                        targetPlatform: StandaloneLinux64
                        buildMethod: HGPBuilder.PerformLinux64
                        editorVersion: 2022.3.14f1
                runner:
                    type: host-native
                    timeoutSeconds: 3600
                output:
                    kind: directory
                    path: Builds/Linux
                config: {}
```

### Windows

```yaml
spec:
    build:
        targets:
            - name: windows-player
                enabled: true
                buildKind: player
                contract:
                    unity:
                        targetPlatform: StandaloneWindows64
                        buildMethod: HGPBuilder.PerformWindows64
                        editorVersion: 2022.3.14f1
                runner:
                    type: host-native
                    timeoutSeconds: 3600
                output:
                    kind: directory
                    path: Builds/Windows
                config: {}
```

### macOS

```yaml
spec:
    build:
        targets:
            - name: macos-player
                enabled: true
                buildKind: player
                contract:
                    unity:
                        targetPlatform: StandaloneOSX
                        buildMethod: HGPBuilder.PerformMacOS
                        editorVersion: 2022.3.14f1
                runner:
                    type: host-native
                    timeoutSeconds: 3600
                output:
                    kind: directory
                    path: Builds/macOS
                config: {}
```

### WebGL

```yaml
spec:
    build:
        targets:
            - name: webgl-player
                enabled: true
                buildKind: player
                contract:
                    unity:
                        targetPlatform: WebGL
                        buildMethod: HGPBuilder.PerformWebGL
                        editorVersion: 2022.3.14f1
                runner:
                    type: host-native
                    timeoutSeconds: 3600
                output:
                    kind: directory
                    path: Builds/WebGL
                config: {}
```

### Android APK

```yaml
spec:
    build:
        targets:
            - name: android-apk
                enabled: true
                buildKind: player
                contract:
                    unity:
                        targetPlatform: Android
                        buildMethod: HGPBuilder.PerformAndroid
                        editorVersion: 2022.3.14f1
                runner:
                    type: host-native
                    timeoutSeconds: 3600
                output:
                    kind: binary
                    path: Builds/Android/Game.apk
                config: {}
```

### WebGL ZIP Archive

```yaml
spec:
    build:
        targets:
            - name: webgl-archive
                enabled: true
                buildKind: player
                contract:
                    unity:
                        targetPlatform: WebGL
                        buildMethod: HGPBuilder.PerformWebGL
                        editorVersion: 2022.3.14f1
                runner:
                    type: host-native
                    timeoutSeconds: 3600
                output:
                    kind: archive
                    path: Builds/WebGL
                config: {}
```

The `HGPBuilder.PerformWebGL` method above automatically produces a zip file when
`output.kind: archive`. HGP configuration should express archive
behavior through `output.kind: archive` and a non-zip requested path.

The reference archive routine also strips Unity-generated `DoNotShip` and
backup folders from the zip while preserving the real runtime files.

The runtime may still rewrite the concrete output filename to its canonical
`<repository>.<tag>.<target>` form before Unity starts. Use the provided path,
do not hardcode the archive name.

## Rules For Developers And AI Agents

- Do not leave `build_method` empty for executable builds
- Do not invent method names without adding the corresponding Unity Editor code
- Keep method names stable once referenced by build targets
- Prefer one small static method per platform instead of one giant method with
  runtime branching across unrelated targets
- Always verify that the build produces real files under the artifact root
- Prefer explicit `output_path_template` values that communicate artifact style
  or extension, not the final host-visible release filename
- Do not end `output_path_template` with `.zip` when `output_kind=archive`

## Common Failure Modes

### `build_method` is empty

The worker fails before Unity starts because the current runtime requires
`build_method` for execution.

### The method exists but writes outside the artifact root

The worker reports missing artifacts because it only registers files found
under the prepared artifact directory.

### The script ignores the provided output path and hardcodes its own filename

The runtime canonicalizes artifact names before execution. If the script writes
to some unrelated hardcoded `Builds/...` path instead of the provided output
path, the worker will not see the expected artifact.

### The method writes a directory but no files

The worker still fails. At least one regular file must exist under the build
artifact root.

### The build target uses `archive` and `output_path_template` ends with `.zip`

The runtime rejects that configuration. `output_path_template` is only the
requested build-method path and must not try to define the final archive name.
Use a staging path such as `Builds/WebGL` instead.

### The build target uses `archive` but the script never creates an archive

Set `output_kind=directory` instead, or use a script like the example above
that zips the build output when `output_kind=archive`.

### The archive contains `DoNotShip` or backup directories

Use a zip routine that filters only Unity-marked non-shippable paths such as
`*_DoNotShip` and `*_BackUpThisFolder_ButDontShipItWithYourGame`. Do not try to
guess a tiny allowlist of platform files unless you are ready to maintain it
for every backend variation.

### The archive contains debug symbol files that are not needed to ship

Strip Unity-generated debug-only sidecars from shipping archives, especially
`*.dSYM` on macOS standalone builds, `*.pdb` on Windows standalone builds, and
`*.symbols.json` on WebGL builds.
Those files belong in crash-analysis workflows, not in the end-user artifact.
