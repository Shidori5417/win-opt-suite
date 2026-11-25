import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import type { AppError_Serialize } from "../bindings";
import en from "../i18n/en.json";
import tr from "../i18n/tr.json";
import { I18nProvider, type Locale } from "../i18n";
import { toAppError, toUnexpectedError, type UiError } from "../lib/format";
import { ErrorAlert } from "./ErrorAlert";

const enMessages: Record<string, string> = en;
const trMessages: Record<string, string> = tr;

// Exhaustive over the generated AppError union: adding a backend code without
// a sample here fails the typecheck, which forces the i18n key check below to run.
const SAMPLE_ERRORS = {
  access_denied: { code: "access_denied", resource: "HKLM\\SOFTWARE\\Policies" },
  not_found: { code: "not_found", resource: "tweak.unknown" },
  unsupported_platform: {
    code: "unsupported_platform",
    reason: "system probe requires Windows 11",
  },
  provider_not_registered: { code: "provider_not_registered", provider: "service" },
  action_provider_mismatch: {
    code: "action_provider_mismatch",
    expected: "registry",
    found: "service",
  },
  invalid_manifest: { code: "invalid_manifest", id: "bad.tweak", issues: [] },
  conflict_blocked: {
    code: "conflict_blocked",
    warnings: [
      {
        severity: "blocking",
        kind: { kind: "declared_conflict", tweak: "a", other: "b" },
      },
      {
        severity: "blocking",
        kind: { kind: "declared_conflict", tweak: "c", other: "d" },
      },
    ],
  },
  lock_unavailable: { code: "lock_unavailable" },
  recovery_required: {
    code: "recovery_required",
    transaction_id: "tx-crashed-01",
  },
  verification_failed: { code: "verification_failed", resource: "registry:x" },
  rollback_incomplete: {
    code: "rollback_incomplete",
    transaction_id: "tx-rb-01",
    failed_resources: ["registry:a", "service:b"],
  },
  storage: { code: "storage", message: "database locked" },
  migration: { code: "migration", version: 2, message: "syntax error" },
  serialization: { code: "serialization", message: "bad json" },
  io: { code: "io", message: "disk full" },
  os: { code: "os", os_code: 5, message: "Access is denied." },
  internal: { code: "internal", message: "invariant broken" },
} satisfies Record<AppError_Serialize["code"], AppError_Serialize>;

const ALL_CODES = Object.keys(SAMPLE_ERRORS) as AppError_Serialize["code"][];

function renderAlert(error: UiError, locale: Locale = "en") {
  return render(
    <I18nProvider initialLocale={locale}>
      <ErrorAlert error={error} />
    </I18nProvider>,
  );
}

describe("AppError i18n coverage", () => {
  it.each(ALL_CODES)("has English and Turkish messages for code %s", (code) => {
    expect(enMessages[`error.${code}`], `en error.${code}`).toBeTruthy();
    expect(trMessages[`error.${code}`], `tr error.${code}`).toBeTruthy();
  });
});

describe("ErrorAlert rendering", () => {
  it("renders the localized access_denied message with the code badge", () => {
    renderAlert(toAppError(SAMPLE_ERRORS.access_denied));

    const alert = screen.getByRole("alert");
    expect(within(alert).getByText("access_denied")).toBeInTheDocument();
    expect(
      within(alert).getByText(
        "Access denied for HKLM\\SOFTWARE\\Policies. Run WinOptSuite as Administrator.",
      ),
    ).toBeInTheDocument();
  });

  it("renders unsupported_platform in English and Turkish", () => {
    const error = toAppError(SAMPLE_ERRORS.unsupported_platform);
    const { unmount } = renderAlert(error, "en");
    expect(
      screen.getByText(
        "Unsupported platform: system probe requires Windows 11. Native Windows 11 system operations require Windows.",
      ),
    ).toBeInTheDocument();
    unmount();

    renderAlert(error, "tr");
    expect(
      screen.getByText(
        "Desteklenmeyen platform: system probe requires Windows 11. Yerel Windows 11 sistem işlemleri için Windows gerekir.",
      ),
    ).toBeInTheDocument();
  });

  it("renders conflict_blocked with the number of conflicts", () => {
    renderAlert(toAppError(SAMPLE_ERRORS.conflict_blocked));
    expect(
      screen.getByText("Apply blocked by 2 conflict(s)."),
    ).toBeInTheDocument();
  });

  it("renders lock_unavailable without parameters", () => {
    renderAlert(toAppError(SAMPLE_ERRORS.lock_unavailable));
    expect(
      screen.getByText(
        "Another WinOptSuite transaction is already running. Wait for it to finish.",
      ),
    ).toBeInTheDocument();
  });

  it("renders recovery_required with the transaction id", () => {
    renderAlert(toAppError(SAMPLE_ERRORS.recovery_required));
    expect(
      screen.getByText(
        "Incomplete transaction tx-crashed-01 must be recovered before making new changes.",
      ),
    ).toBeInTheDocument();
  });

  it("joins failed resources in rollback_incomplete", () => {
    renderAlert(toAppError(SAMPLE_ERRORS.rollback_incomplete));
    expect(
      screen.getByText(
        "Rollback of tx-rb-01 could not restore all resources: registry:a, service:b.",
      ),
    ).toBeInTheDocument();
  });

  it("renders every backend code with a non-raw localized message in both locales", () => {
    for (const locale of ["en", "tr"] as const) {
      for (const code of ALL_CODES) {
        const { unmount, container } = renderAlert(
          toAppError(SAMPLE_ERRORS[code]),
          locale,
        );
        const text = container.querySelector("p.break-words")?.textContent ?? "";
        expect(text, `${locale}/${code}`).not.toBe("");
        expect(text, `${locale}/${code}`).not.toMatch(/^error\./);
        unmount();
      }
    }
  });

  it("renders unexpected frontend errors with the unknown message", () => {
    renderAlert(toUnexpectedError(new Error("IPC channel closed")));
    expect(
      screen.getByText("Unexpected error: IPC channel closed"),
    ).toBeInTheDocument();
    expect(screen.getByText("unexpected")).toBeInTheDocument();
  });

  it("calls onDismiss when the dismiss button is pressed", async () => {
    const user = userEvent.setup();
    let dismissed = 0;
    render(
      <I18nProvider initialLocale="en">
        <ErrorAlert
          error={toAppError(SAMPLE_ERRORS.lock_unavailable)}
          onDismiss={() => {
            dismissed += 1;
          }}
        />
      </I18nProvider>,
    );

    await user.click(screen.getByRole("button", { name: "Dismiss error" }));
    expect(dismissed).toBe(1);
  });
});
