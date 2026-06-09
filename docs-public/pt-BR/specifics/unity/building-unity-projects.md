# Compilando Projetos Unity

Esta página explica a parte específica de Unity no fluxo de build do HGP: como
o HGP inicia o Unity Editor, o que ele passa para o processo e o que o seu
projeto Unity precisa expor para que os targets de build rodem com sucesso.

## Como o HGP inicia o Unity

Quando o HGP executa um target de build de Unity, ele inicia o editor em batch
mode e chama o método de build configurado para aquele target.

```text
unity-editor -batchmode -quit -nographics -buildTarget <target> -executeMethod <build_method> -hgbOutputPath <output_path>
```

Isso significa que o seu projeto Unity precisa fornecer um método estático que
o Unity consiga alcançar por `-executeMethod`. Na prática, isso normalmente
significa colocar o método em um script dentro de `Assets/Editor` ou em outra
assembly exclusiva de Editor.

## O que o HGP passa para o Unity Editor

Além dos argumentos de linha de comando acima, o HGP também passa metadados por
variáveis de ambiente para que o seu script de build consiga inspecionar a
execução atual quando precisar.

- `HGB_OUTPUT_PATH`: caminho canônico de saída para a execução atual de build
- `HGB_OUTPUT_KIND`: tipo de output quando o target usa uma saída de runtime
  que não é empacotada como arquivo
- `HGB_BUILD_RUN_ID`: identificador da execução atual de build
- `HGB_RELEASE_RUN_ID`: identificador da release pai
- `HGB_BUILD_TARGET_ID`: identificador do target de build no HGP
- `HGB_LOG_PATH`: caminho do arquivo de log usado na execução atual
- `HGB_TARGET_PLATFORM`: valor de plataforma Unity selecionado para o target
- `HGB_UNITY_VERSION`: versão do Unity Editor esperada para a execução

A parte importante aqui é o contrato do caminho de saída: o seu script Unity
deve respeitar `HGB_OUTPUT_PATH` ou o argumento `-hgbOutputPath`, em vez de
fixar um local final de exportação no código.

## Como os caminhos de saída funcionam

O HGP calcula o caminho de saída antes de o Unity começar. O seu script deve
tratar esse caminho como o ponto de entrega dos artefatos.

Para saídas em formato de arquivo, como `.exe`, `.apk` ou `.x86_64`, o Unity
pode escrever diretamente no caminho fornecido.

Para saídas em formato de diretório, como WebGL, iOS, exports de console ou
diretórios staged de standalone, o seu script deve criar o diretório e deixar
o Unity escrever lá dentro.

Se o target do HGP estiver configurado para empacotamento em arquivo, o Unity
ainda escreve primeiro em um caminho de staging e o HGP cuida do empacotamento
final depois que o build termina.

## O que o seu projeto Unity precisa fornecer

No mínimo, o seu projeto deve:

- expor um método de build estático para cada target de build do HGP que você
  configurar
- coletar as cenas que devem entrar no build
- resolver o caminho de saída a partir de `HGB_OUTPUT_PATH` ou `-hgbOutputPath`
- passar o caminho resolvido para `BuildPipeline.BuildPlayer`
- falhar de forma clara quando o Unity reportar um erro de build

O exemplo abaixo mostra um jeito de fazer isso mantendo a classe compatível com
targets comuns de standalone, mobile, WebGL e plataformas de console.

## Classe de build Unity de exemplo

Salve a classe abaixo como `Assets/Editor/HGPBuilder.cs` no seu projeto Unity
e depois mantenha os nomes de método sugeridos em `HGPBuilder.Perform...` ou
renomeie para combinar com os métodos de build que você configurou no HGP.

Baixe o arquivo bruto:
<a href="../../../../assets/code/HGPBuilder.cs">HGPBuilder.cs</a>

Além do contrato mínimo acima, este exemplo também mostra alguns reforços
práticos: um log diagnóstico lateral para falhas em batch mode, uma checagem
explícita de suporte ao target antes de invocar o pipeline e scripting defines
opcionais consumidos de `HGB_EXTRA_SCRIPTING_DEFINES` ou
`-hgbExtraScriptingDefines` quando a sua própria automação os fornecer.

