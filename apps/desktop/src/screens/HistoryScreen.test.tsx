import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { App } from "../App";
import type {
  MetricSample,
  TransactionSummary_Serialize,
} from "../bindings";

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

const GIB = 1024 * 1024 * 1024;
const MIB = 1024 * 1024;

const COMMITTED: TransactionSummary_Serialize = {
  id: "tx-committed-01",
  created_at_unix_ms: 1_760_000_000_000,
  status: "committed",
  profile: "safe",
  tweak_ids: ["privacy.telemetry-minimum"],
  step_count: 1,
};

function sample(
  kind: MetricSample["kind"],
  phase: MetricSample["phase"],
  value: number,
): MetricSample {
  return { kind, phase, value, captured_at_unix_ms: 1_760_000_000_000 };
}

function pending<T>(): Promise<T> {
  return new Promise<T>(() => {});
}

beforeEach(() => {
  vi.clearAllMocks();
  window.localStorage.clear();
  mockedCommands.systemFacts.mockReturnValue(pending());
  mockedCommands.detectDrift.mockReturnValue(pending());
  mockedCommands.listTweaks.mockResolvedValue({ status: "ok", data: [] });
  mockedCommands.incompleteTransactions.mockResolvedValue({
    status: "ok",
    data: [],
  });
  mockedCommands.history.mockResolvedValue({
    status: "ok",
    data: [COMMITTED],
  });
  mockedCommands.rollback.mockResolvedValue({ status: "ok", data: null });
  mockedCommands.transactionMetrics.mockResolvedValue({
    status: "ok",
    data: [],
  });
});

async function openHistory() {
  const user = userEvent.setup();
  render(<App initialLocale="en" />);
  await user.click(screen.getByRole("button", { name: /History & Rollback/i }));
  await screen.findByText(COMMITTED.id);
  return user;
}

describe("HistoryScreen rollback confirmation", () => {
  it("opens a confirmation dialog and does not roll back before confirmation", async () => {
    const user = await openHistory();

    await user.click(screen.getByRole("button", { name: /^Rollback$/ }));

    const dialog = await screen.findByRole("alertdialog");
    expect(
      within(dialog).getByText(/Revert all steps in transaction tx-committed-01/),
    ).toBeInTheDocument();
    expect(mockedCommands.rollback).not.toHaveBeenCalled();
  });

  it("does not call rollback when the dialog is cancelled", async () => {
    const user = await openHistory();

    await user.click(screen.getByRole("button", { name: /^Rollback$/ }));
    const dialog = await screen.findByRole("alertdialog");
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));

    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
    expect(mockedCommands.rollback).not.toHaveBeenCalled();
    expect(mockedCommands.history).toHaveBeenCalledTimes(1);
  });

  it("calls rollback exactly once after confirmation and reports success", async () => {
    const user = await openHistory();

    await user.click(screen.getByRole("button", { name: /^Rollback$/ }));
    const dialog = await screen.findByRole("alertdialog");
    await user.click(within(dialog).getByRole("button", { name: /Yes, Roll Back/ }));

    expect(mockedCommands.rollback).toHaveBeenCalledTimes(1);
    expect(mockedCommands.rollback).toHaveBeenCalledWith("tx-committed-01");
    expect(
      await screen.findByText("Transaction tx-committed-01 was rolled back successfully."),
    ).toBeInTheDocument();
  });

  it("does not offer rollback for non-committed transactions", async () => {
    mockedCommands.history.mockResolvedValue({
      status: "ok",
      data: [{ ...COMMITTED, status: "rolled_back" }],
    });
    const user = userEvent.setup();
    render(<App initialLocale="en" />);
    await user.click(screen.getByRole("button", { name: /History & Rollback/i }));
    await screen.findByText(COMMITTED.id);

    expect(screen.queryByRole("button", { name: /^Rollback$/ })).not.toBeInTheDocument();
  });
});

describe("HistoryScreen metric deltas", () => {
  it("formats positive deltas with a plus sign", async () => {
    mockedCommands.transactionMetrics.mockResolvedValue({
      status: "ok",
      data: [
        sample("idle_memory_used_bytes", "before", 4 * GIB),
        sample("idle_memory_used_bytes", "after", 4 * GIB + 256 * MIB),
        sample("running_services", "before", 90),
        sample("running_services", "after", 93),
      ],
    });
    const user = await openHistory();

    await user.click(screen.getByRole("button", { name: "Before / After Report" }));

    expect(await screen.findByText("+256 MiB")).toBeInTheDocument();
    expect(screen.getByText("+3")).toBeInTheDocument();
  });

  it("formats negative deltas with a minus sign and renders missing samples as a dash", async () => {
    mockedCommands.transactionMetrics.mockResolvedValue({
      status: "ok",
      data: [
        sample("idle_memory_used_bytes", "before", 8 * GIB),
        sample("idle_memory_used_bytes", "after", 8 * GIB - 512 * MIB),
        sample("running_services", "before", 96),
        sample("running_services", "after", 92),
        sample("running_processes", "before", 210),
      ],
    });
    const user = await openHistory();

    await user.click(screen.getByRole("button", { name: "Before / After Report" }));

    expect(await screen.findByText("-512 MiB")).toBeInTheDocument();
    expect(screen.getByText("-4")).toBeInTheDocument();
    const processesRow = screen.getByText("Running Processes").closest("tr");
    expect(processesRow).not.toBeNull();
    expect(within(processesRow as HTMLElement).getAllByText("—")).toHaveLength(2);
  });
});
