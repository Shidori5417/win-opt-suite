import { useCallback, useEffect, useState } from "react";
import {
  CheckCircle2,
  History,
  Languages,
  MonitorCheck,
  Moon,
  Sliders,
  Sun,
} from "lucide-react";

import {
  commands,
  type TransactionId,
  type TransactionSummary_Serialize,
} from "./bindings";
import { ErrorAlert } from "./components/ErrorAlert";
import { RecoveryBanner } from "./components/RecoveryBanner";
import { I18nProvider, useI18n, type Locale } from "./i18n";
import {
  statusLabel,
  toAppError,
  toUnexpectedError,
  type UiError,
} from "./lib/format";
import { HistoryScreen } from "./screens/HistoryScreen";
import { SystemOverviewScreen } from "./screens/SystemOverviewScreen";
import { TweakCatalogScreen } from "./screens/TweakCatalogScreen";

type Screen = "overview" | "catalog" | "history";
type Theme = "light" | "dark";

const THEME_STORAGE_KEY = "winoptsuite.theme";

function readInitialTheme(): Theme {
  try {
    const saved = window.localStorage.getItem(THEME_STORAGE_KEY);
    if (saved === "light" || saved === "dark") {
      return saved;
    }
  } catch {
    // Ignore storage access failures.
  }
  if (
    typeof window !== "undefined" &&
    typeof window.matchMedia === "function" &&
    window.matchMedia("(prefers-color-scheme: dark)").matches
  ) {
    return "dark";
  }
  return "light";
}

