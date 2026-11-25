import { useEffect, useRef } from "react";
import {
  AlertOctagon,
  AlertTriangle,
  Info,
  RotateCw,
  X,
} from "lucide-react";

import type {
  ChangeKind,
  DryRunDiff_Serialize,
  Severity,
} from "../bindings";
import { ErrorAlert } from "./ErrorAlert";
import { useI18n } from "../i18n";
import {
  changeKindLabel,
  formatConflictWarning,
  formatResourceState,
  providerLabel,
  severityLabel,
  type UiError,
} from "../lib/format";

interface DryRunModalProps {
  diff: DryRunDiff_Serialize;
  applying: boolean;
  error: UiError | null;
  onDismissError: () => void;
  onClose: () => void;
  onConfirmApply: () => void;
}

function changeBadgeClass(change: ChangeKind): string {
  switch (change) {
    case "create":
      return "bg-emerald-100 text-emerald-800 dark:bg-emerald-950 dark:text-emerald-300";
    case "modify":
      return "bg-sky-100 text-sky-800 dark:bg-sky-950 dark:text-sky-300";
    case "delete":
      return "bg-rose-100 text-rose-800 dark:bg-rose-950 dark:text-rose-300";
    case "no_change":
      return "bg-slate-100 text-slate-600 dark:bg-slate-800 dark:text-slate-400";
  }
}

function severityStyle(severity: Severity): string {
  switch (severity) {
    case "blocking":
      return "border-rose-300 bg-rose-50 text-rose-900 dark:border-rose-800 dark:bg-rose-950/60 dark:text-rose-100";
    case "warning":
      return "border-amber-300 bg-amber-50 text-amber-900 dark:border-amber-800 dark:bg-amber-950/60 dark:text-amber-100";
    case "info":
      return "border-sky-300 bg-sky-50 text-sky-900 dark:border-sky-800 dark:bg-sky-950/60 dark:text-sky-100";
  }
}

