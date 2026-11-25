import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { App } from "../App";
import type {
  DryRunDiff_Serialize,
  TweakManifest_Serialize,
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

function manifest(
  overrides: Partial<TweakManifest_Serialize> & { id: string },
): TweakManifest_Serialize {
  return {
    schema_version: 1,
    title: overrides.id,
    description: `${overrides.id} description`,
    category: "privacy",
    risk: "low",
    side_effects: "none",
    min_windows_build: 22000,
    excluded_on: [],
    conflicts_with: [],
    profiles: [],
    requires_reboot: false,
    references: [],
    actions: [],
    ...overrides,
  };
}

const TELEMETRY = manifest({
  id: "privacy.telemetry-minimum",
  title: "Limit diagnostic data",
  category: "privacy",
  profiles: ["safe", "privacy"],
});
const GPU = manifest({
  id: "gaming.hardware-gpu-scheduling",
  title: "Enable Hardware-Accelerated GPU Scheduling",
  category: "gaming",
  profiles: ["gaming"],
  requires_reboot: true,
});
const VISUALS = manifest({
  id: "performance.visual-effects",
  title: "Reduce visual effects",
  category: "performance",
  profiles: ["safe"],
});

const CLEAN_DIFF: DryRunDiff_Serialize = {
  entries: [],
  warnings: [],
  requires_reboot: false,
};

function pending<T>(): Promise<T> {
  return new Promise<T>(() => {});
}

beforeEach(() => {
  vi.clearAllMocks();
  window.localStorage.clear();
  mockedCommands.systemFacts.mockReturnValue(pending());
  mockedCommands.detectDrift.mockReturnValue(pending());
  mockedCommands.incompleteTransactions.mockResolvedValue({
    status: "ok",
    data: [],
  });
  mockedCommands.listTweaks.mockResolvedValue({
    status: "ok",
    data: [TELEMETRY, GPU, VISUALS],
  });
  mockedCommands.dryRun.mockResolvedValue({ status: "ok", data: CLEAN_DIFF });
});

async function openCatalog() {
  const user = userEvent.setup();
  render(<App initialLocale="en" />);
  await user.click(screen.getByRole("button", { name: /Tweak Catalog/i }));
  await screen.findByText(TELEMETRY.title);
  return user;
}

function visibleTitles(): string[] {
  return screen
    .queryAllByRole("listitem")
    .map((item) => within(item).queryByRole("checkbox"))
    .filter((cb): cb is HTMLElement => cb !== null)
    .map((cb) => cb.getAttribute("aria-label") ?? "");
}

function previewButton() {
  return screen.getByRole("button", { name: /Preview Dry-Run Diff/i });
}

describe("TweakCatalogScreen filtering", () => {
  it("filters by category", async () => {
    const user = await openCatalog();

    await user.selectOptions(screen.getByLabelText("Category"), "gaming");

    expect(visibleTitles()).toEqual([
      "Select tweak Enable Hardware-Accelerated GPU Scheduling",
    ]);
  });

  it("filters by profile", async () => {
    const user = await openCatalog();

    await user.selectOptions(screen.getByLabelText("Profile"), "safe");

    expect(visibleTitles()).toEqual([
      "Select tweak Limit diagnostic data",
      "Select tweak Reduce visual effects",
    ]);
  });

  it("combines search with the category filter", async () => {
    const user = await openCatalog();

    await user.type(screen.getByLabelText("Search tweaks"), "visual");
    expect(visibleTitles()).toEqual([
      "Select tweak Reduce visual effects",
    ]);

    await user.selectOptions(screen.getByLabelText("Category"), "privacy");
    expect(screen.getByText("No tweaks match the current filters.")).toBeInTheDocument();
  });
});

describe("TweakCatalogScreen selection and dry-run requests", () => {
  it("sends an explicit tweak list for multi-select in catalog order", async () => {
    const user = await openCatalog();

    await user.click(
      screen.getByRole("checkbox", { name: /Reduce visual effects/ }),
    );
    await user.click(
      screen.getByRole("checkbox", { name: /Limit diagnostic data/ }),
    );
    expect(screen.getByText("2 tweak(s) selected")).toBeInTheDocument();

    await user.click(previewButton());

    expect(mockedCommands.dryRun).toHaveBeenCalledWith({
      kind: "tweaks",
      ids: ["privacy.telemetry-minimum", "performance.visual-effects"],
    });
  });

  it("sends a profile selection when a profile quick-select is used", async () => {
    const user = await openCatalog();

    await user.click(screen.getByRole("button", { name: "Select Gaming (1)" }));
    expect(
      screen.getByRole("checkbox", { name: /Enable Hardware-Accelerated/ }),
    ).toBeChecked();

    await user.click(previewButton());

    expect(mockedCommands.dryRun).toHaveBeenCalledWith({
      kind: "profile",
      profile: "gaming",
    });
  });

  it("uses the profile dry-run shortcut directly", async () => {
    const user = await openCatalog();

    await user.click(screen.getByRole("button", { name: "Dry-Run Safe" }));

    expect(mockedCommands.dryRun).toHaveBeenCalledWith({
      kind: "profile",
      profile: "safe",
    });
  });

  it("drops the profile selection once the user edits the checkboxes", async () => {
    const user = await openCatalog();

    await user.click(screen.getByRole("button", { name: "Select Safe (2)" }));
    await user.click(
      screen.getByRole("checkbox", { name: /Reduce visual effects/ }),
    );
    await user.click(previewButton());

    expect(mockedCommands.dryRun).toHaveBeenCalledWith({
      kind: "tweaks",
      ids: ["privacy.telemetry-minimum"],
    });
  });

  it("selects only the visible tweaks with Select Visible", async () => {
    const user = await openCatalog();

    await user.selectOptions(screen.getByLabelText("Category"), "gaming");
    await user.click(screen.getByRole("button", { name: "Select Visible" }));
    await user.click(previewButton());

    expect(mockedCommands.dryRun).toHaveBeenCalledWith({
      kind: "tweaks",
      ids: ["gaming.hardware-gpu-scheduling"],
    });
  });

  it("disables preview when nothing is selected and after clearing", async () => {
    const user = await openCatalog();
    expect(previewButton()).toBeDisabled();

    await user.click(
      screen.getByRole("checkbox", { name: /Reduce visual effects/ }),
    );
    expect(previewButton()).toBeEnabled();

    await user.click(screen.getByRole("button", { name: "Clear Selection" }));
    expect(screen.getByText("0 tweak(s) selected")).toBeInTheDocument();
    expect(previewButton()).toBeDisabled();
  });
});
