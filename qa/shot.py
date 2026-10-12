import json
import pathlib
import sys

from playwright.sync_api import sync_playwright

OUT = pathlib.Path(r"D:\webproxy-toolkit\qa\shots")
OUT.mkdir(parents=True, exist_ok=True)
URL = "http://localhost:1420"
MOCK_PATH = pathlib.Path(r"D:\webproxy-toolkit\qa\mock.js")

errors = []
stage = ["init"]


def new_page(browser, scenario=None, viewport=(1440, 900)):
    ctx = browser.new_context(viewport={"width": viewport[0], "height": viewport[1]})
    page = ctx.new_page()
    page.add_init_script("window.__SCENARIO__ = " + json.dumps(scenario or {}) + ";")
    page.add_init_script(path=str(MOCK_PATH))
    page.on(
        "console",
        lambda m: errors.append(f"{stage[0]}: [console.{m.type}] {m.text}")
        if m.type in ("error", "warning")
        else None,
    )
    page.on("pageerror", lambda e: errors.append(f"{stage[0]}: [pageerror] {e}"))
    page.goto(URL, wait_until="load", timeout=20000)
    page.wait_for_selector(".tooltabs", timeout=10000)
    page.wait_for_timeout(700)
    return ctx, page


def tab(page, name):
    stage[0] = f"tab:{name}"
    page.locator(".tooltabs .tab", has_text=name).first.click()
    page.wait_for_timeout(450)


def sub(page, name):
    stage[0] = f"sub:{name}"
    page.locator(".subtabs button", has_text=name).first.click()
    page.wait_for_timeout(450)


def shot(page, name):
    stage[0] = f"shot:{name}"
    page.screenshot(path=str(OUT / f"{name}.png"))
    print(f"  saved {name}.png")


