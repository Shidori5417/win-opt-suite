import { useCallback, useEffect, useState } from "react";
import {
  Activity,
  AlertTriangle,
  CheckCircle2,
  Cpu,
  HardDrive,
  Laptop,
  Monitor,
  RefreshCw,
  ShieldAlert,
  ShieldCheck,
} from "lucide-react";

import {
  commands,
  type DriftReport_Serialize,
  type SystemFacts,
} from "../bindings";
import { ErrorAlert } from "../components/ErrorAlert";
import { useI18n } from "../i18n";
import {
  antiCheatLabel,
  deriveEnvironmentTags,
  envTagLabel,
  formatAppError,
  formatBytes,
  formatGpuVendor,
  formatResourceState,
  providerLabel,
  toAppError,
  toUnexpectedError,
  type UiError,
} from "../lib/format";

export function SystemOverviewScreen() {
  const { t } = useI18n();
  const [facts, setFacts] = useState<SystemFacts | null>(null);
  const [factsLoading, setFactsLoading] = useState<boolean>(true);
  const [factsError, setFactsError] = useState<UiError | null>(null);

  const [drift, setDrift] = useState<DriftReport_Serialize | null>(null);
  const [driftLoading, setDriftLoading] = useState<boolean>(false);
  const [driftError, setDriftError] = useState<UiError | null>(null);

  const loadFacts = useCallback(async () => {
    setFactsLoading(true);
    setFactsError(null);
    try {
      const res = await commands.systemFacts();
      if (res.status === "ok") {
        setFacts(res.data);
      } else {
        setFacts(null);
        setFactsError(toAppError(res.error));
      }
    } catch (err) {
      setFacts(null);
      setFactsError(toUnexpectedError(err));
    } finally {
      setFactsLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadFacts();
  }, [loadFacts]);

  const runDriftScan = useCallback(async () => {
    setDriftLoading(true);
    setDriftError(null);
    try {
      const res = await commands.detectDrift();
      if (res.status === "ok") {
        setDrift(res.data);
      } else {
        setDriftError(toAppError(res.error));
      }
    } catch (err) {
      setDriftError(toUnexpectedError(err));
    } finally {
      setDriftLoading(false);
    }
  }, []);

  const envTags = facts ? deriveEnvironmentTags(facts) : [];

  return (
    <div className="space-y-6">
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div>
          <h1 className="text-2xl font-bold tracking-tight text-slate-900 dark:text-slate-50">
            {t("overview.title")}
          </h1>
          <p className="mt-1 text-sm text-slate-600 dark:text-slate-400">
            {t("overview.subtitle")}
          </p>
        </div>
        <button
          type="button"
          onClick={() => void loadFacts()}
          disabled={factsLoading}
          className="inline-flex items-center gap-2 rounded-lg border border-slate-300 bg-white px-3.5 py-2 text-sm font-medium text-slate-700 shadow-xs transition hover:bg-slate-50 disabled:cursor-not-allowed disabled:opacity-60 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-200 dark:hover:bg-slate-800"
        >
          <RefreshCw
            className={`h-4 w-4 ${factsLoading ? "animate-spin" : ""}`}
            aria-hidden="true"
          />
          {t("overview.refresh")}
        </button>
      </div>

      {factsError ? (
        <ErrorAlert error={factsError} onDismiss={() => setFactsError(null)} />
      ) : null}

      {factsLoading && !facts ? (
        <div
          role="status"
          className="rounded-xl border border-slate-200 bg-white p-6 text-sm text-slate-600 dark:border-slate-800 dark:bg-slate-900 dark:text-slate-400"
        >
          {t("overview.loading")}
        </div>
      ) : null}

      {facts ? (
        <>
          <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-4">
            <article className="rounded-xl border border-slate-200 bg-white p-4 shadow-xs dark:border-slate-800 dark:bg-slate-900">
              <div className="flex items-center gap-2 text-xs font-medium uppercase tracking-wide text-slate-500 dark:text-slate-400">
                <Monitor className="h-4 w-4 text-sky-600 dark:text-sky-400" aria-hidden="true" />
                {t("overview.osCard")}
              </div>
              <p className="mt-2 text-base font-semibold text-slate-900 dark:text-slate-50">
                Windows 11 {facts.windows_display_version}
              </p>
              <p className="mt-0.5 text-sm text-slate-600 dark:text-slate-300">
                {facts.edition}
              </p>
              <p className="mt-1 font-mono text-xs text-slate-500 dark:text-slate-400">
                {t("overview.osBuild", {
                  build: facts.windows_build,
                  ubr: facts.windows_ubr,
                })}
              </p>
            </article>

            <article className="rounded-xl border border-slate-200 bg-white p-4 shadow-xs dark:border-slate-800 dark:bg-slate-900">
              <div className="flex items-center gap-2 text-xs font-medium uppercase tracking-wide text-slate-500 dark:text-slate-400">
                <Cpu className="h-4 w-4 text-indigo-600 dark:text-indigo-400" aria-hidden="true" />
                {t("overview.cpuCard")}
              </div>
              <p className="mt-2 text-sm font-semibold text-slate-900 dark:text-slate-50">
                {facts.cpu.brand || "—"}
              </p>
              <p className="mt-1 text-xs text-slate-600 dark:text-slate-400">
                {t("overview.cpuCores", {
                  physical: facts.cpu.physical_cores,
                  logical: facts.cpu.logical_cores,
                })}
              </p>
            </article>

            <article className="rounded-xl border border-slate-200 bg-white p-4 shadow-xs dark:border-slate-800 dark:bg-slate-900">
              <div className="flex items-center gap-2 text-xs font-medium uppercase tracking-wide text-slate-500 dark:text-slate-400">
                <HardDrive className="h-4 w-4 text-emerald-600 dark:text-emerald-400" aria-hidden="true" />
                {t("overview.memoryCard")}
              </div>
              <p className="mt-2 text-2xl font-bold text-slate-900 dark:text-slate-50">
                {formatBytes(facts.total_memory_bytes)}
              </p>
            </article>

            <article className="rounded-xl border border-slate-200 bg-white p-4 shadow-xs dark:border-slate-800 dark:bg-slate-900">
              <div className="flex items-center gap-2 text-xs font-medium uppercase tracking-wide text-slate-500 dark:text-slate-400">
                <Laptop className="h-4 w-4 text-violet-600 dark:text-violet-400" aria-hidden="true" />
                {t("overview.formFactor")}
              </div>
              <ul className="mt-2 space-y-1 text-sm text-slate-700 dark:text-slate-200">
                <li>
                  {facts.is_laptop
                    ? t("overview.laptop")
                    : t("overview.desktop")}{" "}
                  ·{" "}
                  {facts.on_battery
                    ? t("overview.onBattery")
                    : t("overview.onAc")}
                </li>
                <li>
                  {facts.is_virtual_machine
                    ? t("overview.virtualMachine")
                    : t("overview.physicalMachine")}
                </li>
                <li>
                  {facts.is_domain_joined
                    ? t("overview.domainJoined")
                    : t("overview.workgroup")}
                </li>
              </ul>
            </article>
          </div>

          <div className="grid gap-4 lg:grid-cols-2">
            <section className="rounded-xl border border-slate-200 bg-white p-5 shadow-xs dark:border-slate-800 dark:bg-slate-900">
              <h2 className="text-sm font-semibold text-slate-900 dark:text-slate-100">
                {t("overview.gpuCard")}
              </h2>
              {facts.gpus.length === 0 ? (
                <p className="mt-2 text-sm text-slate-500 dark:text-slate-400">
                  {t("overview.noGpu")}
                </p>
              ) : (
                <ul className="mt-3 space-y-2.5">
                  {facts.gpus.map((gpu, idx) => (
                    <li
                      key={`${gpu.name}-${gpu.vendor_id}-${idx}`}
                      className="rounded-lg border border-slate-100 bg-slate-50 px-3.5 py-2.5 dark:border-slate-800 dark:bg-slate-950/60"
                    >
                      <p className="text-sm font-medium text-slate-900 dark:text-slate-100">
                        {gpu.name}
                      </p>
                      <p className="mt-0.5 text-xs text-slate-600 dark:text-slate-400">
                        {t("overview.gpuVram", {
                          vram: formatBytes(gpu.dedicated_memory_bytes),
                          vendor: formatGpuVendor(gpu.vendor_id),
                        })}
                      </p>
                    </li>
                  ))}
                </ul>
              )}
            </section>

            <section className="space-y-4 rounded-xl border border-slate-200 bg-white p-5 shadow-xs dark:border-slate-800 dark:bg-slate-900">
              <div>
                <h2 className="text-sm font-semibold text-slate-900 dark:text-slate-100">
                  {t("overview.envTagsTitle")}
                </h2>
                <div className="mt-2.5 flex flex-wrap gap-2">
                  {envTags.map((tag) => (
                    <span
                      key={tag}
                      className="inline-flex items-center rounded-full border border-slate-200 bg-slate-100 px-2.5 py-0.5 text-xs font-medium text-slate-700 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-200"
                    >
                      {envTagLabel(tag, t)}
                    </span>
                  ))}
                </div>
              </div>

              {facts.anti_cheat.length > 0 ? (
                <div
                  role="alert"
                  className="rounded-lg border border-amber-300 bg-amber-50 p-3.5 text-amber-950 dark:border-amber-800/80 dark:bg-amber-950/40 dark:text-amber-100"
                >
                  <div className="flex items-center gap-2 text-sm font-semibold">
                    <ShieldAlert
                      className="h-4 w-4 text-amber-600 dark:text-amber-400"
                      aria-hidden="true"
                    />
                    {t("overview.antiCheatTitle")}
                  </div>
                  <p className="mt-1 text-xs text-amber-900/90 dark:text-amber-200/90">
                    {t("overview.antiCheatBody")}
                  </p>
                  <ul className="mt-2 flex flex-wrap gap-1.5">
                    {facts.anti_cheat.map((driver) => (
                      <li
                        key={driver}
                        className="rounded bg-amber-200/80 px-2 py-0.5 text-xs font-semibold text-amber-950 dark:bg-amber-900/70 dark:text-amber-100"
                      >
                        {antiCheatLabel(driver, t)}
                      </li>
                    ))}
                  </ul>
                </div>
              ) : (
                <div className="flex items-center gap-2 rounded-lg border border-emerald-200 bg-emerald-50/70 px-3.5 py-2.5 text-xs font-medium text-emerald-900 dark:border-emerald-900/70 dark:bg-emerald-950/30 dark:text-emerald-200">
                  <ShieldCheck className="h-4 w-4 shrink-0" aria-hidden="true" />
                  {t("overview.antiCheatClean")}
                </div>
              )}
            </section>
          </div>
        </>
      ) : null}

      <section
        aria-labelledby="drift-heading"
        className="rounded-xl border border-slate-200 bg-white p-5 shadow-xs dark:border-slate-800 dark:bg-slate-900"
      >
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div>
            <h2
              id="drift-heading"
              className="flex items-center gap-2 text-base font-semibold text-slate-900 dark:text-slate-100"
            >
              <Activity className="h-4 w-4 text-sky-600 dark:text-sky-400" aria-hidden="true" />
              {t("drift.title")}
            </h2>
            <p className="mt-0.5 text-sm text-slate-600 dark:text-slate-400">
              {t("drift.subtitle")}
            </p>
          </div>
          <button
            type="button"
            onClick={() => void runDriftScan()}
            disabled={driftLoading}
            className="inline-flex items-center gap-2 rounded-lg bg-sky-600 px-4 py-2 text-sm font-semibold text-white transition hover:bg-sky-700 disabled:cursor-not-allowed disabled:opacity-60"
          >
            <RefreshCw
              className={`h-4 w-4 ${driftLoading ? "animate-spin" : ""}`}
              aria-hidden="true"
            />
            {driftLoading ? t("drift.scanning") : t("drift.scanAction")}
          </button>
        </div>

        {driftError ? (
          <div className="mt-4">
            <ErrorAlert error={driftError} onDismiss={() => setDriftError(null)} />
          </div>
        ) : null}

        {!drift && !driftLoading ? (
          <p className="mt-4 text-sm text-slate-500 dark:text-slate-400">
            {t("drift.notRun")}
          </p>
        ) : null}

        {drift ? (
          <div className="mt-4 space-y-4">
            <div className="grid grid-cols-3 gap-3 sm:max-w-md">
              <div className="rounded-lg border border-slate-200 bg-slate-50 px-3 py-2 dark:border-slate-800 dark:bg-slate-950/60">
                <p className="text-xs text-slate-500 dark:text-slate-400">
                  {t("drift.checked")}
                </p>
                <p className="text-lg font-bold text-slate-900 dark:text-slate-100">
                  {drift.checked}
                </p>
              </div>
              <div className="rounded-lg border border-slate-200 bg-slate-50 px-3 py-2 dark:border-slate-800 dark:bg-slate-950/60">
                <p className="text-xs text-slate-500 dark:text-slate-400">
                  {t("drift.drifted")}
                </p>
                <p
                  className={`text-lg font-bold ${
                    drift.drifted.length > 0
                      ? "text-amber-600 dark:text-amber-400"
                      : "text-slate-900 dark:text-slate-100"
                  }`}
                >
                  {drift.drifted.length}
                </p>
              </div>
              <div className="rounded-lg border border-slate-200 bg-slate-50 px-3 py-2 dark:border-slate-800 dark:bg-slate-950/60">
                <p className="text-xs text-slate-500 dark:text-slate-400">
                  {t("drift.errors")}
                </p>
                <p
                  className={`text-lg font-bold ${
                    drift.errors.length > 0
                      ? "text-rose-600 dark:text-rose-400"
                      : "text-slate-900 dark:text-slate-100"
                  }`}
                >
                  {drift.errors.length}
                </p>
              </div>
            </div>

            {drift.checked === 0 &&
            drift.drifted.length === 0 &&
            drift.errors.length === 0 ? (
              <p className="text-sm text-slate-600 dark:text-slate-400">
                {t("drift.emptyBaseline")}
              </p>
            ) : drift.drifted.length === 0 && drift.errors.length === 0 ? (
              <div className="flex items-center gap-2 rounded-lg border border-emerald-200 bg-emerald-50 px-3.5 py-2.5 text-sm font-medium text-emerald-900 dark:border-emerald-900/70 dark:bg-emerald-950/40 dark:text-emerald-200">
                <CheckCircle2 className="h-4 w-4 shrink-0" aria-hidden="true" />
                {t("drift.cleanBadge")}
              </div>
            ) : null}

            {drift.drifted.length > 0 ? (
              <div className="overflow-x-auto rounded-lg border border-slate-200 dark:border-slate-800">
                <table className="min-w-full divide-y divide-slate-200 text-left text-xs dark:divide-slate-800">
                  <thead className="bg-slate-50 text-slate-600 dark:bg-slate-950/70 dark:text-slate-300">
                    <tr>
                      <th scope="col" className="px-3 py-2 font-semibold">
                        {t("drift.colTweak")}
                      </th>
                      <th scope="col" className="px-3 py-2 font-semibold">
                        {t("drift.colProvider")}
                      </th>
                      <th scope="col" className="px-3 py-2 font-semibold">
                        {t("drift.colResource")}
                      </th>
                      <th scope="col" className="px-3 py-2 font-semibold">
                        {t("drift.colExpected")}
                      </th>
                      <th scope="col" className="px-3 py-2 font-semibold">
                        {t("drift.colActual")}
                      </th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-slate-200 bg-white dark:divide-slate-800 dark:bg-slate-900">
                    {drift.drifted.map((entry) => (
                      <tr key={`${entry.transaction_id}-${entry.resource}`}>
                        <td className="whitespace-nowrap px-3 py-2 font-mono text-slate-900 dark:text-slate-100">
                          {entry.tweak_id}
                        </td>
                        <td className="whitespace-nowrap px-3 py-2 text-slate-700 dark:text-slate-300">
                          {providerLabel(entry.provider, t)}
                        </td>
                        <td className="break-all px-3 py-2 font-mono text-slate-700 dark:text-slate-300">
                          {entry.resource}
                        </td>
                        <td className="px-3 py-2 font-mono text-emerald-700 dark:text-emerald-400">
                          {formatResourceState(entry.expected, t)}
                        </td>
                        <td className="px-3 py-2 font-mono text-amber-700 dark:text-amber-400">
                          {formatResourceState(entry.actual, t)}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            ) : null}

            {drift.errors.length > 0 ? (
              <div className="space-y-2 rounded-lg border border-rose-200 bg-rose-50/70 p-3.5 dark:border-rose-900/70 dark:bg-rose-950/30">
                <h3 className="flex items-center gap-1.5 text-xs font-semibold text-rose-900 dark:text-rose-200">
                  <AlertTriangle className="h-3.5 w-3.5" aria-hidden="true" />
                  {t("drift.errorsHeading")}
                </h3>
                <ul className="space-y-1 text-xs text-rose-800 dark:text-rose-200">
                  {drift.errors.map((item) => (
                    <li key={item.resource} className="break-all">
                      <span className="font-mono font-semibold">
                        {item.resource}
                      </span>
                      : {formatAppError(item.error, t)}
                    </li>
                  ))}
                </ul>
              </div>
            ) : null}
          </div>
        ) : null}
      </section>
    </div>
  );
}