export function DryRunModal({
  diff,
  applying,
  error,
  onDismissError,
  onClose,
  onConfirmApply,
}: DryRunModalProps) {
  const { t } = useI18n();
  const closeButtonRef = useRef<HTMLButtonElement | null>(null);

  const hasBlocking = diff.warnings.some((w) => w.severity === "blocking");
  const effectiveChanges = diff.entries.filter(
    (e) => e.change !== "no_change",
  ).length;

  useEffect(() => {
    closeButtonRef.current?.focus();
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !applying) {
        onClose();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [applying, onClose]);

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-slate-950/65 p-4 backdrop-blur-xs"
      role="presentation"
      onClick={(event) => {
        if (event.target === event.currentTarget && !applying) {
          onClose();
        }
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="dry-run-modal-title"
        className="flex max-h-[90vh] w-full max-w-5xl flex-col overflow-hidden rounded-2xl border border-slate-200 bg-white shadow-2xl dark:border-slate-700 dark:bg-slate-900"
      >
        <div className="flex items-start justify-between gap-4 border-b border-slate-200 px-6 py-4 dark:border-slate-800">
          <div>
            <h2
              id="dry-run-modal-title"
              className="text-lg font-bold text-slate-900 dark:text-slate-50"
            >
              {t("diff.title")}
            </h2>
            <p className="mt-0.5 text-sm text-slate-600 dark:text-slate-400">
              {t("diff.subtitle")}
            </p>
          </div>
          <button
            ref={closeButtonRef}
            type="button"
            disabled={applying}
            onClick={onClose}
            aria-label={t("diff.close")}
            className="rounded-lg p-1.5 text-slate-500 transition hover:bg-slate-100 hover:text-slate-900 disabled:opacity-50 dark:text-slate-400 dark:hover:bg-slate-800 dark:hover:text-slate-100"
          >
            <X className="h-5 w-5" aria-hidden="true" />
          </button>
        </div>

        <div className="flex-1 space-y-4 overflow-y-auto px-6 py-5">
          {error ? (
            <ErrorAlert error={error} onDismiss={onDismissError} />
          ) : null}

          {diff.requires_reboot ? (
            <div
              role="note"
              className="flex items-center gap-2.5 rounded-lg border border-indigo-300 bg-indigo-50 px-4 py-3 text-sm text-indigo-950 dark:border-indigo-800 dark:bg-indigo-950/50 dark:text-indigo-100"
            >
              <RotateCw className="h-4 w-4 shrink-0 text-indigo-600 dark:text-indigo-400" aria-hidden="true" />
              <span>{t("diff.rebootNotice")}</span>
            </div>
          ) : null}

          {hasBlocking ? (
            <div
              role="alert"
              className="flex items-center gap-2.5 rounded-lg border border-rose-300 bg-rose-50 px-4 py-3 text-sm font-medium text-rose-900 dark:border-rose-800 dark:bg-rose-950/60 dark:text-rose-100"
            >
              <AlertOctagon className="h-4 w-4 shrink-0 text-rose-600 dark:text-rose-400" aria-hidden="true" />
              <span>{t("diff.blockingNotice")}</span>
            </div>
          ) : null}

          {diff.warnings.length > 0 ? (
            <section aria-label={t("diff.warningsTitle", { count: diff.warnings.length })}>
              <h3 className="text-xs font-semibold uppercase tracking-wide text-slate-600 dark:text-slate-400">
                {t("diff.warningsTitle", { count: diff.warnings.length })}
              </h3>
              <ul className="mt-2 space-y-2">
                {diff.warnings.map((warning, idx) => (
                  <li
                    key={`${warning.severity}-${warning.kind.kind}-${idx}`}
                    className={`flex items-start gap-2.5 rounded-lg border px-3.5 py-2.5 text-sm ${severityStyle(warning.severity)}`}
                  >
                    {warning.severity === "blocking" ? (
                      <AlertOctagon className="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
                    ) : warning.severity === "warning" ? (
                      <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
                    ) : (
                      <Info className="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
                    )}
                    <div className="min-w-0 flex-1">
                      <span className="mr-2 inline-block rounded px-1.5 py-0.5 text-[11px] font-bold uppercase tracking-wide ring-1 ring-current/30">
                        {severityLabel(warning.severity, t)}
                      </span>
                      <span>{formatConflictWarning(warning, t)}</span>
                    </div>
                  </li>
                ))}
              </ul>
            </section>
          ) : null}

          <div className="flex items-center justify-between text-xs text-slate-600 dark:text-slate-400">
            <span>
              {t("diff.summaryTotal", {
                total: diff.entries.length,
                effective: effectiveChanges,
              })}
            </span>
          </div>

          {diff.entries.length === 0 ? (
            <p className="rounded-lg border border-slate-200 p-4 text-sm text-slate-500 dark:border-slate-800 dark:text-slate-400">
              {t("diff.emptyEntries")}
            </p>
          ) : (
            <div className="overflow-x-auto rounded-xl border border-slate-200 dark:border-slate-800">
              <table className="min-w-full divide-y divide-slate-200 text-left text-xs dark:divide-slate-800">
                <thead className="bg-slate-50 text-slate-600 dark:bg-slate-950/70 dark:text-slate-300">
                  <tr>
                    <th scope="col" className="px-3 py-2.5 font-semibold">
                      {t("diff.colTweak")}
                    </th>
                    <th scope="col" className="px-3 py-2.5 font-semibold">
                      {t("diff.colProvider")}
                    </th>
                    <th scope="col" className="px-3 py-2.5 font-semibold">
                      {t("diff.colResource")}
                    </th>
                    <th scope="col" className="px-3 py-2.5 font-semibold">
                      {t("diff.colBefore")}
                    </th>
                    <th scope="col" className="px-3 py-2.5 font-semibold">
                      {t("diff.colAfter")}
                    </th>
                    <th scope="col" className="px-3 py-2.5 font-semibold">
                      {t("diff.colChange")}
                    </th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-slate-200 bg-white dark:divide-slate-800 dark:bg-slate-900">
                  {diff.entries.map((entry, idx) => (
                    <tr key={`${entry.tweak_id}-${entry.resource}-${idx}`}>
                      <td className="whitespace-nowrap px-3 py-2.5 font-mono text-slate-900 dark:text-slate-100">
                        {entry.tweak_id}
                      </td>
                      <td className="whitespace-nowrap px-3 py-2.5 text-slate-700 dark:text-slate-300">
                        {providerLabel(entry.provider, t)}
                      </td>
                      <td className="break-all px-3 py-2.5 font-mono text-slate-700 dark:text-slate-300">
                        {entry.resource}
                      </td>
                      <td className="px-3 py-2.5 font-mono text-slate-600 dark:text-slate-400">
                        {formatResourceState(entry.before, t)}
                      </td>
                      <td className="px-3 py-2.5 font-mono font-medium text-slate-900 dark:text-slate-100">
                        {formatResourceState(entry.after, t)}
                      </td>
                      <td className="whitespace-nowrap px-3 py-2.5">
                        <span
                          className={`inline-flex rounded-full px-2 py-0.5 text-[11px] font-semibold ${changeBadgeClass(entry.change)}`}
                        >
                          {changeKindLabel(entry.change, t)}
                        </span>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>

        <div className="flex flex-wrap items-center justify-end gap-3 border-t border-slate-200 bg-slate-50 px-6 py-4 dark:border-slate-800 dark:bg-slate-950/60">
          <button
            type="button"
            disabled={applying}
            onClick={onClose}
            className="rounded-lg border border-slate-300 bg-white px-4 py-2 text-sm font-medium text-slate-700 transition hover:bg-slate-50 disabled:opacity-50 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-200 dark:hover:bg-slate-800"
          >
            {t("diff.cancel")}
          </button>
          <button
            type="button"
            disabled={hasBlocking || applying || diff.entries.length === 0}
            onClick={onConfirmApply}
            className="rounded-lg bg-sky-600 px-4 py-2 text-sm font-semibold text-white transition hover:bg-sky-700 disabled:cursor-not-allowed disabled:opacity-50"
          >
            {applying ? t("diff.applying") : t("diff.apply")}
          </button>
        </div>
      </div>
    </div>
  );
}
