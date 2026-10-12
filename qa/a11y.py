import json
import pathlib
import sys

from playwright.sync_api import sync_playwright

sys.stdout.reconfigure(encoding="utf-8", errors="replace")

URL = "http://localhost:1420"
MOCK_PATH = pathlib.Path(r"D:\webproxy-toolkit\qa\mock.js")
AXE_PATH = pathlib.Path(r"D:\webproxy-toolkit\app\node_modules\axe-core\axe.min.js")
TAGS = ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"]

if not AXE_PATH.exists():
    print(f"axe-core not found at {AXE_PATH}")
    sys.exit(2)

problems = []


def new_page(browser, scenario=None, viewport=(1440, 900)):
    ctx = browser.new_context(viewport={"width": viewport[0], "height": viewport[1]})
    page = ctx.new_page()
    page.add_init_script("window.__SCENARIO__ = " + json.dumps(scenario or {}) + ";")
    page.add_init_script(path=str(MOCK_PATH))
    page.goto(URL, wait_until="load", timeout=20000)
    page.wait_for_selector(".tooltabs", timeout=10000)
    page.wait_for_timeout(700)
    return ctx, page


def tab(page, name):
    page.locator(".tooltabs .tab", has_text=name).first.click()
    page.wait_for_timeout(400)


def sub(page, name):
    page.locator(".subtabs button", has_text=name).first.click()
    page.wait_for_timeout(400)


def audit(page, label, expect_open=None):
    page.add_script_tag(path=str(AXE_PATH))
    result = page.evaluate(
        """async (tags) => {
            const r = await axe.run(document, { runOnly: { type: 'tag', values: tags } });
            return {
                violations: r.violations.map(v => ({
                    id: v.id, impact: v.impact, help: v.help,
                    nodes: v.nodes.slice(0, 4).map(n => ({ html: n.html, summary: n.failureSummary })),
                    count: v.nodes.length,
                })),
                incomplete: r.incomplete
                    .filter(v => v.id === "color-contrast")
                    .flatMap(v => v.nodes.slice(0, 6).map(n => ({ id: v.id, html: n.html }))),
            };
        }""",
        TAGS,
    )
    viol = result["violations"]
    print(f"[{label}] violations={len(viol)}")
    for v in viol:
        print(f"    {v['impact']}: {v['id']} x{v['count']} — {v['help']}")
        for n in v["nodes"]:
            print(f"        {n['html'][:160]}")
            if n["summary"]:
                print(f"        {n['summary'].splitlines()[0][:160]}")
    for inc in result.get("incomplete", []):
        print(f"    (incomplete color-contrast) {inc['html'][:140]}")
    if expect_open:
        for vid in expect_open:
            if not any(v["id"] == vid for v in viol):
                print(f"    expected violation missing (already fixed?): {vid}")
    problems.extend((label, v["id"]) for v in viol)


def run():
    with sync_playwright() as p:
        browser = p.chromium.launch()

        ctx, page = new_page(browser)
        audit(page, "dashboard")
        tab(page, "Proxy")
        audit(page, "proxy-history")
        sub(page, "WebSockets")
        audit(page, "proxy-websockets")
        sub(page, "Intercept")
        audit(page, "proxy-intercept")
        sub(page, "HTTP history")
        tab(page, "Repeater")
        page.locator(".btn.send").click()
        page.wait_for_timeout(500)
        audit(page, "repeater-after-send")
        tab(page, "Logger")
        audit(page, "logger")
        tab(page, "Dashboard")
        audit(page, "dashboard-eventlog")

        # search dialog (combobox/listbox/selected hit contrast)
        tab(page, "Proxy")
        page.locator('button[aria-label="Search"]').click()
        page.wait_for_timeout(300)
        page.locator(".searchdlg input").fill("cart")
        page.wait_for_timeout(600)
        audit(page, "search-dialog")
        # arrow down so a hit row is selected (selected/active contrast)
        page.locator(".searchdlg input").press("ArrowDown")
        page.wait_for_timeout(250)
        audit(page, "search-dialog-selected")
        page.keyboard.press("Escape")
        ctx.close()

        # narrow viewport (statusbar wrapping, tooltabs scrolling)
        ctx, page = new_page(browser, viewport=(390, 780))
        audit(page, "narrow-390-dashboard")
        tab(page, "Proxy")
        audit(page, "narrow-390-proxy")
        page.locator('button[aria-label="Search"]').click()
        page.wait_for_timeout(300)
        page.locator(".searchdlg input").fill("cart")
        page.wait_for_timeout(600)
        audit(page, "narrow-390-search")
        ctx.close()

        browser.close()

    if problems:
        print(f"\n{len(problems)} violation group(s)")
        sys.exit(1)
    print("\nno violations")


if __name__ == "__main__":
    run()
