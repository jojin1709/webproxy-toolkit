import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Transfer } from "../types";

const OPS: [string, string][] = [
  ["url-encode", "URL encode"],
  ["url-decode", "URL decode"],
  ["b64-encode", "Base64 encode"],
  ["b64-decode", "Base64 decode"],
  ["hex-encode", "Hex encode"],
  ["hex-decode", "Hex decode"],
  ["html-encode", "HTML encode"],
  ["html-decode", "HTML decode"],
  ["md5", "MD5"],
  ["sha1", "SHA-1"],
  ["sha256", "SHA-256"],
  ["sha512", "SHA-512"],
];

export default function DecoderPanel({
  setToast,
  incoming,
}: {
  setToast: (t: string) => void;
  incoming?: Transfer | null;
}) {
  const [input, setInput] = useState("");
  const [output, setOutput] = useState("");
  const [op, setOp] = useState("url-decode");
  const [err, setErr] = useState("");

  useEffect(() => {
    if (incoming && incoming.seq) {
      setInput(incoming.raw);
      setOutput("");
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [incoming?.seq]);

  async function apply() {
    setErr("");
    try {
      if (["md5", "sha1", "sha256", "sha512"].includes(op)) {
        setOutput(await invoke<string>("hash_compute", { alg: op, data: input }));
        return;
      }
      setOutput(runOp(input, op));
    } catch (e) {
      setErr(String(e));
      setOutput("");
    }
  }

  async function smartDecode() {
    setErr("");
    let cur = input;
    for (const candidate of ["url-decode", "b64-decode", "hex-decode", "html-decode"]) {
      try {
        const next = runOp(cur, candidate);
        if (next !== cur) {
          cur = next;
        }
      } catch {
        /* try next */
      }
    }
    setOutput(cur);
    if (cur === input) setToast("No transform changed the value");
  }

  return (
    <div className="panel" role="tabpanel" aria-label="Decoder" id="panel-decoder">
      <div className="ops" role="toolbar" aria-label="Transform">
        {OPS.map(([id, label]) => (
          <button
            key={id}
            className={`op-chip ${op === id ? "active" : ""}`}
            onClick={() => setOp(id)}
          >
            {label}
          </button>
        ))}
        <span className="grow" />
        <button className="btn" onClick={() => setInput(output)} disabled={!output}>
          Output → Input
        </button>
        <button className="btn" onClick={smartDecode}>
          Smart decode
        </button>
        <button className="btn primary" onClick={apply}>
          Apply
        </button>
      </div>
      <div className="split">
        <div className="pane">
          <div className="pane-title">Input</div>
          <div className="pane-body">
            <textarea
              value={input}
              onChange={(e) => setInput(e.target.value)}
              spellCheck={false}
              placeholder="Paste a value to encode, decode, or hash."
              aria-label="Decoder input"
            />
          </div>
        </div>
        <div className="pane">
          <div className="pane-title">
            Output {err && <span className="chip err">{err}</span>}
          </div>
          <div className="pane-body">
            <textarea value={output} readOnly spellCheck={false} aria-label="Decoder output" />
          </div>
        </div>
      </div>
    </div>
  );
}

function runOp(input: string, op: string): string {
  switch (op) {
    case "url-encode":
      return encodeURIComponent(input);
    case "url-decode":
      try {
        return decodeURIComponent(input.replace(/\+/g, " "));
      } catch {
        throw new Error("Not valid URL encoding");
      }
    case "b64-encode":
      return btoa(unescape(encodeURIComponent(input)));
    case "b64-decode":
      try {
        return decodeURIComponent(escape(atob(input.trim())));
      } catch {
        throw new Error("Not valid Base64");
      }
    case "hex-encode":
      return Array.from(new TextEncoder().encode(input))
        .map((b) => b.toString(16).padStart(2, "0"))
        .join("");
    case "hex-decode": {
      const clean = input.trim().replace(/\s+/g, "");
      if (!/^[0-9a-fA-F]*$/.test(clean) || clean.length % 2 !== 0) {
        throw new Error("Not valid hex");
      }
      const bytes = new Uint8Array(clean.length / 2);
      for (let i = 0; i < bytes.length; i++) {
        bytes[i] = parseInt(clean.substr(i * 2, 2), 16);
      }
      return new TextDecoder().decode(bytes);
    }
    case "html-encode":
      return input
        .replace(/&/g, "&amp;")
        .replace(/</g, "&lt;")
        .replace(/>/g, "&gt;")
        .replace(/"/g, "&quot;");
    case "html-decode": {
      const el = document.createElement("textarea");
      el.innerHTML = input;
      return el.value;
    }
    default:
      throw new Error("Unknown op");
  }
}
