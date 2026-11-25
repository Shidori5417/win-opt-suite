import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { App } from "./App";
import type {
  DriftReport_Serialize,
  DryRunDiff_Serialize,
  MetricSample,
  SystemFacts,
  TransactionSummary_Serialize,
  TweakManifest_Serialize,
} from "./bindings";
import en from "./i18n/en.json";
import tr from "./i18n/tr.json";

const mockedCommands = vi.hoisted(() => ({
  systemFacts: vi.fn(),
  listTweaks: vi.fn(),
  dryRun: vi.fn(),
  apply: vi.fn(),
  rollback: vi.fn(),
  history: vi.fn(),
  incompleteTransactions: vi.fn(),
  recover: vi.fn(),
  detectDrift: vi.fn(),
  transactionMetrics: vi.fn(),
}));

vi.mock("./bindings", () => ({
  commands: mockedCommands,
}));

const SAMPLE_FACTS: SystemFacts = {
  windows_build: 26100,
  windows_ubr: 2605,
  windows_display_version: "24H2",
  edition: "Professional",
  cpu: {
    brand: "AMD Ryzen 7 7800X3D",
    physical_cores: 8,
    logical_cores: 16,
  },
  gpus: [
    {
      name: "NVIDIA GeForce RTX 4080",
      vendor_id: 0x10de,
      dedicated_memory_bytes: 16 * 1024 * 1024 * 1024,
    },
  ],
  total_memory_bytes: 32 * 1024 * 1024 * 1024,
  is_laptop: false,
  on_battery: false,
  is_virtual_machine: false,
  is_domain_joined: false,
  anti_cheat: ["vanguard"],
};

const SAMPLE_TWEAKS: TweakManifest_Serialize[] = [
  {
    schema_version: 1,
    id: "privacy.telemetry-minimum",
    title: "Limit diagnostic data",
    description: "Sets AllowTelemetry policy to minimum.",
    category: "privacy",
    risk: "low",
    side_effects: "Windows Insider feedback features stop working.",
    min_windows_build: 22000,
    excluded_on: ["domain_joined"],
    conflicts_with: [],
    profiles: ["safe", "privacy"],
    requires_reboot: false,
    references: ["https://learn.microsoft.com/windows/privacy"],
    actions: [
      {
        provider: "registry",
        hive: "HKLM",
        key: "SOFTWARE\\Policies\\Microsoft\\Windows\\DataCollection",
        value_name: "AllowTelemetry",
        value: { type: "dword", data: 0 },
      },
    ],
  },
  {
    schema_version: 1,
    id: "gaming.hardware-gpu-scheduling",
    title: "Enable Hardware-Accelerated GPU Scheduling",
    description: "Enables HAGS via HwSchMode.",
    category: "gaming",
    risk: "medium",
    side_effects: "Requires a reboot; may affect older capture overlays.",
    min_windows_build: 22000,
    excluded_on: ["virtual_machine"],
    conflicts_with: [],
    profiles: ["gaming"],
    requires_reboot: true,
    references: [],
    actions: [
      {
        provider: "registry",
        hive: "HKLM",
        key: "SYSTEM\\CurrentControlSet\\Control\\GraphicsDrivers",
        value_name: "HwSchMode",
        value: { type: "dword", data: 2 },
      },
    ],
  },
];

const SAMPLE_DRIFT: DriftReport_Serialize = {
  checked: 2,
  drifted: [
    {
      transaction_id: "tx-001",
      tweak_id: "privacy.telemetry-minimum",
      provider: "registry",
      resource: "registry:hklm\\software\\policies\\datacollection\\allowtelemetry",
      expected: { kind: "registry_value", value: { type: "dword", data: 0 } },
      actual: { kind: "registry_value", value: { type: "dword", data: 3 } },
    },
  ],
  errors: [],
};

const SAMPLE_HISTORY: TransactionSummary_Serialize[] = [
  {
    id: "tx-committed-01",
    created_at_unix_ms: 1_760_000_000_000,
    status: "committed",
    profile: "safe",
    tweak_ids: ["privacy.telemetry-minimum"],
    step_count: 1,
  },
];

