import { useCallback, useEffect, useMemo, useState } from "react";
import {
  AlertTriangle,
  ExternalLink,
  Eye,
  RotateCw,
  Search,
  SlidersHorizontal,
} from "lucide-react";

import {
  commands,
  type DryRunDiff_Serialize,
  type Profile,
  type RiskLevel,
  type Selection,
  type TransactionId,
  type TweakCategory,
  type TweakId,
  type TweakManifest_Serialize,
} from "../bindings";
import { DryRunModal } from "../components/DryRunModal";
import { ErrorAlert } from "../components/ErrorAlert";
import { useI18n } from "../i18n";
import {
  categoryLabel,
  envTagLabel,
  profileLabel,
  riskLabel,
  toAppError,
  toUnexpectedError,
  type UiError,
} from "../lib/format";

const ALL_PROFILES: readonly Profile[] = [
  "safe",
  "privacy",
  "gaming",
  "laptop",
];

const ALL_CATEGORIES: readonly TweakCategory[] = [
  "privacy",
  "telemetry",
  "performance",
  "gaming",
  "power",
  "network",
  "interface",
  "services",
  "updates",
  "security",
];

interface TweakCatalogScreenProps {
  recoveryPending: boolean;
  onApplied: (txId: TransactionId) => void;
}

function riskBadgeClass(risk: RiskLevel): string {
  switch (risk) {
    case "low":
      return "bg-emerald-100 text-emerald-800 dark:bg-emerald-950 dark:text-emerald-300";
    case "medium":
      return "bg-amber-100 text-amber-800 dark:bg-amber-950 dark:text-amber-300";
    case "high":
      return "bg-rose-100 text-rose-800 dark:bg-rose-950 dark:text-rose-300";
  }
}

interface ActivePlan {
  selection: Selection;
  profile: Profile | null;
  diff: DryRunDiff_Serialize;
}

