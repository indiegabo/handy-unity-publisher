import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { RepositoryEngineField } from "./RepositoryEngineField";

describe("RepositoryEngineField", () => {
  it("keeps future engines visible but disabled", () => {
    render(<RepositoryEngineField onChange={() => undefined} value="unity" />);

    expect(screen.getByRole("combobox", { name: /Engine/ })).toHaveValue(
      "unity",
    );
    expect(screen.getByRole("option", { name: "Unity" })).toBeEnabled();
    expect(screen.getByRole("option", { name: "Unreal" })).toBeDisabled();
    expect(screen.getByRole("option", { name: "Godot" })).toBeDisabled();
    expect(screen.getByRole("option", { name: "GameMaker" })).toBeDisabled();
    expect(screen.getByRole("option", { name: "Defold" })).toBeDisabled();
    expect(
      screen.getByRole("option", { name: "Cocos Creator" }),
    ).toBeDisabled();
  });
});