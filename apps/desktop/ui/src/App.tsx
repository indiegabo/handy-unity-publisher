import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

import { Button, IconButton } from "./components/Button";
import { SelectField, TextAreaField, TextField } from "./components/Field";
import { Icon, type IconName } from "./components/Icon";
import { Badge, SurfacePanel, type BadgeTone } from "./components/Surface";

type BridgeState = {
  detail: string;
  kind: "loading" | "ready" | "warning";
  version: string | null;
};

type ApplicationVersion = {
  app_version: string;
};

type LaneScope = "all" | "configuration" | "delivery" | "runtime";

type PreviewRow = {
  copy: string;
  group: Exclude<LaneScope, "all">;
  icon: IconName;
  metric: string;
  title: string;
  tone: BadgeTone;
};

const glyphNames: readonly IconName[] = [
  "search",
  "play",
  "plus",
  "refresh",
  "settings",
  "layout",
  "server",
  "box",
  "folder",
  "terminal",
  "spark",
  "arrowUpRight",
] as const;

const laneOptions = [
  { label: "All lanes", value: "all" },
  { label: "Runtime", value: "runtime" },
  { label: "Delivery", value: "delivery" },
  { label: "Configuration", value: "configuration" },
] as const;

const environmentOptions = [
  { label: "Local host", value: "local" },
  { label: "Operator sandbox", value: "sandbox" },
  { label: "Release rehearsal", value: "rehearsal" },
] as const;

const densityOptions = [
  { label: "Dense", value: "dense" },
  { label: "Dense+", value: "dense-plus" },
  { label: "Audit", value: "audit" },
] as const;

const previewRows: readonly PreviewRow[] = [
  {
    copy: "Runs the local poller, updates queue state, and reclaims release pressure without wasting tray space.",
    group: "runtime",
    icon: "refresh",
    metric: "runtime / 12s cadence",
    title: "Automation poll-once",
    tone: "strong",
  },
  {
    copy: "Surfaces queued releases, build fan-out, and blocked publish work in one narrow list row.",
    group: "delivery",
    icon: "box",
    metric: "delivery / 3 blocked",
    title: "Release backlog",
    tone: "neutral",
  },
  {
    copy: "Shows editor discovery, license state, and host prerequisites with tight metadata columns.",
    group: "runtime",
    icon: "server",
    metric: "runtime / 4 editors",
    title: "Unity runners",
    tone: "muted",
  },
  {
    copy: "Keeps secret bindings compact, explicit, and operator-readable instead of bloated form soup.",
    group: "configuration",
    icon: "folder",
    metric: "configuration / 6 mapped",
    title: "Credential bindings",
    tone: "neutral",
  },
  {
    copy: "Fits command targeting, environment selection, and small actions into a compact console band.",
    group: "configuration",
    icon: "layout",
    metric: "configuration / dense form",
    title: "Command controls",
    tone: "strong",
  },
] as const;

const themeNotes = [
  { label: "References", value: "Yaak / Hoppscotch" },
  { label: "Palette", value: "Black + gray" },
  { label: "Corner radius", value: "5px" },
  { label: "Density", value: "Tool-first" },
] as const;

