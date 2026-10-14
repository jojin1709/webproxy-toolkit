import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

export default function SettingsPanel({ setToast }: { setToast: (t: string) => void }) {
  const [caPath, setCaPath] = useState("");

  useEffect(() => {
    invoke<string>("init_ca")
      .then(setCaPath)
      .catch((e) => setToast(String(e)));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <div className="panel panel-overflow" role="tabpanel" aria-label="Settings" id="panel-settings">
      <div className="card">
        <h2>HTTPS certificate</h2>
        <p className="muted">
          Export the local CA (<code className="mono">ca.crt</code>) and trust it in your browser or
          OS to intercept HTTPS. This app never modifies the trust store automatically.
        </p>
        <div className="row">
          <code className="mono grow ellipsis">
            {caPath || "…"}
          </code>
          <button
            className="btn"
            onClick={() =>
              invoke<string>("init_ca").then((p) => {
                setCaPath(p);
                setToast("CA ready");
              })
            }
          >
            Locate / regenerate
          </button>
        </div>
      </div>
      <div className="card">
        <h2>Project</h2>
        <p className="muted">
          History, scope, match/replace rules and Repeater tabs persist to{" "}
          <code className="mono">project.wpt</code> (SQLite) in the app data folder.
        </p>
      </div>
      <div className="card">
        <h2>Keyboard shortcuts</h2>
        <table className="history">
          <tbody>
            <tr>
              <td className="mono">Ctrl+Shift+F</td>
              <td>Global search (history + log)</td>
            </tr>
            <tr>
              <td className="mono">Ctrl+1 … Ctrl+9, Ctrl+0</td>
              <td>Switch tool tab (Dashboard, Target, Proxy, … Extensions)</td>
            </tr>
            <tr>
              <td className="mono">Esc</td>
              <td>Close dialogs and menus</td>
            </tr>
          </tbody>
        </table>
      </div>
      <div className="card">
        <h2>About</h2>
        <p className="muted">
          WebProxy Toolkit — local testing proxy. No accounts, no license server, no telemetry.
        </p>
      </div>
    </div>
  );
}
