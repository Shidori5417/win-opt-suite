import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { App } from "./App";

describe("Desktop shell scaffold", () => {
  it("renders shell heading", () => {
    render(<App />);
    expect(screen.getByRole("heading", { name: "WinOptSuite" })).toBeInTheDocument();
  });
});