const SAMPLE_METRICS: MetricSample[] = [
  {
    kind: "idle_memory_used_bytes",
    phase: "before",
    value: 8 * 1024 * 1024 * 1024,
    captured_at_unix_ms: 1_760_000_000_000,
  },
  {
    kind: "idle_memory_used_bytes",
    phase: "after",
    value: 7.5 * 1024 * 1024 * 1024,
    captured_at_unix_ms: 1_760_000_010_000,
  },
  {
    kind: "running_services",
    phase: "before",
    value: 96,
    captured_at_unix_ms: 1_760_000_000_000,
  },
  {
    kind: "running_services",
    phase: "after",
    value: 92,
    captured_at_unix_ms: 1_760_000_010_000,
  },
];

function resetDefaultMocks() {
  mockedCommands.incompleteTransactions.mockResolvedValue({
    status: "ok",
    data: [],
  });
  mockedCommands.systemFacts.mockResolvedValue({
    status: "ok",
    data: SAMPLE_FACTS,
  });
  mockedCommands.detectDrift.mockResolvedValue({
    status: "ok",
    data: SAMPLE_DRIFT,
  });
  mockedCommands.listTweaks.mockResolvedValue({
    status: "ok",
    data: SAMPLE_TWEAKS,
  });
  mockedCommands.history.mockResolvedValue({
    status: "ok",
    data: SAMPLE_HISTORY,
  });
  mockedCommands.transactionMetrics.mockResolvedValue({
    status: "ok",
    data: SAMPLE_METRICS,
  });
  mockedCommands.rollback.mockResolvedValue({ status: "ok", data: null });
  mockedCommands.recover.mockResolvedValue({
    status: "ok",
    data: "rolled_back",
  });
  mockedCommands.apply.mockResolvedValue({
    status: "ok",
    data: "tx-new-99",
  });
}

