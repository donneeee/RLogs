export interface UiMessageShard {
  schema_version: 1;
  locale: string;
  namespace: string;
  messages: Record<string, string>;
}

export interface UiLocalizer {
  readonly locale: string;
  readonly loadedLocales: readonly string[];
  t(key: string, placeholders?: Readonly<Record<string, string | number>>): string;
  formatNumber(value: number, options?: Intl.NumberFormatOptions): string;
}

export type UiMessageModule = { default: unknown } | unknown;
export type UiMessageLoader = () => Promise<UiMessageModule>;
export type UiMessageLoaders = Readonly<Record<string, UiMessageLoader>>;

const BUILTIN_UI_MESSAGE_LOADERS = import.meta.glob<UiMessageModule>(
  "../../../../../plugins/builtin/localization/*/ui/**/*.json",
);

export async function loadUiLocalizer(
  requestedLocale: string,
  loaders: UiMessageLoaders = BUILTIN_UI_MESSAGE_LOADERS,
): Promise<UiLocalizer> {
  const locale = canonicalLocale(requestedLocale) ?? "en-US";
  const available = indexLoadersByLocale(loaders);
  const messages = new Map<string, string>();
  const loadedLocales: string[] = [];

  // Load fallback first so a more-specific package can replace its entries.
  for (const candidate of localeFallbackChain(locale).reverse()) {
    const localeLoaders = available.get(candidate.toLowerCase());
    if (localeLoaders === undefined) continue;
    for (const loader of localeLoaders) {
      const module = await loader();
      const shard = parseUiMessageShard(unwrapDefault(module), candidate);
      for (const [key, value] of Object.entries(shard.messages)) messages.set(key, value);
    }
    loadedLocales.push(candidate);
  }

  return {
    locale,
    loadedLocales: loadedLocales.reverse(),
    t(key, placeholders = {}) {
      const template = messages.get(key) ?? key;
      return template.replace(/\{([a-z][a-z0-9_]*)\}/gi, (match, name: string) => {
        const replacement = placeholders[name];
        return replacement === undefined ? match : String(replacement);
      });
    },
    formatNumber(value, options) {
      return new Intl.NumberFormat(locale, options).format(value);
    },
  };
}

export function localeFallbackChain(requestedLocale: string): string[] {
  const locale = canonicalLocale(requestedLocale) ?? "en-US";
  const base = locale.split("-")[0]!;
  return [...new Set([locale, base, "en-US"])];
}

export function parseUiMessageShard(value: unknown, expectedLocale?: string): UiMessageShard {
  if (!record(value) || value.schema_version !== 1 || typeof value.locale !== "string" ||
      typeof value.namespace !== "string" || !record(value.messages)) {
    throw new Error("Invalid RLogs UI localization shard.");
  }
  const locale = canonicalLocale(value.locale);
  if (locale === null || (expectedLocale !== undefined && locale !== canonicalLocale(expectedLocale))) {
    throw new Error("RLogs UI localization shard locale does not match its package.");
  }
  const messages: Record<string, string> = {};
  for (const [key, message] of Object.entries(value.messages)) {
    if (!key.startsWith(`${value.namespace}.`) || typeof message !== "string" || message.length === 0) {
      throw new Error(`Invalid RLogs UI localization message: ${key}`);
    }
    messages[key] = message;
  }
  return { schema_version: 1, locale, namespace: value.namespace, messages };
}

function indexLoadersByLocale(loaders: UiMessageLoaders): Map<string, UiMessageLoader[]> {
  const result = new Map<string, UiMessageLoader[]>();
  for (const [path, loader] of Object.entries(loaders).sort(([left], [right]) => left.localeCompare(right))) {
    const match = path.replaceAll("\\", "/").match(/\/localization\/([^/]+)\/ui\//);
    const locale = match?.[1] === undefined ? null : canonicalLocale(match[1]);
    if (locale === null) continue;
    const entries = result.get(locale.toLowerCase()) ?? [];
    entries.push(loader);
    result.set(locale.toLowerCase(), entries);
  }
  return result;
}

function unwrapDefault(value: UiMessageModule): unknown {
  return record(value) && "default" in value ? value.default : value;
}

function canonicalLocale(value: string): string | null {
  try {
    return Intl.getCanonicalLocales(value)[0] ?? null;
  } catch {
    return null;
  }
}

function record(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
