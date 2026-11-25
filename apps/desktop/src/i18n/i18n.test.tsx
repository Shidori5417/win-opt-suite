import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import en from "./en.json";
import tr from "./tr.json";
import { I18nProvider, interpolate, useI18n, type Locale } from "./index";

type Messages = Record<string, string>;

const enMessages: Messages = en;
const trMessages: Messages = tr;

const PLACEHOLDER_PATTERN = /\{(\w+)\}/g;

function placeholdersOf(template: string): string[] {
  return Array.from(template.matchAll(PLACEHOLDER_PATTERN), (m) => m[1] ?? "")
    .filter((name) => name.length > 0)
    .sort();
}

function Probe() {
  const { locale, setLocale, t } = useI18n();
  return (
    <button
      type="button"
      onClick={() => setLocale(locale === "en" ? "tr" : "en")}
    >
      {t("nav.overview")}
    </button>
  );
}

function renderProbe(initialLocale?: Locale) {
  return render(
    <I18nProvider initialLocale={initialLocale}>
      <Probe />
    </I18nProvider>,
  );
}

afterEach(() => {
  vi.restoreAllMocks();
  window.localStorage.clear();
});

describe("translation catalogs", () => {
  it("has exactly the same keys in en.json and tr.json", () => {
    expect(Object.keys(trMessages).sort()).toEqual(
      Object.keys(enMessages).sort(),
    );
  });

  it("keeps every placeholder of an English string in the Turkish string", () => {
    for (const key of Object.keys(enMessages)) {
      const enPlaceholders = placeholdersOf(enMessages[key] ?? "");
      const trPlaceholders = placeholdersOf(trMessages[key] ?? "");
      expect(trPlaceholders, `placeholders for ${key}`).toEqual(enPlaceholders);
    }
  });

  it("has no empty or whitespace-only strings in either locale", () => {
    for (const catalog of [enMessages, trMessages]) {
      for (const [key, value] of Object.entries(catalog)) {
        expect(value.trim(), key).not.toBe("");
      }
    }
  });

  it("uses the same locale-neutral placeholder names as the English source", () => {
    expect(placeholdersOf(trMessages["recovery.success"] ?? "")).toEqual([
      "id",
      "status",
    ]);
  });
});

describe("interpolate", () => {
  it("replaces known parameters and leaves unknown placeholders intact", () => {
    expect(interpolate("{a} and {b}", { a: "x", b: 2 })).toBe("x and 2");
    expect(interpolate("Hello {name}", {})).toBe("Hello {name}");
    expect(interpolate("no params")).toBe("no params");
  });
});

describe("language toggle", () => {
  it("switches locale, updates document language and persists to localStorage", async () => {
    const user = userEvent.setup();
    renderProbe("en");

    const button = screen.getByRole("button", { name: "System Overview" });
    await user.click(button);

    expect(screen.getByRole("button", { name: "Sistem Özeti" })).toBeVisible();
    expect(document.documentElement.lang).toBe("tr");
    expect(window.localStorage.getItem("winoptsuite.locale")).toBe("tr");
  });

  it("restores the persisted locale after a reload (re-mount)", async () => {
    const user = userEvent.setup();
    const first = renderProbe("en");
    await user.click(screen.getByRole("button", { name: "System Overview" }));
    first.unmount();

    renderProbe();

    expect(screen.getByRole("button", { name: "Sistem Özeti" })).toBeVisible();
  });

  it("ignores an invalid persisted locale and falls back to English", () => {
    window.localStorage.setItem("winoptsuite.locale", "de");
    renderProbe();
    expect(screen.getByRole("button", { name: "System Overview" })).toBeVisible();
  });

  it("does not crash when localStorage throws on read or write", async () => {
    const user = userEvent.setup();
    vi.spyOn(window.localStorage, "getItem").mockImplementation(() => {
      throw new DOMException("denied", "SecurityError");
    });
    const setSpy = vi
      .spyOn(window.localStorage, "setItem")
      .mockImplementation(() => {
        throw new DOMException("quota", "QuotaExceededError");
      });

    renderProbe();
    expect(screen.getByRole("button", { name: "System Overview" })).toBeVisible();

    await act(async () => {
      await user.click(screen.getByRole("button", { name: "System Overview" }));
    });

    expect(setSpy).toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Sistem Özeti" })).toBeVisible();
  });
});
