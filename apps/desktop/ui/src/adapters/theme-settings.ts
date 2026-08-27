export type ThemePreset = "midnight" | "graphite" | "aurora";
export type ThemeDensity = "compact" | "comfortable";
export type ThemeFont = "system" | "humanist" | "mono";
export type ThemeBackground = "none" | "soft-glow" | "aurora" | "glass";

export interface ThemeSettings {
  schemaVersion: 1;
  preset: ThemePreset;
  density: ThemeDensity;
  font: ThemeFont;
  fontScalePercent: number;
  accent: string;
  background: ThemeBackground;
}

export function parseThemeSettings(value: unknown): ThemeSettings {
  if (
    !isRecord(value) ||
    value.schemaVersion !== 1 ||
    !isOneOf(value.preset, ["midnight", "graphite", "aurora"]) ||
    !isOneOf(value.density, ["compact", "comfortable"]) ||
    !isOneOf(value.font, ["system", "humanist", "mono"]) ||
    typeof value.fontScalePercent !== "number" ||
    !Number.isSafeInteger(value.fontScalePercent) ||
    value.fontScalePercent < 85 ||
    value.fontScalePercent > 130 ||
    typeof value.accent !== "string" ||
    !/^#[0-9a-f]{6}$/i.test(value.accent) ||
    !isOneOf(value.background, ["none", "soft-glow", "aurora", "glass"])
  ) {
    throw new Error("The local host returned invalid Themes settings.");
  }
  return value as unknown as ThemeSettings;
}

export function applyThemeSettings(settings: ThemeSettings): void {
  const root = document.documentElement;
  root.dataset.theme = settings.preset;
  root.dataset.density = settings.density;
  root.dataset.font = settings.font;
  root.dataset.background = settings.background;
  root.style.setProperty("--accent", settings.accent);
  root.style.setProperty(
    "--font-scale",
    String(settings.fontScalePercent / 100),
  );
}

export async function loadAndApplyThemeSettings(): Promise<void> {
  try {
    const response = await fetch("/api/settings/themes", {
      cache: "no-store",
      headers: { Accept: "application/json" },
      signal: AbortSignal.timeout(1_500),
    });
    if (!response.ok) return;
    applyThemeSettings(parseThemeSettings(await response.json()));
  } catch {
    // The plain Vite development shell has no native settings host.
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isOneOf<T extends string>(
  value: unknown,
  choices: readonly T[],
): value is T {
  return typeof value === "string" && choices.includes(value as T);
}