```csharp
using System;
using System.IO;
using System.Linq;
using UnityEditor;
using UnityEditor.Build.Reporting;
using UnityEngine;

/// <summary>
/// Provides example Unity Editor executeMethod entrypoints for an
/// HGP-integrated project across publicly documented Unity build targets.
/// Closed-platform examples still require the corresponding Unity module and
/// vendor access in the installed editor.
/// </summary>
/// <remarks>
/// The type name is not part of the build contract. HGP resolves whichever
/// <c>&lt;TypeName&gt;.&lt;MethodName&gt;</c> pair is configured by the
/// application, so integrators may rename this class to match their own
/// naming conventions.
/// Most <c>Perform*</c> methods are illustrative templates that map public
/// <see cref="BuildTarget"/> values to a consistent example build flow.
/// The entrypoints validated in practice for this sample are the Windows
/// standalone, Linux64, and WebGL variants.
/// Treat this class as a strong starting point for project-specific build
/// orchestration rather than a fixed framework requirement.
/// </remarks>
public static class HGPBuilder
{
    private const string OutputPathEnvironmentVariableName = "HGB_OUTPUT_PATH";

    private const string OutputPathArgumentName = "-hgbOutputPath";

    private const string ExtraDefinesEnvironmentVariableName =
        "HGB_EXTRA_SCRIPTING_DEFINES";

    private const string ExtraDefinesArgumentName =
        "-hgbExtraScriptingDefines";

    private static readonly string DiagnosticLogPath = Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
        "Temp",
        $"{SafeProductName()}-hgp-builder-diagnostics.log");

    #region API

    /// <summary>
    /// Example entrypoint for the macOS standalone player target.
    /// </summary>
    public static void PerformMacOS()
    {
        RunStandaloneBuild(
            nameof(PerformMacOS),
            BuildTarget.StandaloneOSX,
            ".app",
            "macOS");
    }

    /// <summary>
    /// Example entrypoint for the Windows 32-bit standalone player target.
    /// </summary>
    public static void PerformWindows32()
    {
        RunStandaloneBuild(
            nameof(PerformWindows32),
            BuildTarget.StandaloneWindows,
            ".exe",
            "Windows32");
    }

    /// <summary>
    /// Example entrypoint for the iOS player target.
    /// </summary>
    public static void PerformIOS()
    {
        RunDirectoryBuild(
            nameof(PerformIOS),
            BuildTarget.iOS,
            "iOS");
    }

    /// <summary>
    /// Example entrypoint for the Android player target.
    /// </summary>
    public static void PerformAndroid()
    {
        RunStandaloneBuild(
            nameof(PerformAndroid),
            BuildTarget.Android,
            ".apk",
            "Android");
    }

    /// <summary>
    /// Example entrypoint for the Windows 64-bit standalone player target.
    /// </summary>
    public static void PerformWindows64()
    {
        RunStandaloneBuild(
            nameof(PerformWindows64),
            BuildTarget.StandaloneWindows64,
            ".exe",
            "Windows64");
    }

    /// <summary>
    /// Example entrypoint for the WebGL player target.
    /// </summary>
    public static void PerformWebGL()
    {
        RunDirectoryBuild(
            nameof(PerformWebGL),
            BuildTarget.WebGL,
            "WebGL");
    }

    /// <summary>
    /// Example entrypoint for the Universal Windows Platform player target.
    /// </summary>
    public static void PerformUWP()
    {
        RunDirectoryBuild(
            nameof(PerformUWP),
            BuildTarget.WSAPlayer,
            "UWP");
    }

    /// <summary>
    /// Example entrypoint for the Linux 64-bit standalone player target.
    /// </summary>
    public static void PerformLinux64()
    {
        RunStandaloneBuild(
            nameof(PerformLinux64),
            BuildTarget.StandaloneLinux64,
            ".x86_64",
            "Linux64");
    }

    /// <summary>
    /// Example entrypoint for the PlayStation 4 player target.
    /// </summary>
    public static void PerformPS4()
    {
        RunDirectoryBuild(
            nameof(PerformPS4),
            BuildTarget.PS4,
            "PS4");
    }

    /// <summary>
    /// Example entrypoint for the Xbox One player target.
    /// </summary>
    public static void PerformXboxOne()
    {
        RunDirectoryBuild(
            nameof(PerformXboxOne),
            BuildTarget.XboxOne,
            "XboxOne");
    }

    /// <summary>
    /// Example entrypoint for the tvOS player target.
    /// </summary>
    public static void PerformTvOS()
    {
        RunDirectoryBuild(
            nameof(PerformTvOS),
            BuildTarget.tvOS,
            "tvOS");
    }

    /// <summary>
    /// Example entrypoint for the Nintendo Switch player target.
    /// </summary>
    public static void PerformSwitch()
    {
        RunDirectoryBuild(
            nameof(PerformSwitch),
            BuildTarget.Switch,
            "Switch");
    }

    /// <summary>
    /// Example entrypoint for the Linux dedicated server target.
    /// </summary>
    public static void PerformDedicatedServerLinux()
    {
        RunStandaloneBuild(
            nameof(PerformDedicatedServerLinux),
            BuildTarget.LinuxHeadlessSimulation,
            ".x86_64",
            "DedicatedServerLinux");
    }

    /// <summary>
    /// Example entrypoint for the Xbox Series GameCore player target.
    /// </summary>
    public static void PerformGameCoreXboxSeries()
    {
        RunDirectoryBuild(
            nameof(PerformGameCoreXboxSeries),
            BuildTarget.GameCoreXboxSeries,
            "GameCoreXboxSeries");
    }

    /// <summary>
    /// Example entrypoint for the Xbox One GameCore player target.
    /// </summary>
    public static void PerformGameCoreXboxOne()
    {
        RunDirectoryBuild(
            nameof(PerformGameCoreXboxOne),
            BuildTarget.GameCoreXboxOne,
            "GameCoreXboxOne");
    }

    /// <summary>
    /// Example entrypoint for the PlayStation 5 player target.
    /// </summary>
    public static void PerformPS5()
    {
        RunDirectoryBuild(
            nameof(PerformPS5),
            BuildTarget.PS5,
            "PS5");
    }

    /// <summary>
    /// Example entrypoint for the visionOS player target.
    /// </summary>
    public static void PerformVisionOS()
    {
        RunDirectoryBuild(
            nameof(PerformVisionOS),
            BuildTarget.VisionOS,
            "visionOS");
    }

    #endregion

    #region Build Flow

    /// <summary>
    /// Wraps one batch entrypoint with a side-channel diagnostic log so batch
    /// failures that terminate before Unity flushes the main log can still be
    /// inspected after the process exits.
    /// </summary>
    /// <param name="operationName">Logical build operation label.</param>
    /// <param name="action">Operation body to execute.</param>
    private static void RunWithDiagnostics(
        string operationName,
        Action action)
    {
        ResetDiagnosticLog();
        AppendDiagnostic($"{operationName} started.");

        try
        {
            action();
            AppendDiagnostic($"{operationName} completed successfully.");
        }
        catch (Exception exception)
        {
            AppendDiagnostic(
                $"{operationName} failed: {exception}");

            throw;
        }
    }

    /// <summary>
    /// Executes one standalone-style build entrypoint with consistent
    /// diagnostics so batch callers receive the same behavior regardless of
    /// target.
    /// </summary>
    /// <param name="operationName">Logical build operation label.</param>
    /// <param name="target">Unity build target to compile.</param>
    /// <param name="playerExtension">Executable extension for the player.</param>
    /// <param name="defaultFolderName">Fallback output folder label.</param>
    private static void RunStandaloneBuild(
        string operationName,
        BuildTarget target,
        string playerExtension,
        string defaultFolderName)
    {
        RunWithDiagnostics(
            operationName,
            () => BuildStandalone(
                target,
                playerExtension,
                defaultFolderName));
    }

    /// <summary>
    /// Executes one directory-style build entrypoint with consistent
    /// diagnostics so batch callers receive the same behavior regardless of
    /// target.
    /// </summary>
    /// <param name="operationName">Logical build operation label.</param>
    /// <param name="target">Unity build target to compile.</param>
    /// <param name="defaultFolderName">Fallback output folder label.</param>
    private static void RunDirectoryBuild(
        string operationName,
        BuildTarget target,
        string defaultFolderName)
    {
        RunWithDiagnostics(
            operationName,
            () => BuildDirectoryTarget(
                target,
                defaultFolderName));
    }

    /// <summary>
    /// Illustrates how the example routes standalone-style targets to a player
    /// file or app bundle under the requested output directory.
    /// </summary>
    /// <param name="target">Unity build target to compile.</param>
    /// <param name="playerExtension">Executable extension for the player.</param>
    /// <param name="defaultFolderName">Fallback output folder label.</param>
    private static void BuildStandalone(
        BuildTarget target,
        string playerExtension,
        string defaultFolderName)
    {
        string requestedPath = ResolveOutputPath(
            Path.Combine("Builds", defaultFolderName));

        string playerOutputPath = NormalizeStandaloneOutputPath(
            requestedPath,
            playerExtension);


        RunBuild(target, playerOutputPath);
    }

    /// <summary>
    /// Illustrates how the example routes directory-style targets such as
    /// WebGL, mobile exports, and console exports.
    /// </summary>
    /// <param name="target">Unity build target to compile.</param>
    /// <param name="defaultFolderName">Fallback output folder label.</param>
    private static void BuildDirectoryTarget(
        BuildTarget target,
        string defaultFolderName)
    {
        string requestedPath = ResolveOutputPath(
            Path.Combine("Builds", defaultFolderName));

        requestedPath = NormalizeBuildContainerPath(requestedPath);
        EnsureDirectoryExists(requestedPath);
        RunBuild(target, requestedPath);
    }

    /// <summary>
    /// Executes one example Unity build with the enabled Build Settings scenes
    /// and fails fast when Unity reports a non-success result.
    /// </summary>
    /// <param name="target">Unity build target to compile.</param>
    /// <param name="locationPathName">
    /// Target output path passed to BuildPipeline.BuildPlayer.
    /// </param>
    private static void RunBuild(BuildTarget target, string locationPathName)
    {
        AppendDiagnostic(
            $"RunBuild invoked for target '{target}' to '{locationPathName}'.");

        if (string.IsNullOrWhiteSpace(locationPathName))
        {
            throw new Exception("Build output path must not be empty.");
        }

        EnsureBuildTargetSupport(target);

        string[] scenes = GetEnabledScenes();
        string directory = Path.GetDirectoryName(locationPathName);

        if (!string.IsNullOrWhiteSpace(directory))
        {
            EnsureDirectoryExists(directory);
        }

        Debug.Log(
            $"Starting {target} build to '{locationPathName}' with {scenes.Length} scene(s).");

        BuildPlayerOptions options = CreateBuildPlayerOptions(
            scenes,
            target,
            locationPathName);

        BuildReport report = BuildPipeline.BuildPlayer(options);

        AppendDiagnostic(
            $"BuildPipeline.BuildPlayer result: {report.summary.result}. " +
            $"Errors={report.summary.totalErrors}, " +
            $"Warnings={report.summary.totalWarnings}.");

        if (report.summary.result != BuildResult.Succeeded)
        {
            throw new Exception(
                $"Unity build failed for {target}: {report.summary.result}. " +
                $"Errors: {report.summary.totalErrors}. " +
                $"Warnings: {report.summary.totalWarnings}.");
        }

        Debug.Log(
            $"Completed {target} build at '{locationPathName}'. " +
            $"Size: {report.summary.totalSize} bytes.");
    }

    /// <summary>
    /// Creates the player build options used by the batch example and injects
    /// optional scripting defines from HGP-specific environment variables or
    /// command-line arguments without assuming project-local assets.
    /// </summary>
    /// <param name="scenes">Enabled scene asset paths to include.</param>
    /// <param name="target">Unity build target to compile.</param>
    /// <param name="locationPathName">Target output path.</param>
    /// <returns>The configured build options.</returns>
    private static BuildPlayerOptions CreateBuildPlayerOptions(
        string[] scenes,
        BuildTarget target,
        string locationPathName)
    {
        string[] extraScriptingDefines = ReadAdditionalScriptingDefines();

        AppendDiagnostic(
            $"Editor active build target before build: '{EditorUserBuildSettings.activeBuildTarget}'.");
        AppendDiagnostic(
            "Editor active build target group before build: '" +
            BuildPipeline.GetBuildTargetGroup(
                EditorUserBuildSettings.activeBuildTarget) +
            "'.");

        if (extraScriptingDefines.Length > 0)
        {
            AppendDiagnostic(
                "Applying additional scripting defines: " +
                string.Join(", ", extraScriptingDefines));
        }

        return new BuildPlayerOptions
        {
            scenes = scenes,
            target = target,
            locationPathName = locationPathName,
            options = BuildOptions.None,
            extraScriptingDefines =
                extraScriptingDefines.Length > 0
                    ? extraScriptingDefines
                    : null,
        };
    }

    /// <summary>
    /// Verifies that the installed Unity editor can build the requested target
    /// before the example attempts to invoke the build pipeline.
    /// </summary>
    /// <param name="target">Unity build target to validate.</param>
    private static void EnsureBuildTargetSupport(BuildTarget target)
    {
        BuildTargetGroup targetGroup = BuildPipeline.GetBuildTargetGroup(target);
        BuildTargetGroup supportGroup = targetGroup == BuildTargetGroup.Unknown
            ? BuildTargetGroup.Unknown
            : targetGroup;

        AppendDiagnostic(
            $"Resolved build target group '{supportGroup}' for target '{target}'.");

        if (BuildPipeline.IsBuildTargetSupported(supportGroup, target))
        {
            return;
        }

        throw new Exception(
            $"The installed Unity editor does not support the '{target}' " +
            "build target. Install the corresponding platform module " +
            "or run this method in an editor that already includes it.");
    }

    #endregion

    #region Scene Resolution

    /// <summary>
    /// Collects the enabled scenes used by the example build entrypoints.
    /// </summary>
    /// <returns>The ordered list of enabled scene asset paths.</returns>
    private static string[] GetEnabledScenes()
    {
        string[] scenes = EditorBuildSettings.scenes
            .Where(scene => scene.enabled)
            .Select(scene => scene.path)
            .ToArray();

        if (scenes.Length == 0)
        {
            throw new Exception(
                "No enabled scenes were found in Build Settings.");
        }

        return scenes;
    }

    #endregion

    #region Path Resolution

    /// <summary>
    /// Resolves the example output path requested by HGP,
    /// preferring the environment variable contract and then the explicit
    /// command-line argument before falling back to a repository-local default
    /// path.
    /// </summary>
    /// <param name="fallbackPath">Project-local fallback output path.</param>
    /// <returns>The normalized output path for the current build run.</returns>
    private static string ResolveOutputPath(string fallbackPath)
    {
        string fromEnvironment = Environment.GetEnvironmentVariable(
            OutputPathEnvironmentVariableName);

        if (!string.IsNullOrWhiteSpace(fromEnvironment))
        {
            return NormalizePath(fromEnvironment);
        }

        string fromCommandLine = ReadNamedArgument(OutputPathArgumentName);

        if (!string.IsNullOrWhiteSpace(fromCommandLine))
        {
            return NormalizePath(fromCommandLine);
        }

        return NormalizePath(fallbackPath);
    }

    /// <summary>
    /// Reads the value that follows a named command-line argument in the
    /// example executeMethod contract.
    /// </summary>
    /// <param name="argumentName">Argument name to search for.</param>
    /// <returns>
    /// The value that follows the named argument, or null when the argument is
    /// absent.
    /// </returns>
    private static string ReadNamedArgument(string argumentName)
    {
        string[] arguments = Environment.GetCommandLineArgs();

        for (int index = 0; index < arguments.Length - 1; index++)
        {
            if (string.Equals(
                arguments[index],
                argumentName,
                StringComparison.Ordinal))
            {
                return arguments[index + 1];
            }
        }

        return null;
    }

    /// <summary>
    /// Reads optional extra scripting defines supplied by HGP callers so the
    /// example builder can remain self-contained while still supporting
    /// project-specific compile flags when needed.
    /// </summary>
    /// <returns>The distinct additional scripting defines to apply.</returns>
    private static string[] ReadAdditionalScriptingDefines()
    {
        string fromEnvironment = Environment.GetEnvironmentVariable(
            ExtraDefinesEnvironmentVariableName);
        string fromCommandLine = ReadNamedArgument(ExtraDefinesArgumentName);

        return new[]
            {
                fromEnvironment,
                fromCommandLine,
            }
            .Where(value => !string.IsNullOrWhiteSpace(value))
            .SelectMany(value => value.Split(
                new[]
                {
                    ';',
                    ',',
                },
                StringSplitOptions.RemoveEmptyEntries))
            .Select(value => value.Trim())
            .Where(value => !string.IsNullOrWhiteSpace(value))
            .Distinct(StringComparer.Ordinal)
            .ToArray();
    }

    /// <summary>
    /// Normalizes a standalone example output path so folder-style requests
    /// still end with a concrete player file or app bundle path.
    /// </summary>
    /// <param name="requestedPath">Requested artifact path.</param>
    /// <param name="playerExtension">Expected executable extension.</param>
    /// <returns>The final player path passed to Unity.</returns>
    private static string NormalizeStandaloneOutputPath(
        string requestedPath,
        string playerExtension)
    {
        requestedPath = NormalizeBuildContainerPath(requestedPath);

        if (requestedPath.EndsWith(
            playerExtension,
            StringComparison.OrdinalIgnoreCase))
        {
            return requestedPath;
        }

        return Path.Combine(requestedPath, SafeProductName() + playerExtension);
    }

    /// <summary>
    /// Normalizes an example build container path so legacy zip-shaped
    /// requests are treated as plain directories for runtime-owned packaging.
    /// </summary>
    /// <param name="path">Requested artifact path.</param>
    /// <returns>The normalized directory path.</returns>
    private static string NormalizeBuildContainerPath(string path)
    {
        if (path.EndsWith(".zip", StringComparison.OrdinalIgnoreCase))
        {
            return path.Substring(0, path.Length - ".zip".Length);
        }

        return path;
    }

    /// <summary>
    /// Normalizes a potentially relative example output path against the Unity
    /// project root.
    /// </summary>
    /// <param name="rawPath">Raw path value to normalize.</param>
    /// <returns>The absolute normalized path.</returns>
    private static string NormalizePath(string rawPath)
    {
        return Path.GetFullPath(rawPath.Trim());
    }

    #endregion

    #region Filesystem Helpers

    /// <summary>
    /// Recreates the side-channel diagnostic file used by batch entrypoints.
    /// </summary>
    private static void ResetDiagnosticLog()
    {
        string directory = Path.GetDirectoryName(DiagnosticLogPath);

        if (!string.IsNullOrWhiteSpace(directory))
        {
            Directory.CreateDirectory(directory);
        }

        File.WriteAllText(DiagnosticLogPath, string.Empty);
    }

    /// <summary>
    /// Appends one timestamped diagnostic line to the side-channel batch log.
    /// </summary>
    /// <param name="message">Diagnostic payload to append.</param>
    private static void AppendDiagnostic(string message)
    {
        string line =
            $"[{DateTime.UtcNow:O}] {message}{Environment.NewLine}";

        File.AppendAllText(DiagnosticLogPath, line);
    }

    /// <summary>
    /// Ensures an example output directory exists before Unity writes build
    /// artifacts into it.
    /// </summary>
    /// <param name="path">Directory path to create.</param>
    private static void EnsureDirectoryExists(string path)
    {
        if (string.IsNullOrWhiteSpace(path))
        {
            return;
        }

        Directory.CreateDirectory(path);
    }

    /// <summary>
    /// Produces a filesystem-safe product name for example player filenames.
    /// </summary>
    /// <returns>The sanitized player name.</returns>
    private static string SafeProductName()
    {
        string productName = PlayerSettings.productName;

        if (string.IsNullOrWhiteSpace(productName))
        {
            productName = Application.productName;
        }

        char[] invalidCharacters = Path.GetInvalidFileNameChars();

        return new string(
            productName
                .Select(character =>
                    invalidCharacters.Contains(character) ? '_' : character)
                .ToArray());
    }

    #endregion
}
```
