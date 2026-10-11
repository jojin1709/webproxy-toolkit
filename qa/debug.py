import pathlib
from playwright.sync_api import sync_playwright

MOCK = pathlib.Path(r"D:\webproxy-toolkit\qa\mock.js")

with sync_playwright() as p:
    b = p.chromium.launch()
    ctx = b.new_context(viewport={"width": 1440, "height": 900})
    pg = ctx.new_page()
    pg.on("console", lambda m: print("CONSOLE", m.type, m.text))
    pg.on("pageerror", lambda e: print("PAGEERROR", e))
    pg.add_init_script("window.__SCENARIO__ = {};")
    pg.add_init_script(path=str(MOCK))
    pg.goto("http://localhost:1420", wait_until="load")
    pg.wait_for_selector(".tooltabs")
    pg.wait_for_timeout(700)
    btns = pg.locator(".subtabs button").all()
    print("count:", len(btns))
    for x in btns:
        print(" -", repr(x.inner_text()), "disabled=", x.is_disabled())
    pg.locator(".subtabs button", has_text="Intercept").first.click()
    pg.wait_for_timeout(400)
    pg.get_by_role("button", name="Turn Intercept on").click()
    pg.wait_for_timeout(500)
    btns = pg.locator(".subtabs button").all()
    print("after intercept-on count:", len(btns))
    for x in btns:
        print(" -", repr(x.inner_text()), "disabled=", x.is_disabled())
    print("toasts:", pg.locator(".status-toast").all_inner_texts())
    b.close()
