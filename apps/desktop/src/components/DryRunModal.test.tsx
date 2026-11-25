import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type {
  ChangeKind,
  ConflictWarning_Serialize,
  DryRunDiff_Serialize as Diff,
} from "../bindings";
import { I18nProvider } from "../i18n";
import { DryRunModal } from "./DryRunModal";

type Entry = Diff["entries"][number];

function entry(change: ChangeKind, tweakId = "privacy.telemetry-minimum"): Entry {
  return {
    tweak_id: tweakId,
    provider: "registry",
    resource: "registry:hklm\\software\\policies\\datacollection\\allowtelemetry",
    before: change === "create" ? null : { kind: "registry_value", value: { type: "dword", data: 3 } },
    after: { kind: "registry_value", value: { type: "dword", data: 0 } },
    change,
  };
}

const BLOCKING_WARNING: ConflictWarning_Serialize = {
  severity: "blocking",
  kind: {
    kind: "excluded_environment",
    tweak: "gaming.hardware-gpu-scheduling",
    tag: "virtual_machine",
  },
};

const WARNING_ONLY: ConflictWarning_Serialize = {
  severity: "warning",
  kind: {
    kind: "resource_contention",
    resource: "registry:hklm\\graphicsdrivers\\hwschmode",
    tweaks: ["a", "b"],
  },
};

function makeDiff(overrides: Partial<Diff> = {}): Diff {
  return {
    entries: [entry("modify")],
    warnings: [],
    requires_reboot: false,
    ...overrides,
  };
}

interface RenderOptions {
  diff?: Diff;
  applying?: boolean;
  onClose?: () => void;
  onConfirmApply?: () => void;
}

function renderModal({
  diff = makeDiff(),
  applying = false,
  onClose = vi.fn(),
  onConfirmApply = vi.fn(),
}: RenderOptions = {}) {
  render(
    <I18nProvider initialLocale="en">
      <DryRunModal
        diff={diff}
        applying={applying}
        error={null}
        onDismissError={vi.fn()}
        onClose={onClose}
        onConfirmApply={onConfirmApply}
      />
    </I18nProvider>,
  );
  return { onClose, onConfirmApply };
}

const confirmButton = () =>
  screen.getByRole("button", { name: /Confirm & Apply|Applying transaction/ });

describe("DryRunModal apply gating", () => {
  it("disables Confirm & Apply when a blocking warning is present", async () => {
    const user = userEvent.setup();
    const { onConfirmApply } = renderModal({
      diff: makeDiff({ warnings: [BLOCKING_WARNING] }),
    });

    expect(screen.getByRole("alert")).toHaveTextContent(
      /Apply is disabled because one or more blocking conflicts/,
    );
    expect(confirmButton()).toBeDisabled();

    await user.click(confirmButton());
    expect(onConfirmApply).not.toHaveBeenCalled();
  });

  it("allows Confirm & Apply when only non-blocking warnings exist", async () => {
    const user = userEvent.setup();
    const { onConfirmApply } = renderModal({
      diff: makeDiff({ warnings: [WARNING_ONLY] }),
    });

    expect(screen.queryByText(/Apply is disabled/)).not.toBeInTheDocument();
    expect(confirmButton()).toBeEnabled();

    await user.click(confirmButton());
    expect(onConfirmApply).toHaveBeenCalledTimes(1);
  });

  it("disables Confirm & Apply when the plan has no entries", () => {
    renderModal({ diff: makeDiff({ entries: [] }) });
    expect(screen.getByText("No actions in this plan.")).toBeInTheDocument();
    expect(confirmButton()).toBeDisabled();
  });

  it("disables both action buttons and shows progress while applying", () => {
    renderModal({ applying: true });
    expect(
      screen.getByRole("button", { name: "Applying transaction..." }),
    ).toBeDisabled();
    expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();
  });

  it("does not close on Escape while applying", async () => {
    const user = userEvent.setup();
    const busy = renderModal({ applying: true });
    await user.keyboard("{Escape}");
    expect(busy.onClose).not.toHaveBeenCalled();
  });

  it("closes on Escape when idle", async () => {
    const user = userEvent.setup();
    const idle = renderModal();
    await user.keyboard("{Escape}");
    expect(idle.onClose).toHaveBeenCalled();
  });
});

describe("DryRunModal notices", () => {
  it("shows the reboot notice only when requires_reboot is true", () => {
    const { unmount } = render(
      <I18nProvider initialLocale="en">
        <DryRunModal
          diff={makeDiff({ requires_reboot: true })}
          applying={false}
          error={null}
          onDismissError={vi.fn()}
          onClose={vi.fn()}
          onConfirmApply={vi.fn()}
        />
      </I18nProvider>,
    );
    expect(
      screen.getByRole("note"),
    ).toHaveTextContent(/require a system reboot/i);
    unmount();

    renderModal({ diff: makeDiff({ requires_reboot: false }) });
    expect(screen.queryByRole("note")).not.toBeInTheDocument();
  });

  it("renders blocking and warning severities with their labels", () => {
    renderModal({
      diff: makeDiff({ warnings: [BLOCKING_WARNING, WARNING_ONLY] }),
    });
    const list = screen.getByRole("list");
    expect(within(list).getByText("Blocking")).toBeInTheDocument();
    expect(within(list).getByText("Warning")).toBeInTheDocument();
    expect(
      within(list).getByText(/is excluded on detected environment: Virtual Machine/),
    ).toBeInTheDocument();
  });
});

describe("DryRunModal ChangeKind badges", () => {
  it("labels create, modify, delete and no_change entries", () => {
    renderModal({
      diff: makeDiff({
        entries: [
          entry("create", "t.create"),
          entry("modify", "t.modify"),
          entry("delete", "t.delete"),
          entry("no_change", "t.none"),
        ],
      }),
    });

    const table = screen.getByRole("table");
    const rows = within(table).getAllByRole("row").slice(1);
    const badges = rows.map((row) => within(row).getAllByRole("cell").at(-1));

    expect(badges.map((cell) => cell?.textContent)).toEqual([
      "Create",
      "Modify",
      "Delete",
      "No Change",
    ]);
    expect(badges[0]?.firstElementChild?.className).toContain("emerald");
    expect(badges[1]?.firstElementChild?.className).toContain("sky");
    expect(badges[2]?.firstElementChild?.className).toContain("rose");
  });

  it("reports total and effective change counts, excluding no_change", () => {
    renderModal({
      diff: makeDiff({
        entries: [entry("create", "a"), entry("no_change", "b"), entry("delete", "c")],
      }),
    });
    expect(
      screen.getByText("3 action(s) inspected · 2 effective change(s)"),
    ).toBeInTheDocument();
  });
});
