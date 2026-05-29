using System;
using System.IO;
using System.Linq;
using UnityEditor;
using UnityEditor.Build.Reporting;
using UnityEngine;

/// <summary>
/// Example Unity Editor entrypoints that show how an HGP-integrated Unity
/// project can
/// expose one executeMethod target per officially documented Unity
/// BuildTarget value in the public scripting API.
/// Closed-platform examples still require the corresponding Unity module and
/// vendor access in the installed editor.
/// </summary>
public static class UnityBuilderExample
{
    #region API

    /// <summary>
    /// Example entrypoint for the macOS standalone player target.
    /// </summary>
    public static void PerformMacOS()
    {
        BuildStandalone(
            BuildTarget.StandaloneOSX,
            ".app",
            "macOS");
    }

    /// <summary>
    /// Example entrypoint for the Windows 32-bit standalone player target.
    /// </summary>
    public static void PerformWindows32()
    {
        BuildStandalone(
            BuildTarget.StandaloneWindows,
            ".exe",
            "Windows32");
    }

    /// <summary>
    /// Example entrypoint for the iOS player target.
    /// </summary>
    public static void PerformIOS()
    {
        BuildDirectoryTarget(BuildTarget.iOS, "iOS");
    }

    /// <summary>
    /// Example entrypoint for the Android player target.
    /// </summary>
    public static void PerformAndroid()
    {
        BuildStandalone(
            BuildTarget.Android,
            ".apk",
            "Android");
    }

    /// <summary>
    /// Example entrypoint for the Windows 64-bit standalone player target.
    /// </summary>
    public static void PerformWindows64()
    {
        BuildStandalone(
            BuildTarget.StandaloneWindows64,
            ".exe",
            "Windows64");
    }

    /// <summary>
    /// Example entrypoint for the WebGL player target.
    /// </summary>
    public static void PerformWebGL()
    {
        BuildDirectoryTarget(BuildTarget.WebGL, "WebGL");
    }

    /// <summary>
    /// Example entrypoint for the Universal Windows Platform player target.
    /// </summary>
    public static void PerformUWP()
    {
        BuildDirectoryTarget(BuildTarget.WSAPlayer, "UWP");
    }

    /// <summary>
    /// Example entrypoint for the Linux 64-bit standalone player target.
    /// </summary>
    public static void PerformLinux64()
    {
        BuildStandalone(
            BuildTarget.StandaloneLinux64,
            ".x86_64",
            "Linux64");
    }

    /// <summary>
    /// Example entrypoint for the PlayStation 4 player target.
    /// </summary>
    public static void PerformPS4()
    {
        BuildDirectoryTarget(BuildTarget.PS4, "PS4");
    }

    /// <summary>
    /// Example entrypoint for the Xbox One player target.
    /// </summary>
    public static void PerformXboxOne()
    {
        BuildDirectoryTarget(BuildTarget.XboxOne, "XboxOne");
    }

    /// <summary>
    /// Example entrypoint for the tvOS player target.
    /// </summary>
    public static void PerformTvOS()
    {
        BuildDirectoryTarget(BuildTarget.tvOS, "tvOS");
    }

    /// <summary>
    /// Example entrypoint for the Nintendo Switch player target.
    /// </summary>
    public static void PerformSwitch()
    {
        BuildDirectoryTarget(BuildTarget.Switch, "Switch");
    }

    /// <summary>
    /// Example entrypoint for the Linux dedicated server target.
    /// </summary>
    public static void PerformDedicatedServerLinux()
    {
        BuildStandalone(
            BuildTarget.LinuxHeadlessSimulation,
            ".x86_64",
            "DedicatedServerLinux");
    }

    /// <summary>
    /// Example entrypoint for the Xbox Series GameCore player target.
    /// </summary>
    public static void PerformGameCoreXboxSeries()
    {
        BuildDirectoryTarget(
            BuildTarget.GameCoreXboxSeries,
            "GameCoreXboxSeries");
    }

    /// <summary>
    /// Example entrypoint for the Xbox One GameCore player target.
    /// </summary>
    public static void PerformGameCoreXboxOne()
    {
        BuildDirectoryTarget(
            BuildTarget.GameCoreXboxOne,
            "GameCoreXboxOne");
    }

    /// <summary>
    /// Example entrypoint for the PlayStation 5 player target.
    /// </summary>
    public static void PerformPS5()
    {
        BuildDirectoryTarget(BuildTarget.PS5, "PS5");
    }

    /// <summary>
    /// Example entrypoint for the visionOS player target.
    /// </summary>
    public static void PerformVisionOS()
    {
        BuildDirectoryTarget(BuildTarget.VisionOS, "visionOS");
    }

    #endregion

    #region Build Flow

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
        if (string.IsNullOrWhiteSpace(locationPathName))
        {
            throw new Exception("Build output path must not be empty.");
        }

        string[] scenes = GetEnabledScenes();
        string directory = Path.GetDirectoryName(locationPathName);

        if (!string.IsNullOrWhiteSpace(directory))
        {
            EnsureDirectoryExists(directory);
        }

        Debug.Log(
            $"Starting {target} build to '{locationPathName}' with {scenes.Length} scene(s).");

        BuildPlayerOptions options = new BuildPlayerOptions
        {
            scenes = scenes,
            target = target,
            locationPathName = locationPathName,
            options = BuildOptions.None,
        };

        BuildReport report = BuildPipeline.BuildPlayer(options);

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
            "HGB_OUTPUT_PATH");

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

        if (string.IsNullOrWhiteSpace(productName))
        {
            productName = "Revolutions";
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