function AppShell() {
  const { locale, setLocale, t } = useI18n();
  const [screen, setScreen] = useState<Screen>("overview");
  const [theme, setTheme] = useState<Theme>(readInitialTheme);

  const [incomplete, setIncomplete] = useState<TransactionSummary_Serialize[]>(
    [],
  );
  const [recoveringId, setRecoveringId] = useState<TransactionId | null>(null);
  const [globalError, setGlobalError] = useState<UiError | null>(null);
  const [globalNotice, setGlobalNotice] = useState<string | null>(null);
  const [historyRefreshToken, setHistoryRefreshToken] = useState<number>(0);

  useEffect(() => {
    document.documentElement.classList.toggle("dark", theme === "dark");
    try {
      window.localStorage.setItem(THEME_STORAGE_KEY, theme);
    } catch {
      // Ignore storage access failures.
    }
  }, [theme]);

  const loadIncomplete = useCallback(async () => {
    try {
      const res = await commands.incompleteTransactions();
      if (res.status === "ok") {
        setIncomplete(res.data);
      } else {
        setGlobalError(toAppError(res.error));
      }
    } catch (err) {
      setGlobalError(toUnexpectedError(err));
    }
  }, []);

  useEffect(() => {
    void loadIncomplete();
  }, [loadIncomplete]);

  const handleRecover = useCallback(
    async (id: TransactionId) => {
      setRecoveringId(id);
      setGlobalError(null);
      setGlobalNotice(null);
      try {
        const res = await commands.recover(id);
        if (res.status === "ok") {
          setGlobalNotice(
            t("recovery.success", {
              id,
              status: statusLabel(res.data, t),
            }),
          );
          await loadIncomplete();
          setHistoryRefreshToken((n) => n + 1);
        } else {
          setGlobalError(toAppError(res.error));
        }
      } catch (err) {
        setGlobalError(toUnexpectedError(err));
      } finally {
        setRecoveringId(null);
      }
    },
    [loadIncomplete, t],
  );

  const handleApplied = useCallback(
    (txId: TransactionId) => {
      setGlobalNotice(t("diff.appliedSuccess", { id: txId }));
      setHistoryRefreshToken((n) => n + 1);
      setScreen("history");
    },
    [t],
  );

  const toggleTheme = useCallback(() => {
    setTheme((prev) => (prev === "dark" ? "light" : "dark"));
  }, []);

  const toggleLocale = useCallback(() => {
    const next: Locale = locale === "en" ? "tr" : "en";
    setLocale(next);
  }, [locale, setLocale]);

  const navItems: readonly {
    id: Screen;
    label: string;
    icon: typeof MonitorCheck;
  }[] = [
    { id: "overview", label: t("nav.overview"), icon: MonitorCheck },
    { id: "catalog", label: t("nav.catalog"), icon: Sliders },
    { id: "history", label: t("nav.history"), icon: History },
  ];

  return (
    <div className="flex min-h-screen flex-col bg-slate-50 text-slate-900 dark:bg-slate-950 dark:text-slate-100 lg:flex-row">
      <a
        href="#main-content"
        className="sr-only focus:not-sr-only focus:fixed focus:top-3 focus:left-3 focus:z-50 focus:rounded-lg focus:bg-sky-600 focus:px-4 focus:py-2 focus:text-sm focus:font-semibold focus:text-white"
      >
        {t("app.skipToMain")}
      </a>

      <aside className="flex shrink-0 flex-col border-b border-slate-200 bg-white dark:border-slate-800 dark:bg-slate-900 lg:w-64 lg:border-r lg:border-b-0">
        <div className="border-b border-slate-200 px-5 py-4 dark:border-slate-800">
          <div className="flex items-center gap-2.5">
            <span
              aria-hidden="true"
              className="grid h-8 w-8 grid-cols-2 gap-0.5 rounded-lg bg-slate-900 p-1.5 dark:bg-sky-600"
            >
              <span className="rounded-[2px] bg-slate-100" />
              <span className="rounded-[2px] bg-cyan-400" />
              <span className="rounded-[2px] bg-slate-100" />
              <span className="rounded-[2px] bg-slate-100" />
            </span>
            <div>
              <p className="text-base font-bold tracking-tight text-slate-900 dark:text-slate-50">
                {t("app.name")}
              </p>
              <p className="text-[11px] text-slate-500 dark:text-slate-400">
                {t("app.tagline")}
              </p>
            </div>
          </div>
        </div>

        <nav
          aria-label={t("nav.ariaLabel")}
          className="flex flex-row gap-1 overflow-x-auto p-3 lg:flex-1 lg:flex-col"
        >
          {navItems.map((item) => {
            const Icon = item.icon;
            const active = screen === item.id;
            return (
              <button
                key={item.id}
                type="button"
                aria-current={active ? "page" : undefined}
                onClick={() => setScreen(item.id)}
                className={`flex items-center gap-2.5 whitespace-nowrap rounded-lg px-3.5 py-2.5 text-sm font-medium transition ${
                  active
                    ? "bg-sky-50 text-sky-700 ring-1 ring-sky-200 dark:bg-sky-950/70 dark:text-sky-300 dark:ring-sky-900"
                    : "text-slate-700 hover:bg-slate-100 dark:text-slate-300 dark:hover:bg-slate-800"
                }`}
              >
                <Icon className="h-4 w-4 shrink-0" aria-hidden="true" />
                <span>{item.label}</span>
              </button>
            );
          })}
        </nav>

        <div className="flex items-center justify-between gap-2 border-t border-slate-200 p-3 dark:border-slate-800">
          <button
            type="button"
            onClick={toggleLocale}
            aria-label={t("lang.toggleAria")}
            className="inline-flex flex-1 items-center justify-center gap-1.5 rounded-lg border border-slate-200 bg-slate-50 px-3 py-1.5 text-xs font-semibold text-slate-700 transition hover:bg-slate-100 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-200 dark:hover:bg-slate-700"
          >
            <Languages className="h-3.5 w-3.5" aria-hidden="true" />
            <span>{locale.toUpperCase()}</span>
          </button>

          <button
            type="button"
            onClick={toggleTheme}
            aria-label={t("theme.toggleAria")}
            className="inline-flex flex-1 items-center justify-center gap-1.5 rounded-lg border border-slate-200 bg-slate-50 px-3 py-1.5 text-xs font-semibold text-slate-700 transition hover:bg-slate-100 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-200 dark:hover:bg-slate-700"
          >
            {theme === "dark" ? (
              <>
                <Sun className="h-3.5 w-3.5" aria-hidden="true" />
                <span>{t("theme.light")}</span>
              </>
            ) : (
              <>
                <Moon className="h-3.5 w-3.5" aria-hidden="true" />
                <span>{t("theme.dark")}</span>
              </>
            )}
          </button>
        </div>
      </aside>

      <main
        id="main-content"
        className="min-w-0 flex-1 overflow-y-auto px-5 py-6 lg:px-8"
      >
        <div className="mx-auto max-w-6xl space-y-6">
          <RecoveryBanner
            incomplete={incomplete}
            recoveringId={recoveringId}
            onRecover={(id) => void handleRecover(id)}
          />

          {globalNotice ? (
            <div
              role="status"
              className="flex items-center justify-between gap-3 rounded-lg border border-emerald-300 bg-emerald-50 px-4 py-3 text-sm font-medium text-emerald-900 dark:border-emerald-800 dark:bg-emerald-950/50 dark:text-emerald-200"
            >
              <span className="flex items-center gap-2">
                <CheckCircle2 className="h-4 w-4 shrink-0" aria-hidden="true" />
                {globalNotice}
              </span>
              <button
                type="button"
                onClick={() => setGlobalNotice(null)}
                className="text-xs underline hover:no-underline"
              >
                {t("diff.close")}
              </button>
            </div>
          ) : null}

          {globalError ? (
            <ErrorAlert
              error={globalError}
              onDismiss={() => setGlobalError(null)}
            />
          ) : null}

          {screen === "overview" ? <SystemOverviewScreen /> : null}
          {screen === "catalog" ? (
            <TweakCatalogScreen
              recoveryPending={incomplete.length > 0}
              onApplied={handleApplied}
            />
          ) : null}
          {screen === "history" ? (
            <HistoryScreen
              refreshToken={historyRefreshToken}
              onRecovered={() => void loadIncomplete()}
            />
          ) : null}
        </div>
      </main>
    </div>
  );
}

export function App({ initialLocale }: { initialLocale?: Locale } = {}) {
  return (
    <I18nProvider initialLocale={initialLocale}>
      <AppShell />
    </I18nProvider>
  );
}

export default App;