def run():
    with sync_playwright() as p:
        browser = p.chromium.launch()

        # ---- Group A: standard data, desktop ----
        ctx, page = new_page(browser)
        tab(page, "Dashboard"); shot(page, "01-dashboard")
        tab(page, "Target"); shot(page, "02-target-sitemap")
        sub(page, "Scope"); shot(page, "03-target-scope")
        tab(page, "Proxy"); shot(page, "04-proxy-history")
        rows = page.locator(".history tbody tr")
        if rows.count() > 2:
            stage[0] = "select row"
            rows.nth(2).click()
            page.wait_for_timeout(500)
            shot(page, "05-proxy-history-detail")
            stage[0] = "context menu"
            rows.nth(4).click(button="right")
            page.wait_for_timeout(300)
            shot(page, "06-proxy-history-ctx")
            page.keyboard.press("Escape")
            page.wait_for_timeout(200)
        sub(page, "Intercept"); shot(page, "07-proxy-intercept-off")
        try:
            stage[0] = "intercept on"
            page.get_by_role("button", name="Turn Intercept on").click()
            page.wait_for_timeout(400)
            shot(page, "08-proxy-intercept-on")
        except Exception as e:
            errors.append(f"intercept-on click failed: {e}")
        stage[0] = "back to history"
        page.locator(".subtabs button", has_text="HTTP history").first.click()
        page.wait_for_timeout(450)
        sub(page, "Options"); shot(page, "09-proxy-options")
        tab(page, "Repeater"); shot(page, "10-repeater")
        try:
            stage[0] = "repeater send"
            page.locator(".btn.send").click()
            page.wait_for_timeout(600)
            shot(page, "11-repeater-send")
        except Exception as e:
            errors.append(f"repeater send failed: {e}")
        tab(page, "Decoder"); shot(page, "12-decoder")
        stage[0] = "settings"
        page.locator('button[title="Settings"]').click()
        page.wait_for_timeout(450)
        shot(page, "13-settings")
        tab(page, "Intruder"); shot(page, "14-placeholder")

        # ---- Group A2: Phase 2a tools ----
        tab(page, "Proxy")
        stage[0] = "stage comparer 1"
        page.locator(".history tbody tr").nth(0).click(button="right")
        page.wait_for_timeout(250)
        page.get_by_role("menuitem", name="Send to Comparer (request)").click()
        page.wait_for_timeout(400)
        stage[0] = "stage comparer 2"
        tab(page, "Proxy")
        page.locator(".history tbody tr").nth(2).click(button="right")
        page.wait_for_timeout(250)
        page.get_by_role("menuitem", name="Send to Comparer (request)").click()
        page.wait_for_timeout(400)
        shot(page, "20-comparer")

        stage[0] = "sequencer paste"
        tab(page, "Sequencer")
        page.locator(".segmented button", has_text="Paste tokens").first.click()
        page.wait_for_timeout(250)
        page.locator(".seq-paste").fill(
            "aZ3kQ9pLmX7vB2nR\nwE5tY8uI1oP4aS6d\nfG0hJ2kL5zX8cV9b\nnM3qW6eR9tY0uI4o\n"
            "pA7sD0fG3hJ6kL1z\nxC9vB2nM5qW8eR4t\nyU6iO9pA2sD5fG8h\njK1lZ4xC7vB0nM3q\n"
            "W5eR8tY1uI4oP7a\nS2dF5gH8jK0lZ3x\nC6vB9nM2qW5eR8t\nY1uI4oP7aS0dF3g\n"
            "H5jK8lZ1xC4vB7n\nM0qW3eR6tY9uI2o\nP5aS8dF1gH4jK7l\nZ0xC3vB6nM9qW2e\n"
            "R5tY8uI1oP4aS7d\nF0gH3jK6lZ9xC2v\nB5nM8qW1eR4tY7u\nI0oP3aS6dF9gH2j"
        )
        page.get_by_role("button", name="Analyze").click()
        page.wait_for_timeout(500)
        shot(page, "21-sequencer")

        tab(page, "Logger"); shot(page, "22-logger")

        stage[0] = "dashboard event log"
        tab(page, "Dashboard")
        page.locator(".subtabs button", has_text="Event log").first.click()
        page.wait_for_timeout(500)
        shot(page, "23-dashboard-eventlog")
        ctx.close()

        # ---- Group A3: Phase 2b (WS history, search, action menu, repeater history) ----
        ctx, page = new_page(browser)
        tab(page, "Proxy")
        sub(page, "WebSockets")
        stage[0] = "select ws conn"
        conns = page.locator('div[aria-label="WebSocket connections"] tbody tr')
        if conns.count() > 0:
            conns.first.click()
            page.wait_for_timeout(400)
            shot(page, "24-proxy-ws")
            stage[0] = "select ws frame"
            frames = page.locator('div[aria-label="WebSocket frames"] tbody tr')
            if frames.count() > 2:
                frames.nth(2).click()
                page.wait_for_timeout(350)
                shot(page, "25-proxy-ws-frame")
            stage[0] = "ws handshake"
            page.locator('button[title="Show the reconstructed HTTP upgrade handshake"]').click()
            page.wait_for_timeout(350)
            shot(page, "26-proxy-ws-handshake")
        else:
            errors.append("no websocket connections rendered")

        stage[0] = "global search"
        page.locator('button[aria-label="Search"]').click()
        page.wait_for_timeout(350)
        page.locator(".searchdlg input").fill("cart")
        page.wait_for_timeout(600)
        shot(page, "27-search-dialog")
        hits = page.locator(".searchdlg-hit")
        if hits.count() == 0:
            errors.append("global search returned no hits")
        else:
            stage[0] = "search jump"
            page.locator(".searchdlg input").press("Enter")
            page.wait_for_timeout(600)
            shot(page, "28-search-jumped")
            selected = page.locator(".history tbody tr.selected")
            if selected.count() == 0:
                errors.append("search jump did not select a history row")

        stage[0] = "repeater follow + history"
        tab(page, "Repeater")
        page.locator('label.switch', has_text="Follow redirects").click()
        page.locator(".btn.send").click()
        page.wait_for_timeout(700)
        shot(page, "29-repeater-redirects")
        # a second send gives the tab a previous entry to step back to
        stage[0] = "repeater second send"
        page.locator(".btn.send").click()
        page.wait_for_timeout(700)
        older = page.locator('button[title^="Previous request"]')
        if older.count() and not older.first.is_disabled():
            older.first.click()
            page.wait_for_timeout(400)
            shot(page, "30-repeater-history-prev")
        else:
            errors.append("repeater history prev button stayed disabled after send")

        tab(page, "Logger"); shot(page, "31-logger-search-source")
        ctx.close()

        # ---- Group B: empty site map ----
        ctx, page = new_page(browser, {"sitemapEmpty": True})
        tab(page, "Target"); shot(page, "15-target-empty")
        ctx.close()

        # ---- Group C: intercept holding a message ----
        ctx, page = new_page(browser, {"intercept": True, "held": True})
        sub(page, "Intercept"); shot(page, "16-proxy-held")
        stage[0] = "action menu"
        page.locator('button[aria-label="Action"]').click()
        page.wait_for_timeout(350)
        shot(page, "32-intercept-action-menu")
        stage[0] = "action menu open in repeater"
        try:
            page.get_by_role("menuitem", name="Open in Repeater").click()
            page.wait_for_timeout(500)
            shot(page, "33-action-open-in-repeater")
        except Exception as e:
            errors.append(f"action menu open-in-repeater failed: {e}")
        ctx.close()

        # ---- Group D: narrow viewport ----
        ctx, page = new_page(browser, viewport=(1000, 720))
        tab(page, "Dashboard"); shot(page, "17-narrow-dashboard")
        tab(page, "Proxy"); shot(page, "18-narrow-proxy")
        ctx.close()

        # ---- Group E: listener stopped ----
        ctx, page = new_page(browser, {"stopped": True})
        shot(page, "19-stopped-statusbar")
        ctx.close()

        browser.close()

    print("\n--- console/page errors ---")
    if errors:
        for e in errors:
            print(" ", e)
        sys.exit(1)
    print("  none")


if __name__ == "__main__":
    run()
