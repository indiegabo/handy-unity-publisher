import { expect, test } from "@playwright/test";

test.describe("desktop UI shell", () => {
    test("opens and dismisses the worker quick-view overlay", async ({ page }) => {
        await page.goto("/");

        const workerTrigger = page.getByRole("button", {
            name: /Project workers active/i,
        });

        await expect(workerTrigger).toBeVisible();
        await workerTrigger.click();

        await expect(
            page.getByRole("dialog", { name: "Project Workers" }),
        ).toBeVisible();

        await page.keyboard.press("Escape");

        await expect(
            page.getByRole("dialog", { name: "Project Workers" }),
        ).toHaveCount(0);
        await expect(workerTrigger).toBeFocused();
        await expect(page.getByRole("button", { name: "Projetos" })).toBeVisible();
        await expect(
            page.getByRole("button", { name: "Criar novo projeto" }),
        ).toBeVisible();
    });

    test("navigates from the main feed to the project list", async ({ page }) => {
        await page.goto("/");

        await page.getByRole("button", { name: "Projetos" }).click();

        await expect(
            page.getByRole("heading", { name: "Project List" }),
        ).toBeVisible();
        await expect(page.getByText("Indie Demo Repository")).toBeVisible();
    });

    test("closes the project picker on Escape without leaving the project list", async ({ page }) => {
        await page.goto("/");

        await page.getByRole("button", { name: "Projetos" }).click();

        await expect(
            page.getByRole("heading", { name: "Project List" }),
        ).toBeVisible();

        const browseButton = page.getByRole("button", { name: "Browse" });
        await browseButton.click();

        await expect(
            page.getByRole("dialog", { name: "Open project" }),
        ).toBeVisible();

        await page.keyboard.press("Escape");

        await expect(
            page.getByRole("dialog", { name: "Open project" }),
        ).toHaveCount(0);
        await expect(
            page.getByRole("heading", { name: "Project List" }),
        ).toBeVisible();
        await expect(browseButton).toBeFocused();
    });

    test("returns from the project list to the main feed when Back is pressed without an overlay", async ({ page }) => {
        await page.goto("/");

        await page.getByRole("button", { name: "Projetos" }).click();

        await expect(
            page.getByRole("heading", { name: "Project List" }),
        ).toBeVisible();

        await page
            .getByRole("button", { name: "Voltar para a tela principal" })
            .click();

        await expect(
            page.getByRole("button", { name: "Criar novo projeto" }),
        ).toBeVisible();
        await expect(page.getByRole("button", { name: "Projetos" })).toBeVisible();
        await expect(page.getByRole("button", { name: "Auth" })).toBeVisible();
        await expect(page.getByRole("button", { name: "Settings" })).toBeVisible();
        await expect(
            page.getByRole("button", { name: /Project workers active/i }),
        ).toBeVisible();
        await expect(
            page.getByRole("heading", { name: "Project List" }),
        ).toHaveCount(0);
    });
});