import { AlertCircle, X } from "lucide-react";

import { useI18n } from "../i18n";
import { formatUiError, type UiError } from "../lib/format";

interface ErrorAlertProps {
  error: UiError;
  onDismiss?: () => void;
}

export function ErrorAlert({ error, onDismiss }: ErrorAlertProps) {
  const { t } = useI18n();
  const code = error.kind === "app" ? error.error.code : "unexpected";

  return (
    <div
      role="alert"
      className="flex items-start gap-3 rounded-lg border border-rose-300 bg-rose-50 p-4 text-sm text-rose-900 dark:border-rose-800/80 dark:bg-rose-950/60 dark:text-rose-100"
    >
      <AlertCircle className="mt-0.5 h-5 w-5 shrink-0 text-rose-600 dark:text-rose-400" aria-hidden="true" />
      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-center gap-2">
          <p className="font-semibold">{t("error.title")}</p>
          <span className="rounded bg-rose-200/80 px-1.5 py-0.5 font-mono text-xs text-rose-900 dark:bg-rose-900/80 dark:text-rose-200">
            {code}
          </span>
        </div>
        <p className="mt-1 break-words">{formatUiError(error, t)}</p>
      </div>
      {onDismiss ? (
        <button
          type="button"
          onClick={onDismiss}
          aria-label={t("error.dismiss")}
          className="rounded p-1 text-rose-700 hover:bg-rose-200/60 dark:text-rose-300 dark:hover:bg-rose-900/60"
        >
          <X className="h-4 w-4" aria-hidden="true" />
        </button>
      ) : null}
    </div>
  );
}
