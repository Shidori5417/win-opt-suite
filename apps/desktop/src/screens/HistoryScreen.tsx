import { useCallback, useEffect, useRef, useState } from "react";
import {
  BarChart3,
  CheckCircle2,
  RefreshCw,
  RotateCcw,
} from "lucide-react";

import {
  commands,
  type MetricSample,
  type TransactionId,
  type TransactionStatus,
  type TransactionSummary_Serialize,
} from "../bindings";
import { ErrorAlert } from "../components/ErrorAlert";
import { useI18n } from "../i18n";
import {
  formatMetricDelta,
  formatMetricValue,
  formatTimestamp,
  metricKindLabel,
  pairMetrics,
  profileLabel,
  statusLabel,
  toAppError,
  toUnexpectedError,
  type UiError,
} from "../lib/format";

interface HistoryScreenProps {
  refreshToken: number;
  onRecovered: () => void;
}

function statusBadgeClass(status: TransactionStatus): string {
  switch (status) {
    case "committed":
      return "bg-emerald-100 text-emerald-800 dark:bg-emerald-950 dark:text-emerald-300";
    case "rolled_back":
      return "bg-slate-200 text-slate-800 dark:bg-slate-800 dark:text-slate-300";
    case "failed":
      return "bg-rose-100 text-rose-800 dark:bg-rose-950 dark:text-rose-300";
    case "pending":
    case "applying":
    case "rolling_back":
      return "bg-amber-100 text-amber-800 dark:bg-amber-950 dark:text-amber-300";
  }
}

function isRecoverable(status: TransactionStatus): boolean {
  return (
    status === "pending" ||
    status === "applying" ||
    status === "rolling_back" ||
    status === "failed"
  );
}