describe("WinOptSuite Desktop UI", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    window.localStorage.clear();
    resetDefaultMocks();
  });

  it("keeps en.json and tr.json translation keys in strict parity", () => {
    expect(Object.keys(tr).sort()).toEqual(Object.keys(en).sort());
  });

  it("renders system overview facts, anti-cheat warning, and runs drift scan", async () => {
    const user = userEvent.setup();
    render(<App initialLocale="en" />);

    expect(await screen.findByText(/AMD Ryzen 7 7800X3D/)).toBeInTheDocument();
    expect(screen.getByText(/NVIDIA GeForce RTX 4080/)).toBeInTheDocument();
    expect(screen.getByText(/Kernel Anti-Cheat Detected/)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /Run Drift Scan/i }));

    expect(mockedCommands.detectDrift).toHaveBeenCalledTimes(1);
    expect(
      await screen.findByText(/privacy\.telemetry-minimum/),
    ).toBeInTheDocument();
    expect(screen.getByText(/REG_DWORD \(3\)/)).toBeInTheDocument();
  });

  it("surfaces AppError by code via i18n on unsupported platforms", async () => {
    mockedCommands.systemFacts.mockResolvedValue({
      status: "error",
      error: {
        code: "unsupported_platform",
        reason: "system probe requires Windows 11",
      },
    });

    render(<App initialLocale="en" />);

    expect(
      await screen.findByText(/unsupported_platform/),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/system probe requires Windows 11/),
    ).toBeInTheDocument();
  });

  it("shows startup recovery banner when incomplete transactions exist and recovers them", async () => {
    const user = userEvent.setup();
    mockedCommands.incompleteTransactions
      .mockResolvedValueOnce({
        status: "ok",
        data: [
          {
            id: "tx-crashed-01",
            created_at_unix_ms: 1_760_000_000_000,
            status: "applying",
            profile: "gaming",
            tweak_ids: ["gaming.hardware-gpu-scheduling"],
            step_count: 2,
          },
        ],
      })
      .mockResolvedValueOnce({ status: "ok", data: [] });

    render(<App initialLocale="en" />);

    expect(
      await screen.findByText(/Incomplete transaction detected/i),
    ).toBeInTheDocument();
    expect(screen.getByText("tx-crashed-01")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /Recover Now/i }));

    expect(mockedCommands.recover).toHaveBeenCalledWith("tx-crashed-01");
    await waitFor(() => {
      expect(
        screen.queryByText(/Incomplete transaction detected/i),
      ).not.toBeInTheDocument();
    });
  });

  it("filters tweak catalog, opens dry-run modal, blocks apply on blocking conflict, and applies when clean", async () => {
    const user = userEvent.setup();
    const blockingDiff: DryRunDiff_Serialize = {
      entries: [
        {
          tweak_id: "gaming.hardware-gpu-scheduling",
          provider: "registry",
          resource: "registry:hklm\\graphicsdrivers\\hwschmode",
          before: { kind: "registry_value", value: { type: "dword", data: 1 } },
          after: { kind: "registry_value", value: { type: "dword", data: 2 } },
          change: "modify",
        },
      ],
      warnings: [
        {
          severity: "blocking",
          kind: {
            kind: "excluded_environment",
            tweak: "gaming.hardware-gpu-scheduling",
            tag: "virtual_machine",
          },
        },
      ],
      requires_reboot: true,
    };
    const cleanDiff: DryRunDiff_Serialize = {
      entries: [
        {
          tweak_id: "privacy.telemetry-minimum",
          provider: "registry",
          resource: "registry:hklm\\datacollection\\allowtelemetry",
          before: null,
          after: { kind: "registry_value", value: { type: "dword", data: 0 } },
          change: "create",
        },
      ],
      warnings: [],
      requires_reboot: false,
    };
    mockedCommands.dryRun
      .mockResolvedValueOnce({ status: "ok", data: blockingDiff })
      .mockResolvedValueOnce({ status: "ok", data: cleanDiff });

    render(<App initialLocale="en" />);

    await user.click(screen.getByRole("button", { name: /Tweak Catalog/i }));
    expect(
      await screen.findByText("Limit diagnostic data"),
    ).toBeInTheDocument();

    // Preview gaming profile -> blocking conflict disables Apply button
    await user.click(screen.getByRole("button", { name: /Dry-Run Gaming/i }));
    const dialog = await screen.findByRole("dialog");
    expect(
      within(dialog).getByText(/require a system reboot/i),
    ).toBeInTheDocument();
    expect(
      within(dialog).getByRole("button", { name: /Confirm & Apply/i }),
    ).toBeDisabled();
    await user.click(within(dialog).getByRole("button", { name: /Cancel/i }));

    // Select telemetry tweak and preview clean diff -> Confirm & Apply succeeds
    await user.click(
      screen.getByRole("checkbox", { name: /Limit diagnostic data/i }),
    );
    await user.click(
      screen.getByRole("button", { name: /Preview Dry-Run Diff/i }),
    );
    const cleanDialog = await screen.findByRole("dialog");
    const applyBtn = within(cleanDialog).getByRole("button", {
      name: /Confirm & Apply/i,
    });
    expect(applyBtn).toBeEnabled();
    await user.click(applyBtn);

    expect(mockedCommands.apply).toHaveBeenCalledWith(
      { kind: "tweaks", ids: ["privacy.telemetry-minimum"] },
      null,
    );
    expect(await screen.findByText(/tx-new-99/)).toBeInTheDocument();
  });

  it("renders rollback history, expands before/after metrics, and confirms rollback", async () => {
    const user = userEvent.setup();
    render(<App initialLocale="en" />);

    await user.click(
      screen.getByRole("button", { name: /History & Rollback/i }),
    );
    expect(await screen.findByText("tx-committed-01")).toBeInTheDocument();

    await user.click(
      screen.getByRole("button", { name: /Before \/ After Report/i }),
    );
    expect(mockedCommands.transactionMetrics).toHaveBeenCalledWith(
      "tx-committed-01",
    );
    expect(await screen.findByText("Idle Memory Used")).toBeInTheDocument();
    expect(screen.getByText("-512 MiB")).toBeInTheDocument();
    expect(screen.getByText("-4")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /^Rollback$/i }));
    const alertDialog = await screen.findByRole("alertdialog");
    await user.click(
      within(alertDialog).getByRole("button", { name: /Yes, Roll Back/i }),
    );
    expect(mockedCommands.rollback).toHaveBeenCalledWith("tx-committed-01");
  });

  it("toggles language between EN and TR and persists selection in localStorage", async () => {
    const user = userEvent.setup();
    render(<App initialLocale="en" />);

    expect(
      await screen.findByText("System Overview & Health"),
    ).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /Switch language/i }));

    expect(screen.getByText("Sistem Özeti ve Sağlık")).toBeInTheDocument();
    expect(window.localStorage.getItem("winoptsuite.locale")).toBe("tr");
  });
});