function App() {
  const [bridgeState, setBridgeState] = useState<BridgeState>({
    detail: "Connecting to the desktop shell command bridge...",
    kind: "loading",
    version: null,
  });
  const [commandQuery, setCommandQuery] = useState("release");
  const [environment, setEnvironment] = useState("local");
  const [scope, setScope] = useState<LaneScope>("all");
  const [density, setDensity] = useState("dense");
  const [repositoryPath, setRepositoryPath] = useState(
    "pipelines/hup-runtime.yaml",
  );
  const [notes, setNotes] = useState(
    "Keep buttons short, forms tight, and containers locked to 5px radius.",
  );

  useEffect(() => {
    let cancelled = false;

    async function loadShellVersion() {
      try {
        const response = await invoke<ApplicationVersion>("application_version");
        if (cancelled) {
          return;
        }

        setBridgeState({
          detail:
            "The Tauri bridge is alive and the reusable UI kit is rendering inside the desktop shell.",
          kind: "ready",
          version: response.app_version,
        });
      } catch (error) {
        if (cancelled) {
          return;
        }

        setBridgeState({
          detail: buildBridgeWarning(error),
          kind: "warning",
          version: null,
        });
      }
    }

    void loadShellVersion();

    return () => {
      cancelled = true;
    };
  }, []);

  const filteredRows = previewRows.filter((row) => {
    if (scope !== "all" && row.group !== scope) {
      return false;
    }

    if (!commandQuery.trim()) {
      return true;
    }

    const normalizedQuery = commandQuery.trim().toLowerCase();
    return [row.title, row.copy, row.metric].some((field) =>
      field.toLowerCase().includes(normalizedQuery),
    );
  });

  const bridgeTone = mapBridgeTone(bridgeState.kind);
  const showcaseStats = [
    {
      label: "Bridge",
      value: bridgeState.kind === "ready" ? "Connected" : "Preview",
    },
    { label: "Shell", value: bridgeState.version ?? "0.1.x" },
    { label: "Palette", value: "Black / gray" },
    { label: "Radius", value: "5px" },
  ] as const;

  return (
    <main className="app-shell">
      <div className="showcase-frame">
        <header className="shell-header">
          <div className="shell-brand">
            <p className="eyebrow">Yaak x Hoppscotch direction</p>
            <div className="shell-brand-row">
              <h1 className="wordmark">HUP UI Kit</h1>
              <Badge tone={bridgeTone}>{buildBridgeLabel(bridgeState.kind)}</Badge>
            </div>
            <p className="shell-copy">
              Compact, dark, tool-first primitives for the Tauri operator shell.
            </p>
          </div>

          <div className="shell-toolbar" aria-label="Header actions">
            <IconButton icon="search" label="Search commands" size="sm" variant="ghost" />
            <IconButton icon="settings" label="Open preferences" size="sm" variant="ghost" />
            <Button leadingIcon="refresh" size="sm" variant="secondary">
              Sync
            </Button>
            <Button leadingIcon="play" size="sm" variant="primary">
              Launch
            </Button>
          </div>
        </header>

        <section className="hero-stats" aria-label="Theme summary">
          {showcaseStats.map((stat) => (
            <article className="hero-stat" key={stat.label}>
              <span className="hero-stat__label">{stat.label}</span>
              <strong className="hero-stat__value">{stat.value}</strong>
            </article>
          ))}
        </section>

        <section className="showcase-layout">
          <aside className="showcase-rail">
            <SurfacePanel
              description="Black and gray surfaces with narrow spacing and 5px corners everywhere that matters."
              eyebrow="Theme"
              title="Dark monochrome"
            >
              <div className="token-list">
                {themeNotes.map((note) => (
                  <div className="token-row" key={note.label}>
                    <span className="token-row__label">{note.label}</span>
                    <strong className="token-row__value">{note.value}</strong>
                  </div>
                ))}
              </div>
              <p className="bridge-copy">{bridgeState.detail}</p>
            </SurfacePanel>

            <SurfacePanel
              description="Pure icons for buttons, lists, fields, and shell chrome."
              eyebrow="Icons"
              title="Base glyphs"
            >
              <div className="icon-grid">
                {glyphNames.map((iconName) => (
                  <div className="icon-swatch" key={iconName}>
                    <span className="icon-swatch__preview">
                      <Icon name={iconName} size={15} />
                    </span>
                    <span className="icon-swatch__label">
                      {formatIconLabel(iconName)}
                    </span>
                  </div>
                ))}
              </div>
            </SurfacePanel>
          </aside>

          <div className="showcase-main">
            <SurfacePanel
              description="Primary, secondary, ghost, icon-leading, icon-trailing, and icon-only controls."
              eyebrow="Buttons"
              title="Action set"
            >
              <div className="demo-stack">
                <div className="demo-row">
                  <Button leadingIcon="play" variant="primary">
                    Launch runtime
                  </Button>
                  <Button leadingIcon="plus" variant="secondary">
                    New pipeline
                  </Button>
                  <Button trailingIcon="arrowUpRight" variant="ghost">
                    Open logs
                  </Button>
                </div>
                <div className="demo-row">
                  <Button leadingIcon="refresh" size="sm" variant="secondary">
                    Compact sync
                  </Button>
                  <Button size="sm" variant="ghost">
                    Quiet action
                  </Button>
                  <IconButton icon="search" label="Find command" size="sm" variant="secondary" />
                  <IconButton icon="settings" label="Open settings" size="sm" variant="ghost" />
                  <IconButton icon="spark" label="Inspect shortcuts" size="sm" variant="primary" />
                </div>
              </div>
            </SurfacePanel>

            <SurfacePanel
              description="Reusable compact fields sized for tray forms, filters, and narrow command bars."
              eyebrow="Fields"
              title="Inputs and selects"
            >
              <div className="field-demo-grid">
                <TextField
                  hint="Filters preview rows"
                  label="Command search"
                  leadingIcon="search"
                  onChange={(event) => setCommandQuery(event.target.value)}
                  placeholder="Find lanes, releases, credentials"
                  type="search"
                  value={commandQuery}
                />
                <SelectField
                  hint="Current workspace"
                  label="Environment"
                  onChange={(event) => setEnvironment(event.target.value)}
                  options={environmentOptions}
                  value={environment}
                />
                <TextField
                  hint="Compact target input"
                  label="Pipeline manifest"
                  leadingIcon="folder"
                  onChange={(event) => setRepositoryPath(event.target.value)}
                  placeholder="pipelines/repository.yaml"
                  value={repositoryPath}
                />
                <SelectField
                  hint="Interactive row filter"
                  label="Lane scope"
                  onChange={(event) => setScope(event.target.value as LaneScope)}
                  options={laneOptions}
                  value={scope}
                />
                <SelectField
                  hint="Spacing preset"
                  label="Density"
                  onChange={(event) => setDensity(event.target.value)}
                  options={densityOptions}
                  value={density}
                />
                <TextAreaField
                  className="field-demo-grid__notes"
                  hint="Component note"
                  label="Design rule"
                  onChange={(event) => setNotes(event.target.value)}
                  placeholder="Write compact UI rules"
                  rows={4}
                  value={notes}
                />
              </div>
            </SurfacePanel>

            <SurfacePanel
              actions={<Badge tone="muted">{filteredRows.length} rows</Badge>}
              description="Rows combine icons, badges, buttons, and terse metadata the same way the real shell should."
              eyebrow="Showcase"
              title="Operator row preview"
            >
              <div className="preview-list">
                {filteredRows.map((row) => (
                  <article className="preview-row" key={row.title}>
                    <span className="preview-row__glyph">
                      <Icon name={row.icon} size={16} />
                    </span>

                    <div className="preview-row__body">
                      <div className="preview-row__title-row">
                        <h3 className="preview-row__title">{row.title}</h3>
                        <Badge tone={row.tone}>{row.group}</Badge>
                      </div>
                      <p className="preview-row__copy">{row.copy}</p>
                    </div>

                    <div className="preview-row__meta">
                      <span className="preview-row__metric">{row.metric}</span>
                    </div>

                    <div className="preview-row__actions">
                      <Button leadingIcon="play" size="sm" variant="secondary">
                        Open
                      </Button>
                      <IconButton icon="arrowUpRight" label={`Inspect ${row.title}`} size="sm" variant="ghost" />
                    </div>
                  </article>
                ))}

                {filteredRows.length === 0 ? (
                  <article className="preview-row preview-row--empty">
                    <div className="preview-row__body">
                      <h3 className="preview-row__title">No rows match the current filter.</h3>
                      <p className="preview-row__copy">
                        Clear the search or widen the lane scope to inspect the showcase again.
                      </p>
                    </div>
                  </article>
                ) : null}
              </div>
            </SurfacePanel>
          </div>
        </section>
      </div>
    </main>
  );
}

function buildBridgeWarning(error: unknown): string {
  if (error instanceof Error && error.message) {
    return `${error.message} Open the shell through Tauri to exercise live runtime commands.`;
  }

  if (typeof error === "string" && error.trim()) {
    return `${error.trim()} Open the shell through Tauri to exercise live runtime commands.`;
  }

  return "The Tauri command bridge is not available in a plain browser preview. Open the shell through Tauri to exercise live runtime commands.";
}

function buildBridgeLabel(kind: BridgeState["kind"]) {
  return kind === "ready" ? "bridge up" : kind === "loading" ? "connecting" : "preview";
}

function mapBridgeTone(kind: BridgeState["kind"]): BadgeTone {
  return kind === "ready" ? "strong" : kind === "loading" ? "neutral" : "muted";
}

function formatIconLabel(name: IconName) {
  return name
    .replace(/([A-Z])/g, " $1")
    .replace(/^./, (value) => value.toUpperCase());
}

export default App;