export function HistoryScreen({
  refreshToken,
  onRecovered,
}: HistoryScreenProps) {
  const { locale, t } = useI18n();
  const [items, setItems] = useState<TransactionSummary_Serialize[]>([]);
  const [loading, setLoading] = useState<boolean>(true);
  const [error, setError] = useState<UiError | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const [confirmRollbackId, setConfirmRollbackId] =
    useState<TransactionId | null>(null);
  const [busyId, setBusyId] = useState<TransactionId | null>(null);

  const [expandedMetricsId, setExpandedMetricsId] =
    useState<TransactionId | null>(null);
  const [metricsByTx, setMetricsByTx] = useState<
    Record<string, MetricSample[]>
  >({});
  const [metricsLoadingId, setMetricsLoadingId] =
    useState<TransactionId | null>(null);

  const cancelRollbackBtnRef = useRef<HTMLButtonElement | null>(null);

  const loadHistory = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const res = await commands.history();
      if (res.status === "ok") {
        setItems(res.data);
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
    void loadHistory();
  }, [loadHistory, refreshToken]);

  useEffect(() => {
    if (!confirmRollbackId) {
      return;
    }
    cancelRollbackBtnRef.current?.focus();
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && busyId === null) {
        setConfirmRollbackId(null);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [busyId, confirmRollbackId]);

  const executeRollback = useCallback(
    async (id: TransactionId) => {
      setBusyId(id);
      setError(null);
      setNotice(null);
      try {
        const res = await commands.rollback(id);
        if (res.status === "ok") {
          setConfirmRollbackId(null);
          setNotice(t("history.rollbackSuccess", { id }));
          await loadHistory();
        } else {
          setConfirmRollbackId(null);
          setError(toAppError(res.error));
        }
      } catch (err) {
        setConfirmRollbackId(null);
        setError(toUnexpectedError(err));
      } finally {
        setBusyId(null);
      }
    },
    [loadHistory, t],
  );

  const executeRecover = useCallback(
    async (id: TransactionId) => {
      setBusyId(id);
      setError(null);
      setNotice(null);
      try {
        const res = await commands.recover(id);
        if (res.status === "ok") {
          setNotice(
            t("recovery.success", {
              id,
              status: statusLabel(res.data, t),
            }),
          );
          await loadHistory();
          onRecovered();
        } else {
          setError(toAppError(res.error));
        }
      } catch (err) {
        setError(toUnexpectedError(err));
      } finally {
        setBusyId(null);
      }
    },
    [loadHistory, onRecovered, t],
  );

  const toggleMetrics = useCallback(
    async (id: TransactionId) => {
      if (expandedMetricsId === id) {
        setExpandedMetricsId(null);
        return;
      }
      setExpandedMetricsId(id);
      if (metricsByTx[id] !== undefined) {
        return;
      }
      setMetricsLoadingId(id);
      setError(null);
      try {
        const res = await commands.transactionMetrics(id);
        if (res.status === "ok") {
          setMetricsByTx((prev) => ({ ...prev, [id]: res.data }));
        } else {
          setError(toAppError(res.error));
        }
      } catch (err) {
        setError(toUnexpectedError(err));
      } finally {
        setMetricsLoadingId(null);
      }
    },
    [expandedMetricsId, metricsByTx],
  );

  return (
    <div className="space-y-6">
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div>
          <h1 className="text-2xl font-bold tracking-tight text-slate-900 dark:text-slate-50">
            {t("history.title")}
          </h1>
          <p className="mt-1 text-sm text-slate-600 dark:text-slate-400">
            {t("history.subtitle")}
          </p>
        </div>
        <button
          type="button"
          onClick={() => void loadHistory()}
          disabled={loading}
          className="inline-flex items-center gap-2 rounded-lg border border-slate-300 bg-white px-3.5 py-2 text-sm font-medium text-slate-700 shadow-xs transition hover:bg-slate-50 disabled:opacity-60 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-200 dark:hover:bg-slate-800"
        >
          <RefreshCw
            className={`h-4 w-4 ${loading ? "animate-spin" : ""}`}
            aria-hidden="true"
          />
          {t("history.refresh")}
        </button>
      </div>

      {notice ? (
        <div
          role="status"
          className="flex items-center gap-2 rounded-lg border border-emerald-300 bg-emerald-50 px-4 py-3 text-sm font-medium text-emerald-900 dark:border-emerald-800 dark:bg-emerald-950/50 dark:text-emerald-200"
        >
          <CheckCircle2 className="h-4 w-4 shrink-0" aria-hidden="true" />
          <span>{notice}</span>
        </div>
      ) : null}

      {error ? (
        <ErrorAlert error={error} onDismiss={() => setError(null)} />
      ) : null}

      {loading ? (
        <div
          role="status"
          className="rounded-xl border border-slate-200 bg-white p-6 text-sm text-slate-600 dark:border-slate-800 dark:bg-slate-900 dark:text-slate-400"
        >
          {t("history.loading")}
        </div>
      ) : items.length === 0 ? (
        <div className="rounded-xl border border-slate-200 bg-white p-6 text-sm text-slate-500 dark:border-slate-800 dark:bg-slate-900 dark:text-slate-400">
          {t("history.empty")}
        </div>
      ) : (
        <ul className="space-y-3">
          {items.map((tx) => {
            const isExpanded = expandedMetricsId === tx.id;
            const samples = metricsByTx[tx.id];
            const metricRows = samples ? pairMetrics(samples) : [];
            const isBusy = busyId === tx.id;

            return (
              <li
                key={tx.id}
                className="rounded-xl border border-slate-200 bg-white p-5 shadow-xs dark:border-slate-800 dark:bg-slate-900"
              >
                <div className="flex flex-wrap items-start justify-between gap-4">
                  <div className="min-w-0 space-y-1.5">
                    <div className="flex flex-wrap items-center gap-2">
                      <span className="font-mono text-sm font-semibold text-slate-900 dark:text-slate-100">
                        {tx.id}
                      </span>
                      <span
                        className={`rounded-full px-2.5 py-0.5 text-xs font-semibold ${statusBadgeClass(tx.status)}`}
                      >
                        {statusLabel(tx.status, t)}
                      </span>
                      <span className="rounded-full border border-slate-200 bg-slate-50 px-2.5 py-0.5 text-xs font-medium text-slate-700 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-300">
                        {tx.profile
                          ? profileLabel(tx.profile, t)
                          : t("history.customSelection")}
                      </span>
                    </div>
                    <p className="text-xs text-slate-500 dark:text-slate-400">
                      {formatTimestamp(tx.created_at_unix_ms, locale)} ·{" "}
                      {t("history.steps", { count: tx.step_count })}
                    </p>
                    <div className="flex flex-wrap items-center gap-1.5 pt-1 text-xs">
                      <span className="font-medium text-slate-600 dark:text-slate-400">
                        {t("history.tweaks", { count: tx.tweak_ids.length })}:
                      </span>
                      {tx.tweak_ids.map((tweakId) => (
                        <span
                          key={tweakId}
                          className="rounded bg-slate-100 px-2 py-0.5 font-mono text-[11px] text-slate-700 dark:bg-slate-800 dark:text-slate-300"
                        >
                          {tweakId}
                        </span>
                      ))}
                    </div>
                  </div>

                  <div className="flex flex-wrap items-center gap-2">
                    <button
                      type="button"
                      aria-expanded={isExpanded}
                      onClick={() => void toggleMetrics(tx.id)}
                      className="inline-flex items-center gap-1.5 rounded-lg border border-slate-300 bg-white px-3 py-1.5 text-xs font-semibold text-slate-700 transition hover:bg-slate-50 dark:border-slate-700 dark:bg-slate-950 dark:text-slate-200 dark:hover:bg-slate-800"
                    >
                      <BarChart3 className="h-3.5 w-3.5" aria-hidden="true" />
                      {isExpanded
                        ? t("history.metricsHide")
                        : t("history.metricsShow")}
                    </button>

                    {tx.status === "committed" ? (
                      <button
                        type="button"
                        disabled={busyId !== null}
                        onClick={() => setConfirmRollbackId(tx.id)}
                        className="inline-flex items-center gap-1.5 rounded-lg bg-rose-600 px-3 py-1.5 text-xs font-semibold text-white transition hover:bg-rose-700 disabled:cursor-not-allowed disabled:opacity-50"
                      >
                        <RotateCcw className="h-3.5 w-3.5" aria-hidden="true" />
                        {isBusy
                          ? t("history.rollingBack")
                          : t("history.rollback")}
                      </button>
                    ) : null}

                    {isRecoverable(tx.status) ? (
                      <button
                        type="button"
                        disabled={busyId !== null}
                        onClick={() => void executeRecover(tx.id)}
                        className="inline-flex items-center gap-1.5 rounded-lg bg-amber-600 px-3 py-1.5 text-xs font-semibold text-white transition hover:bg-amber-700 disabled:cursor-not-allowed disabled:opacity-50"
                      >
                        <RotateCcw className="h-3.5 w-3.5" aria-hidden="true" />
                        {t("history.recover")}
                      </button>
                    ) : null}
                  </div>
                </div>

                {isExpanded ? (
                  <div className="mt-4 border-t border-slate-200 pt-4 dark:border-slate-800">
                    {metricsLoadingId === tx.id ? (
                      <p className="text-xs text-slate-500 dark:text-slate-400">
                        {t("history.metricsLoading")}
                      </p>
                    ) : metricRows.length === 0 ? (
                      <p className="text-xs text-slate-500 dark:text-slate-400">
                        {t("history.metricsEmpty")}
                      </p>
                    ) : (
                      <div className="overflow-x-auto rounded-lg border border-slate-200 dark:border-slate-800">
                        <table className="min-w-full divide-y divide-slate-200 text-left text-xs dark:divide-slate-800">
                          <thead className="bg-slate-50 text-slate-600 dark:bg-slate-950/70 dark:text-slate-300">
                            <tr>
                              <th scope="col" className="px-3 py-2 font-semibold">
                                {t("history.metricKind")}
                              </th>
                              <th scope="col" className="px-3 py-2 font-semibold">
                                {t("history.metricBefore")}
                              </th>
                              <th scope="col" className="px-3 py-2 font-semibold">
                                {t("history.metricAfter")}
                              </th>
                              <th scope="col" className="px-3 py-2 font-semibold">
                                {t("history.metricDelta")}
                              </th>
                            </tr>
                          </thead>
                          <tbody className="divide-y divide-slate-200 bg-white dark:divide-slate-800 dark:bg-slate-900">
                            {metricRows.map((row) => {
                              const deltaClass =
                                row.delta === null || row.delta === 0
                                  ? "text-slate-600 dark:text-slate-400"
                                  : row.delta < 0
                                    ? "font-semibold text-emerald-700 dark:text-emerald-400"
                                    : "font-semibold text-amber-700 dark:text-amber-400";
                              return (
                                <tr key={row.kind}>
                                  <td className="px-3 py-2 font-medium text-slate-900 dark:text-slate-100">
                                    {metricKindLabel(row.kind, t)}
                                  </td>
                                  <td className="px-3 py-2 font-mono text-slate-700 dark:text-slate-300">
                                    {formatMetricValue(row.kind, row.before)}
                                  </td>
                                  <td className="px-3 py-2 font-mono text-slate-700 dark:text-slate-300">
                                    {formatMetricValue(row.kind, row.after)}
                                  </td>
                                  <td className={`px-3 py-2 font-mono ${deltaClass}`}>
                                    {formatMetricDelta(row.kind, row.delta)}
                                  </td>
                                </tr>
                              );
                            })}
                          </tbody>
                        </table>
                      </div>
                    )}
                  </div>
                ) : null}
              </li>
            );
          })}
        </ul>
      )}

      {confirmRollbackId ? (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-slate-950/65 p-4 backdrop-blur-xs"
          role="presentation"
          onClick={(event) => {
            if (event.target === event.currentTarget && busyId === null) {
              setConfirmRollbackId(null);
            }
          }}
        >
          <div
            role="alertdialog"
            aria-modal="true"
            aria-labelledby="rollback-confirm-title"
            aria-describedby="rollback-confirm-desc"
            className="w-full max-w-md rounded-2xl border border-slate-200 bg-white p-6 shadow-2xl dark:border-slate-700 dark:bg-slate-900"
          >
            <h2
              id="rollback-confirm-title"
              className="text-base font-bold text-slate-900 dark:text-slate-50"
            >
              {t("history.rollbackConfirmTitle")}
            </h2>
            <p
              id="rollback-confirm-desc"
              className="mt-2 text-sm text-slate-600 dark:text-slate-300"
            >
              {t("history.rollbackConfirmBody", { id: confirmRollbackId })}
            </p>
            <div className="mt-6 flex justify-end gap-3">
              <button
                ref={cancelRollbackBtnRef}
                type="button"
                disabled={busyId !== null}
                onClick={() => setConfirmRollbackId(null)}
                className="rounded-lg border border-slate-300 bg-white px-4 py-2 text-sm font-medium text-slate-700 hover:bg-slate-50 disabled:opacity-50 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-200 dark:hover:bg-slate-800"
              >
                {t("history.rollbackConfirmCancel")}
              </button>
              <button
                type="button"
                disabled={busyId !== null}
                onClick={() => void executeRollback(confirmRollbackId)}
                className="rounded-lg bg-rose-600 px-4 py-2 text-sm font-semibold text-white hover:bg-rose-700 disabled:opacity-50"
              >
                {busyId === confirmRollbackId
                  ? t("history.rollingBack")
                  : t("history.rollbackConfirmSubmit")}
              </button>
            </div>
          </div>
        </div>
      ) : null}
    </div>
  );
}
