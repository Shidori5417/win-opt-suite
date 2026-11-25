import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { App } from "../App";
import type {
  TransactionSummary_Serialize,
  TweakManifest_Serialize,
} from "../bindings";
import { I18nProvider } from "../i18n";
import { RecoveryBanner } from "./RecoveryBanner";

const mockedCommands = vi.hoisted(() => ({
  systemFacts: vi.fn(),
  detectDrift: vi.fn(),
  listTweaks: vi.fn(),
  dryRun: vi.fn(),
  apply: vi.fn(),
  rollback: vi.fn(),
  history: vi.fn(),
  incompleteTransactions: vi.fn(),
  recover: vi.fn(),
  transactionMetrics: vi.fn(),
}));

vi.mock("../bindings", () => ({
  commands: mockedCommands,
}));

const CRASHED: TransactionSummary_Serialize = {
  id: "tx-crashed-01",
  created_at_unix_ms: 1_760_000_000_000,
  status: "applying",
  profile: "gaming",
  tweak_ids: ["gaming.hardware-gpu-scheduling"],
  step_count: 2,
};

const FAILED: TransactionSummary_Serialize = {
  ...CRASHED,
  id: "tx-failed-02",
  status: "failed",
  step_count: 4,
};

const TELEMETRY: TweakManifest_Serialize = {
  schema_version: 1,
  id: "privacy.telemetry-minimum",
  title: "Limit diagnostic data",
  description: "Sets AllowTelemetry policy to minimum.",
  category: "privacy",
  risk: "low",
  side_effects: "Insider feedback stops working.",
  min_windows_build: 22000,
  excluded_on: [],
  conflicts_with: [],
  profiles: ["safe"],
  requires_reboot: false,
  references: [],
  actions: [],
};

function pending<T>(): Promise<T> {
  return new Promise<T>(() => {});
}

describe("RecoveryBanner component", () => {
  it("renders nothing when there are no incomplete transactions", () => {
    const { container } = render(
      <I18nProvider initialLocale="en">
        <RecoveryBanner incomplete={[]} recoveringId={null} onRecover={vi.fn()} />
      </I18nProvider>,
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("lists each incomplete transaction with its status and step count", () => {
    render(
      <I18nProvider initialLocale="en">
        <RecoveryBanner
          incomplete={[CRASHED, FAILED]}
          recoveringId={null}
          onRecover={vi.fn()}
        />
      </I18nProvider>,
    );

    const alert = screen.getByRole("alert");
    expect(
      within(alert).getByText(/Incomplete transaction detected/),
    ).toBeInTheDocument();
    expect(
      within(alert).getByText("Status: Applying · 2 step(s)"),
    ).toBeInTheDocument();
    expect(
      within(alert).getByText("Status: Failed · 4 step(s)"),
    ).toBeInTheDocument();
    expect(within(alert).getAllByRole("button", { name: "Recover Now" })).toHaveLength(2);
  });

  it("passes the transaction id to onRecover", async () => {
    const user = userEvent.setup();
    const onRecover = vi.fn();
    render(
      <I18nProvider initialLocale="en">
        <RecoveryBanner
          incomplete={[CRASHED]}
          recoveringId={null}
          onRecover={onRecover}
        />
      </I18nProvider>,
    );

    await user.click(screen.getByRole("button", { name: "Recover Now" }));

    expect(onRecover).toHaveBeenCalledWith("tx-crashed-01");
  });

  it("shows progress and disables every recover button while one is running", () => {
    render(
      <I18nProvider initialLocale="en">
        <RecoveryBanner
          incomplete={[CRASHED, FAILED]}
          recoveringId="tx-crashed-01"
          onRecover={vi.fn()}
        />
      </I18nProvider>,
    );

    expect(screen.getByRole("button", { name: "Recovering..." })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Recover Now" })).toBeDisabled();
  });
});

describe("Recovery gating in the application", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    window.localStorage.clear();
    mockedCommands.systemFacts.mockReturnValue(pending());
    mockedCommands.detectDrift.mockReturnValue(pending());
    mockedCommands.listTweaks.mockResolvedValue({
      status: "ok",
      data: [TELEMETRY],
    });
    mockedCommands.history.mockResolvedValue({ status: "ok", data: [] });
    mockedCommands.dryRun.mockResolvedValue({
      status: "ok",
      data: { entries: [], warnings: [], requires_reboot: false },
    });
  });

  it("calls recover for the banner transaction and clears the banner afterwards", async () => {
    const user = userEvent.setup();
    let pendingRecovery: TransactionSummary_Serialize[] = [CRASHED];
    mockedCommands.incompleteTransactions.mockImplementation(async () => ({
      status: "ok",
      data: pendingRecovery,
    }));
    mockedCommands.recover.mockImplementation(async () => {
      pendingRecovery = [];
      return { status: "ok", data: "rolled_back" };
    });

    render(<App initialLocale="en" />);

    await user.click(await screen.findByRole("button", { name: "Recover Now" }));

    expect(mockedCommands.recover).toHaveBeenCalledTimes(1);
    expect(mockedCommands.recover).toHaveBeenCalledWith("tx-crashed-01");
    await waitFor(() => {
      expect(
        screen.queryByText(/Incomplete transaction detected/),
      ).not.toBeInTheDocument();
    });
  });

  it("blocks apply actions until recovery completes", async () => {
    const user = userEvent.setup();
    let pendingRecovery: TransactionSummary_Serialize[] = [CRASHED];
    mockedCommands.incompleteTransactions.mockImplementation(async () => ({
      status: "ok",
      data: pendingRecovery,
    }));
    mockedCommands.recover.mockImplementation(async () => {
      pendingRecovery = [];
      return { status: "ok", data: "rolled_back" };
    });

    render(<App initialLocale="en" />);
    await screen.findByText(/Incomplete transaction detected/);

    await user.click(screen.getByRole("button", { name: /Tweak Catalog/i }));
    await screen.findByText(TELEMETRY.title);

    await user.click(screen.getByRole("checkbox", { name: /Limit diagnostic data/ }));
    expect(
      screen.getByRole("button", { name: "Preview Dry-Run Diff" }),
    ).toBeDisabled();
    expect(screen.getByRole("button", { name: "Dry-Run Safe" })).toBeDisabled();
    expect(mockedCommands.dryRun).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "Recover Now" }));
    await waitFor(() => {
      expect(
        screen.getByRole("button", { name: "Preview Dry-Run Diff" }),
      ).toBeEnabled();
    });
    expect(screen.getByRole("button", { name: "Dry-Run Safe" })).toBeEnabled();
  });
});
