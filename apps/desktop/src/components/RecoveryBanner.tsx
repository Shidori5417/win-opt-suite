import { AlertTriangle, RotateCcw } from "lucide-react";

import type { TransactionId, TransactionSummary_Serialize } from "../bindings";
import { useI18n } from "../i18n";
import { formatTimestamp, statusLabel } from "../lib/format";

interface RecoveryBannerProps {
  incomplete: TransactionSummary_Serialize[];
  recoveringId: TransactionId | null;
  onRecover: (id: TransactionId) => void;
}

export function RecoveryBanner({
  incomplete,
  recoveringId,
  onRecover,
}: RecoveryBannerProps) {
  const { locale, t } = useI18n();

  if (incomplete.length === 0) {
    return null;
  }

  return (
    <section
      aria-labelledby="recovery-banner-heading"
      role="alert"
      className="rounded-xl border-2 border-amber-500/80 bg-amber-50 p-5 text-amber-950 shadow-sm dark:border-amber-500/70 dark:bg-amber-950/50 dark:text-amber-100"
    >
      <div className="flex items-start gap-3">
        <AlertTriangle
          className="mt-0.5 h-6 w-6 shrink-0 text-amber-600 dark:text-amber-400"
          aria-hidden="true"
        />
        <div className="min-w-0 flex-1">
          <h2 id="recovery-banner-heading" className="text-base font-semibold">
            {t("recovery.title")}
          </h2>
          <p className="mt-1 text-sm text-amber-900/90 dark:text-amber-200/90">
            {t("recovery.description")}
          </p>
          <p className="mt-1 text-xs font-medium text-amber-800 dark:text-amber-300">
            {t("recovery.blockedHint")}
          </p>

          <ul className="mt-4 space-y-2.5">
            {incomplete.map((tx) => {
              const isRecovering = recoveringId === tx.id;
              return (
                <li
                  key={tx.id}
                  className="flex flex-wrap items-center justify-between gap-3 rounded-lg border border-amber-300 bg-white/80 px-4 py-3 dark:border-amber-800 dark:bg-slate-900/70"
                >
                  <div className="min-w-0">
                    <div className="flex flex-wrap items-center gap-2">
                      <span className="font-mono text-xs font-semibold text-slate-900 dark:text-slate-100">
                        {tx.id}
                      </span>
                      <span className="text-xs text-slate-600 dark:text-slate-400">
                        {formatTimestamp(tx.created_at_unix_ms, locale)}
                      </span>
                    </div>
                    <p className="mt-1 text-xs text-slate-700 dark:text-slate-300">
                      {t("recovery.txMeta", {
                        status: statusLabel(tx.status, t),
                        steps: tx.step_count,
                      })}
                    </p>
                  </div>
                  <button
                    type="button"
                    disabled={recoveringId !== null}
                    onClick={() => onRecover(tx.id)}
                    className="inline-flex items-center gap-2 rounded-lg bg-amber-600 px-3.5 py-2 text-xs font-semibold text-white transition hover:bg-amber-700 disabled:cursor-not-allowed disabled:opacity-60"
                  >
                    <RotateCcw className="h-3.5 w-3.5" aria-hidden="true" />
                    {isRecovering ? t("recovery.recovering") : t("recovery.action")}
                  </button>
                </li>
              );
            })}
          </ul>
        </div>
      </div>
    </section>
  );
}
