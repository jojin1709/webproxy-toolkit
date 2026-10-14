import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { CollabInteraction, CollabPayload, CollabStatus } from "../types";

export default function CollaboratorPanel({ setToast }: { setToast: (t: string) => void }) {
  const [status, setStatus] = useState<CollabStatus>({
    running: false,
    dns_port: 5354,
    http_port: 8081,
    domain: "oob.wpt.local",
    bind_ip: "127.0.0.1",
  });
  const [domain, setDomain] = useState("oob.wpt.local");
  const [bindIp, setBindIp] = useState("127.0.0.1");
  const [dnsPort, setDnsPort] = useState(5354);
  const [httpPort, setHttpPort] = useState(8081);
  const [payloads, setPayloads] = useState<CollabPayload[]>([]);
  const [interactions, setInteractions] = useState<CollabInteraction[]>([]);
  const [selected, setSelected] = useState<number | null>(null);
  const [kind, setKind] = useState("url");
  const [label, setLabel] = useState("");
  const [err, setErr] = useState("");

  const refreshStatus = useCallback(async () => {
    try {
      setStatus(await invoke<CollabStatus>("collab_status"));
    } catch {
      /* mock or older backend */
    }
  }, []);

  const refreshPayloads = useCallback(async () => {
    try {
      setPayloads(await invoke<CollabPayload[]>("collab_payload_list"));
    } catch (e) {
      setErr(String(e));
    }
  }, []);

  const refreshInteractions = useCallback(async (pid: number | null) => {
    try {
      setInteractions(
        await invoke<CollabInteraction[]>("collab_interactions_list", { payloadId: pid })
      );
    } catch (e) {
      setErr(String(e));
    }
  }, []);

  useEffect(() => {
    void refreshStatus();
    void refreshPayloads();
    void refreshInteractions(null);
  }, [refreshStatus, refreshPayloads, refreshInteractions]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    listen<CollabInteraction>("collab-interaction", (e) => {
      setInteractions((prev) => {
        // respect the selected-payload filter
        if (selected != null && e.payload.payload_id !== selected) return prev;
        return [e.payload, ...prev].slice(0, 200);
      });
    }).then((f) => {
      unlisten = f;
    });
    return () => unlisten?.();
  }, [selected]);

  async function start() {
    setErr("");
    try {
      const s = await invoke<CollabStatus>("collab_start", {
        domain,
        bindIp,
        dnsPort,
        httpPort,
      });
      setStatus(s);
      setToast("Collaborator server started");
    } catch (e) {
      setErr(String(e));
    }
  }

  async function stop() {
    setErr("");
    try {
      await invoke("collab_stop");
      await refreshStatus();
      setToast("Collaborator server stopped");
    } catch (e) {
      setErr(String(e));
    }
  }

  async function generate() {
    setErr("");
    try {
      const p = await invoke<CollabPayload>("collab_new_payload", { kind, label: label || null });
      setPayloads((prev) => [p, ...prev]);
      setLabel("");
      setToast("Payload generated");
    } catch (e) {
      setErr(String(e));
    }
  }

  async function removePayload(id: number) {
    try {
      await invoke("collab_delete_payload", { id });
      setPayloads((prev) => prev.filter((p) => p.id !== id));
      if (selected === id) {
        setSelected(null);
        void refreshInteractions(null);
      }
    } catch (e) {
      setErr(String(e));
    }
  }

  async function selectPayload(id: number | null) {
    setSelected(id);
    void refreshInteractions(id);
  }

  async function copyValue(v: string) {
    navigator.clipboard
      .writeText(v)
      .then(() => setToast("Payload copied"))
      .catch(() => setToast("Clipboard unavailable"));
  }

  async function clearInteractions() {
    try {
      await invoke("collab_clear_interactions");
      void refreshInteractions(selected);
    } catch (e) {
      setErr(String(e));
    }
  }

  function detailSummary(i: CollabInteraction): string {
    const d = i.detail ?? {};
    if (i.protocol === "dns") {
      const name = typeof d.name === "string" ? d.name : i.subdomain;
      const qtype = d.qtype === 1 ? "A" : d.qtype === 28 ? "AAAA" : String(d.qtype ?? "");
      return `${name} ${qtype}`;
    }
    const method = typeof d.method === "string" ? d.method : "";
    const path = typeof d.path === "string" ? d.path : "";
    return `${method} ${path}`;
  }

  return (
    <div className="panel" role="tabpanel" aria-label="Collaborator" id="panel-collaborator">
      <div className="toolbar" style={{ flexWrap: "wrap", gap: 8 }}>
        <label style={{ display: "flex", gap: 4, alignItems: "center" }}>
          <span style={{ fontSize: 11, opacity: 0.8 }}>Domain</span>
          <input
            className="input"
            value={domain}
            onChange={(e) => setDomain(e.target.value)}
            aria-label="Collaborator domain"
            style={{ width: 160 }}
          />
        </label>
        <label style={{ display: "flex", gap: 4, alignItems: "center" }}>
          <span style={{ fontSize: 11, opacity: 0.8 }}>Bind IP</span>
          <input
            className="input"
            value={bindIp}
            onChange={(e) => setBindIp(e.target.value)}
            aria-label="Bind IP"
            style={{ width: 110 }}
          />
        </label>
        <label style={{ display: "flex", gap: 4, alignItems: "center" }}>
          <span style={{ fontSize: 11, opacity: 0.8 }}>DNS</span>
          <input
            className="input"
            type="number"
            value={dnsPort}
            onChange={(e) => setDnsPort(Number(e.target.value))}
            aria-label="DNS port"
            style={{ width: 70 }}
          />
        </label>
        <label style={{ display: "flex", gap: 4, alignItems: "center" }}>
          <span style={{ fontSize: 11, opacity: 0.8 }}>HTTP</span>
          <input
            className="input"
            type="number"
            value={httpPort}
            onChange={(e) => setHttpPort(Number(e.target.value))}
            aria-label="HTTP port"
            style={{ width: 70 }}
          />
        </label>
        {status.running ? (
          <button className="btn" onClick={() => void stop()}>
            Stop server
          </button>
        ) : (
          <button className="btn primary" onClick={() => void start()}>
            Start server
          </button>
        )}
        <span className={`chip ${status.running ? "ok" : ""}`}>
          {status.running ? `running :${status.dns_port}/:${status.http_port}` : "stopped"}
        </span>
      </div>

      {err && (
        <div style={{ padding: "8px 12px" }} role="alert">
          <span className="chip err">{err}</span>
        </div>
      )}

      <div className="toolbar" style={{ gap: 8 }}>
        <label style={{ display: "flex", gap: 4, alignItems: "center" }}>
          <span className="sr-only">Payload kind</span>
          <select
            className="input"
            value={kind}
            onChange={(e) => setKind(e.target.value)}
            aria-label="Payload kind"
          >
            <option value="url">URL</option>
            <option value="html">HTML img</option>
            <option value="host">Hostname</option>
          </select>
        </label>
        <input
          className="input grow"
          placeholder="Label (optional)"
          value={label}
          onChange={(e) => setLabel(e.target.value)}
          aria-label="Payload label"
        />
        <button className="btn primary" onClick={() => void generate()}>
          Generate payload
        </button>
        <button className="btn" onClick={() => void clearInteractions()}>
          Clear interactions
        </button>
      </div>

      <div className="table-wrap" role="region" aria-label="Payloads" tabIndex={0} style={{ maxHeight: 180 }}>
        <table className="history" aria-label="Collaborator payloads">
          <thead>
            <tr>
              <th scope="col">Label</th>
              <th scope="col">Kind</th>
              <th scope="col">Value</th>
              <th scope="col">Created</th>
              <th scope="col">Actions</th>
            </tr>
          </thead>
          <tbody>
            {payloads.map((p) => (
              <tr
                key={p.id}
                className={selected === p.id ? "selected" : ""}
                onClick={() => void selectPayload(p.id)}
              >
                <td>{p.label || "—"}</td>
                <td>{p.kind}</td>
                <td className="mono" style={{ maxWidth: 320, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                  {p.value}
                </td>
                <td>{p.created_at}</td>
                <td>
                  <button
                    className="btn"
                    style={{ marginRight: 6 }}
                    onClick={(e) => {
                      e.stopPropagation();
                      void copyValue(p.value);
                    }}
                  >
                    Copy
                  </button>
                  <button
                    className="btn"
                    onClick={(e) => {
                      e.stopPropagation();
                      void removePayload(p.id);
                    }}
                  >
                    Remove
                  </button>
                </td>
              </tr>
            ))}
            {payloads.length === 0 && (
              <tr>
                <td colSpan={5} style={{ opacity: 0.6, padding: 12 }}>
                  No payloads yet — generate one above.
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>

      <div className="table-wrap grow min-h-0" role="region" aria-label="Interactions" tabIndex={0}>
        <table className="history" aria-label="Collaborator interactions">
          <thead>
            <tr>
              <th scope="col">Time</th>
              <th scope="col">Protocol</th>
              <th scope="col">Source</th>
              <th scope="col">Detail</th>
            </tr>
          </thead>
          <tbody>
            {interactions.map((i) => (
              <tr key={i.id}>
                <td>{i.seen_at}</td>
                <td>
                  <span className="chip">{i.protocol}</span>
                </td>
                <td>{i.source}</td>
                <td className="mono">{detailSummary(i)}</td>
              </tr>
            ))}
            {interactions.length === 0 && (
              <tr>
                <td colSpan={4} style={{ opacity: 0.6, padding: 12 }}>
                  No interactions yet{selected != null ? " for this payload" : ""}. Point a lab
                  target at {status.bind_ip}:{status.dns_port} and trigger an OOB request.
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
    </div>
  );
}