export function TweakCatalogScreen({
  recoveryPending,
  onApplied,
}: TweakCatalogScreenProps) {
  const { t } = useI18n();
  const [tweaks, setTweaks] = useState<TweakManifest_Serialize[]>([]);
  const [loading, setLoading] = useState<boolean>(true);
  const [error, setError] = useState<UiError | null>(null);

  const [search, setSearch] = useState<string>("");
  const [categoryFilter, setCategoryFilter] = useState<TweakCategory | "all">(
    "all",
  );
  const [profileFilter, setProfileFilter] = useState<Profile | "all">("all");
  const [selectedIds, setSelectedIds] = useState<Set<TweakId>>(new Set());
  const [activeProfileSelection, setActiveProfileSelection] =
    useState<Profile | null>(null);

  const [planning, setPlanning] = useState<boolean>(false);
  const [plan, setPlan] = useState<ActivePlan | null>(null);
  const [applying, setApplying] = useState<boolean>(false);
  const [applyError, setApplyError] = useState<UiError | null>(null);

  const loadTweaks = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const res = await commands.listTweaks();
      if (res.status === "ok") {
        setTweaks(res.data);
      } else {
        setError(toAppError(res.error));
      }
    } catch (err) {
      setError(toUnexpectedError(err));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadTweaks();
  }, [loadTweaks]);

  const filteredTweaks = useMemo(() => {
    const query = search.trim().toLowerCase();
    return tweaks.filter((item) => {
      if (categoryFilter !== "all" && item.category !== categoryFilter) {
        return false;
      }
      if (
        profileFilter !== "all" &&
        !item.profiles.includes(profileFilter)
      ) {
        return false;
      }
      if (query.length > 0) {
        const haystack =
          `${item.id} ${item.title} ${item.description}`.toLowerCase();
        if (!haystack.includes(query)) {
          return false;
        }
      }
      return true;
    });
  }, [tweaks, categoryFilter, profileFilter, search]);

  const toggleTweak = useCallback((id: TweakId) => {
    setActiveProfileSelection(null);
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  }, []);

  const selectProfile = useCallback(
    (profile: Profile) => {
      const ids = tweaks
        .filter((item) => item.profiles.includes(profile))
        .map((item) => item.id);
      setSelectedIds(new Set(ids));
      setActiveProfileSelection(profile);
    },
    [tweaks],
  );

  const selectVisible = useCallback(() => {
    setActiveProfileSelection(null);
    setSelectedIds((prev) => {
      const next = new Set(prev);
      for (const item of filteredTweaks) {
        next.add(item.id);
      }
      return next;
    });
  }, [filteredTweaks]);

  const clearSelection = useCallback(() => {
    setActiveProfileSelection(null);
    setSelectedIds(new Set());
  }, []);

  const runDryRun = useCallback(
    async (selection: Selection, profile: Profile | null) => {
      setPlanning(true);
      setError(null);
      setApplyError(null);
      try {
        const res = await commands.dryRun(selection);
        if (res.status === "ok") {
          setPlan({ selection, profile, diff: res.data });
        } else {
          setError(toAppError(res.error));
        }
      } catch (err) {
        setError(toUnexpectedError(err));
      } finally {
        setPlanning(false);
      }
    },
    [],
  );

  const handlePreviewSelection = useCallback(() => {
    if (selectedIds.size === 0) {
      return;
    }
    if (activeProfileSelection !== null) {
      void runDryRun(
        { kind: "profile", profile: activeProfileSelection },
        activeProfileSelection,
      );
      return;
    }
    const orderedIds = tweaks
      .filter((item) => selectedIds.has(item.id))
      .map((item) => item.id);
    void runDryRun({ kind: "tweaks", ids: orderedIds }, null);
  }, [activeProfileSelection, runDryRun, selectedIds, tweaks]);

  const handleConfirmApply = useCallback(async () => {
    if (!plan) {
      return;
    }
    setApplying(true);
    setApplyError(null);
    try {
      const res = await commands.apply(plan.selection, plan.profile);
      if (res.status === "ok") {
        setPlan(null);
        setSelectedIds(new Set());
        setActiveProfileSelection(null);
        onApplied(res.data);
      } else {
        setApplyError(toAppError(res.error));
      }
    } catch (err) {
      setApplyError(toUnexpectedError(err));
    } finally {
      setApplying(false);
    }
  }, [onApplied, plan]);

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-bold tracking-tight text-slate-900 dark:text-slate-50">
          {t("catalog.title")}
        </h1>
        <p className="mt-1 text-sm text-slate-600 dark:text-slate-400">
          {t("catalog.subtitle")}
        </p>
      </div>

      {error ? (
        <ErrorAlert error={error} onDismiss={() => setError(null)} />
      ) : null}

      <section
        aria-label={t("catalog.quickProfileTitle")}
        className="rounded-xl border border-slate-200 bg-white p-4 shadow-xs dark:border-slate-800 dark:bg-slate-900"
      >
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-slate-600 dark:text-slate-400">
            <SlidersHorizontal className="h-4 w-4 text-sky-600 dark:text-sky-400" aria-hidden="true" />
            {t("catalog.quickProfileTitle")}
          </div>
          <div className="flex flex-wrap gap-2">
            {ALL_PROFILES.map((profile) => {
              const count = tweaks.filter((m) =>
                m.profiles.includes(profile),
              ).length;
              const isSelected = activeProfileSelection === profile;
              return (
                <div
                  key={profile}
                  className="inline-flex overflow-hidden rounded-lg border border-slate-200 dark:border-slate-700"
                >
                  <button
                    type="button"
                    onClick={() => selectProfile(profile)}
                    className={`px-3 py-1.5 text-xs font-semibold transition ${
                      isSelected
                        ? "bg-sky-600 text-white"
                        : "bg-slate-50 text-slate-700 hover:bg-slate-100 dark:bg-slate-800 dark:text-slate-200 dark:hover:bg-slate-700"
                    }`}
                  >
                    {t("catalog.selectProfile", {
                      profile: profileLabel(profile, t),
                    })}{" "}
                    ({count})
                  </button>
                  <button
                    type="button"
                    disabled={planning || recoveryPending || count === 0}
                    onClick={() =>
                      void runDryRun({ kind: "profile", profile }, profile)
                    }
                    className="border-l border-slate-200 bg-white px-2.5 py-1.5 text-xs font-medium text-sky-700 transition hover:bg-sky-50 disabled:cursor-not-allowed disabled:opacity-50 dark:border-slate-700 dark:bg-slate-900 dark:text-sky-400 dark:hover:bg-slate-800"
                  >
                    {t("catalog.previewProfile", {
                      profile: profileLabel(profile, t),
                    })}
                  </button>
                </div>
              );
            })}
          </div>
        </div>
      </section>

      <div className="flex flex-wrap items-end gap-3 rounded-xl border border-slate-200 bg-white p-4 shadow-xs dark:border-slate-800 dark:bg-slate-900">
        <div className="min-w-[220px] flex-1">
          <label
            htmlFor="catalog-search"
            className="block text-xs font-medium text-slate-600 dark:text-slate-400"
          >
            {t("catalog.searchLabel")}
          </label>
          <div className="relative mt-1">
            <Search
              className="pointer-events-none absolute top-1/2 left-3 h-4 w-4 -translate-y-1/2 text-slate-400"
              aria-hidden="true"
            />
            <input
              id="catalog-search"
              type="search"
              value={search}
              onChange={(event) => setSearch(event.target.value)}
              placeholder={t("catalog.searchPlaceholder")}
              className="w-full rounded-lg border border-slate-300 bg-white py-2 pr-3 pl-9 text-sm text-slate-900 placeholder:text-slate-400 dark:border-slate-700 dark:bg-slate-950 dark:text-slate-100"
            />
          </div>
        </div>

        <div className="w-44">
          <label
            htmlFor="catalog-category"
            className="block text-xs font-medium text-slate-600 dark:text-slate-400"
          >
            {t("catalog.categoryLabel")}
          </label>
          <select
            id="catalog-category"
            value={categoryFilter}
            onChange={(event) =>
              setCategoryFilter(event.target.value as TweakCategory | "all")
            }
            className="mt-1 w-full rounded-lg border border-slate-300 bg-white px-3 py-2 text-sm text-slate-900 dark:border-slate-700 dark:bg-slate-950 dark:text-slate-100"
          >
            <option value="all">{t("catalog.categoryAll")}</option>
            {ALL_CATEGORIES.map((category) => (
              <option key={category} value={category}>
                {categoryLabel(category, t)}
              </option>
            ))}
          </select>
        </div>

        <div className="w-40">
          <label
            htmlFor="catalog-profile"
            className="block text-xs font-medium text-slate-600 dark:text-slate-400"
          >
            {t("catalog.profileFilterLabel")}
          </label>
          <select
            id="catalog-profile"
            value={profileFilter}
            onChange={(event) =>
              setProfileFilter(event.target.value as Profile | "all")
            }
            className="mt-1 w-full rounded-lg border border-slate-300 bg-white px-3 py-2 text-sm text-slate-900 dark:border-slate-700 dark:bg-slate-950 dark:text-slate-100"
          >
            <option value="all">{t("catalog.profileAll")}</option>
            {ALL_PROFILES.map((profile) => (
              <option key={profile} value={profile}>
                {profileLabel(profile, t)}
              </option>
            ))}
          </select>
        </div>
      </div>

      <div className="flex flex-wrap items-center justify-between gap-3 rounded-xl border border-slate-200 bg-slate-100/80 px-4 py-3 dark:border-slate-800 dark:bg-slate-900/80">
        <div className="flex flex-wrap items-center gap-2 text-sm">
          <span className="font-semibold text-slate-900 dark:text-slate-100">
            {t("catalog.selectedCount", { count: selectedIds.size })}
          </span>
          <button
            type="button"
            onClick={selectVisible}
            disabled={filteredTweaks.length === 0}
            className="rounded-md border border-slate-300 bg-white px-2.5 py-1 text-xs font-medium text-slate-700 hover:bg-slate-50 disabled:opacity-50 dark:border-slate-700 dark:bg-slate-950 dark:text-slate-200 dark:hover:bg-slate-800"
          >
            {t("catalog.selectVisible")}
          </button>
          <button
            type="button"
            onClick={clearSelection}
            disabled={selectedIds.size === 0}
            className="rounded-md border border-slate-300 bg-white px-2.5 py-1 text-xs font-medium text-slate-700 hover:bg-slate-50 disabled:opacity-50 dark:border-slate-700 dark:bg-slate-950 dark:text-slate-200 dark:hover:bg-slate-800"
          >
            {t("catalog.clearSelection")}
          </button>
        </div>

        <button
          type="button"
          disabled={selectedIds.size === 0 || planning || recoveryPending}
          onClick={handlePreviewSelection}
          className="inline-flex items-center gap-2 rounded-lg bg-sky-600 px-4 py-2 text-sm font-semibold text-white transition hover:bg-sky-700 disabled:cursor-not-allowed disabled:opacity-50"
        >
          <Eye className="h-4 w-4" aria-hidden="true" />
          {planning ? t("catalog.previewing") : t("catalog.previewDiff")}
        </button>
      </div>

      {loading ? (
        <div
          role="status"
          className="rounded-xl border border-slate-200 bg-white p-6 text-sm text-slate-600 dark:border-slate-800 dark:bg-slate-900 dark:text-slate-400"
        >
          {t("catalog.loading")}
        </div>
      ) : filteredTweaks.length === 0 ? (
        <div className="rounded-xl border border-slate-200 bg-white p-6 text-sm text-slate-500 dark:border-slate-800 dark:bg-slate-900 dark:text-slate-400">
          {t("catalog.empty")}
        </div>
      ) : (
        <ul className="space-y-3">
          {filteredTweaks.map((manifest) => {
            const checked = selectedIds.has(manifest.id);
            return (
              <li
                key={manifest.id}
                className={`rounded-xl border bg-white p-5 transition dark:bg-slate-900 ${
                  checked
                    ? "border-sky-500 ring-1 ring-sky-500/40"
                    : "border-slate-200 dark:border-slate-800"
                }`}
              >
                <div className="flex items-start gap-3.5">
                  <input
                    id={`tweak-${manifest.id}`}
                    type="checkbox"
                    checked={checked}
                    aria-label={t("catalog.selectAria", {
                      title: manifest.title,
                    })}
                    onChange={() => toggleTweak(manifest.id)}
                    className="mt-1 h-4 w-4 rounded border-slate-300 text-sky-600 focus:ring-sky-500 dark:border-slate-600 dark:bg-slate-950"
                  />
                  <div className="min-w-0 flex-1 space-y-2.5">
                    <div className="flex flex-wrap items-center gap-2">
                      <label
                        htmlFor={`tweak-${manifest.id}`}
                        className="cursor-pointer text-base font-semibold text-slate-900 dark:text-slate-50"
                      >
                        {manifest.title}
                      </label>
                      <span
                        className={`rounded-full px-2.5 py-0.5 text-xs font-semibold ${riskBadgeClass(manifest.risk)}`}
                      >
                        {riskLabel(manifest.risk, t)}
                      </span>
                      <span className="rounded-full bg-slate-100 px-2.5 py-0.5 text-xs font-medium text-slate-700 dark:bg-slate-800 dark:text-slate-300">
                        {categoryLabel(manifest.category, t)}
                      </span>
                      {manifest.requires_reboot ? (
                        <span className="inline-flex items-center gap-1 rounded-full bg-indigo-100 px-2.5 py-0.5 text-xs font-medium text-indigo-800 dark:bg-indigo-950 dark:text-indigo-300">
                          <RotateCw className="h-3 w-3" aria-hidden="true" />
                          {t("catalog.rebootRequired")}
                        </span>
                      ) : null}
                    </div>

                    <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-slate-500 dark:text-slate-400">
                      <span className="font-mono">{manifest.id}</span>
                      <span>
                        {t("catalog.actionsCount", {
                          count: manifest.actions.length,
                        })}
                      </span>
                      <span>
                        {t("catalog.minBuild", {
                          build: manifest.min_windows_build,
                        })}
                      </span>
                      {manifest.profiles.map((p) => (
                        <span
                          key={p}
                          className="rounded border border-sky-200 bg-sky-50 px-1.5 py-0.25 text-[11px] font-medium text-sky-800 dark:border-sky-900 dark:bg-sky-950/60 dark:text-sky-300"
                        >
                          {profileLabel(p, t)}
                        </span>
                      ))}
                    </div>

                    <p className="text-sm text-slate-700 dark:text-slate-300">
                      {manifest.description}
                    </p>

                    <div className="rounded-lg border border-amber-200/80 bg-amber-50/70 px-3 py-2 text-xs text-amber-950 dark:border-amber-900/60 dark:bg-amber-950/30 dark:text-amber-200">
                      <span className="inline-flex items-center gap-1 font-semibold">
                        <AlertTriangle className="h-3.5 w-3.5 shrink-0" aria-hidden="true" />
                        {t("catalog.sideEffects")}:
                      </span>{" "}
                      {manifest.side_effects}
                    </div>

                    {manifest.excluded_on.length > 0 ||
                    manifest.conflicts_with.length > 0 ||
                    manifest.references.length > 0 ? (
                      <div className="flex flex-wrap gap-x-5 gap-y-1.5 pt-0.5 text-xs text-slate-600 dark:text-slate-400">
                        {manifest.excluded_on.length > 0 ? (
                          <div>
                            <span className="font-semibold">
                              {t("catalog.excludedOn")}:
                            </span>{" "}
                            {manifest.excluded_on
                              .map((tag) => envTagLabel(tag, t))
                              .join(", ")}
                          </div>
                        ) : null}

                        {manifest.conflicts_with.length > 0 ? (
                          <div>
                            <span className="font-semibold">
                              {t("catalog.conflictsWith")}:
                            </span>{" "}
                            <span className="font-mono">
                              {manifest.conflicts_with.join(", ")}
                            </span>
                          </div>
                        ) : null}

                        {manifest.references.length > 0 ? (
                          <div className="flex flex-wrap items-center gap-2">
                            <span className="font-semibold">
                              {t("catalog.references")}:
                            </span>
                            {manifest.references.map((href, idx) => (
                              <a
                                key={href}
                                href={href}
                                target="_blank"
                                rel="noopener noreferrer"
                                className="inline-flex items-center gap-1 text-sky-700 underline decoration-sky-400/60 hover:text-sky-800 dark:text-sky-400 dark:hover:text-sky-300"
                              >
                                <span>#{idx + 1}</span>
                                <ExternalLink className="h-3 w-3" aria-hidden="true" />
                              </a>
                            ))}
                          </div>
                        ) : null}
                      </div>
                    ) : null}
                  </div>
                </div>
              </li>
            );
          })}
        </ul>
      )}

      {plan ? (
        <DryRunModal
          diff={plan.diff}
          applying={applying}
          error={applyError}
          onDismissError={() => setApplyError(null)}
          onClose={() => {
            if (!applying) {
              setPlan(null);
              setApplyError(null);
            }
          }}
          onConfirmApply={() => void handleConfirmApply()}
        />
      ) : null}
    </div>
  );
}
