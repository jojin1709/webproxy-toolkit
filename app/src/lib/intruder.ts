import { invoke } from "@tauri-apps/api/core";
import {
  IntruderAttack,
  IntruderCampaignDetail,
  IntruderCampaignSummary,
  IntruderResultFull,
  IntruderResultRow,
  IntruderRunState,
} from "../types";

export const MARK = "\u00a7";

export const ATTACKS: { id: IntruderAttack; label: string; hint: string }[] = [
  { id: "sniper", label: "Sniper", hint: "Each position is attacked in turn with its own payload set" },
  { id: "battering_ram", label: "Battering ram", hint: "One payload is applied to every position at the same time" },
  { id: "pitchfork", label: "Pitchfork", hint: "Payload sets advance together, one item per position" },
  { id: "cluster_bomb", label: "Cluster bomb", hint: "Every combination of every payload set is tried" },
];

export type ProcessorDef = { kind: string; label: string; args: string[] };

export const PROCESSORS: ProcessorDef[] = [
  { kind: "url_encode", label: "URL-encode", args: [] },
  { kind: "url_decode", label: "URL-decode", args: [] },
  { kind: "base64_encode", label: "Base64-encode", args: [] },
  { kind: "base64_decode", label: "Base64-decode", args: [] },
  { kind: "md5", label: "MD5 hash", args: [] },
  { kind: "sha1", label: "SHA-1 hash", args: [] },
  { kind: "sha256", label: "SHA-256 hash", args: [] },
  { kind: "replace", label: "Replace", args: ["a", "b"] },
  { kind: "prefix", label: "Add prefix", args: ["a"] },
  { kind: "suffix", label: "Add suffix", args: ["a"] },
  { kind: "upper", label: "Uppercase", args: [] },
  { kind: "lower", label: "Lowercase", args: [] },
  { kind: "trim", label: "Trim", args: [] },
  { kind: "reverse", label: "Reverse", args: [] },
];

export const processorDef = (kind: string): ProcessorDef =>
  PROCESSORS.find((p) => p.kind === kind) ?? { kind, label: kind, args: [] };

/** Number of complete §…§ pairs, matching the Rust parser. */
export function countPositions(raw: string): number {
  let open = false;
  let pairs = 0;
  for (const ch of raw) {
    if (ch !== MARK) continue;
    if (!open) open = true;
    else {
      pairs++;
      open = false;
    }
  }
  return pairs;
}

/** Inner text of each §…§ pair, matching the Rust parser. */
export function positionTexts(raw: string): string[] {
  const out: string[] = [];
  let buf = "";
  let open = false;
  for (const ch of raw) {
    if (ch === MARK) {
      if (!open) {
        open = true;
        buf = "";
      } else {
        out.push(buf);
        open = false;
      }
    } else if (open) {
      buf += ch;
    }
  }
  return out;
}

export function stripMarkers(raw: string): string {
  return raw.split(MARK).join("");
}

export function wrapSelection(raw: string, start: number, end: number): string {
  return raw.slice(0, start) + MARK + raw.slice(start, end) + MARK + raw.slice(end);
}

export function parseJsonArray<T>(s: string | null | undefined, fallback: T[]): T[] {
  if (!s) return fallback;
  try {
    const v = JSON.parse(s);
    return Array.isArray(v) ? (v as T[]) : fallback;
  } catch {
    return fallback;
  }
}

export function parseJsonObject(s: string | null | undefined): Record<string, string> {
  if (!s) return {};
  try {
    const v = JSON.parse(s);
    return v && typeof v === "object" && !Array.isArray(v) ? (v as Record<string, string>) : {};
  } catch {
    return {};
  }
}

export function parseCombo(combo: string): string[] {
  return parseJsonArray<string>(combo, []);
}

export function payloadCount(
  kind: string,
  items: string,
  start?: number | null,
  end?: number | null,
  step?: number | null
): number {
  if (kind === "numbers") {
    const s = typeof start === "number" ? start : NaN;
    const e = typeof end === "number" ? end : NaN;
    const st = typeof step === "number" && step > 0 ? step : 1;
    if (Number.isNaN(s) || Number.isNaN(e) || s > e) return 0;
    return Math.floor((e - s) / st) + 1;
  }
  return items.split("\n").filter((l) => l.trim().length > 0).length;
}

// Mirror of the engine's plan_total so the toolbar can show an estimate.
export function estimateTotal(
  attack: IntruderAttack,
  setSizes: number[],
  positions: number,
  setMap: number[]
): number {
  if (setSizes.length === 0 || positions === 0) return 0;
  const sizeAt = (p: number): number => {
    const mapped = setMap[p];
    const idx = mapped !== undefined && mapped < setSizes.length ? mapped : 0;
    return setSizes[Math.min(idx, setSizes.length - 1)] ?? 0;
  };
  if (attack === "battering_ram") return sizeAt(0);
  if (attack === "pitchfork") {
    let min = Infinity;
    for (let p = 0; p < positions; p++) min = Math.min(min, sizeAt(p));
    return Number.isFinite(min) ? min : 0;
  }
  if (attack === "cluster_bomb") {
    let total = 1;
    for (let p = 0; p < positions; p++) total *= sizeAt(p);
    return total;
  }
  let sum = 0;
  for (let p = 0; p < positions; p++) sum += sizeAt(p);
  return sum;
}

export type PayloadSetInput = {
  index: number;
  name: string;
  kind: string;
  items: string;
  start?: number | null;
  end?: number | null;
  step?: number | null;
  processors: { kind: string; a?: string | null; b?: string | null }[];
};

export type CampaignSaveInput = {
  id: number;
  name: string;
  attackType: IntruderAttack;
  reqRaw: string;
  urlHint: string | null;
  follow: boolean;
  setMap: number[];
  threads: number;
  delayMs: number;
  maxRequests: number;
  extractors: { name: string; pattern: string }[];
};

export const intruderApi = {
  list: () => invoke<IntruderCampaignSummary[]>("intruder_campaign_list"),

  get: (campaignId: number) =>
    invoke<IntruderCampaignDetail>("intruder_campaign_get", { campaignId }),

  create: (raw: string, urlHint: string | null, name?: string) =>
    invoke<number>("intruder_campaign_new", { raw, urlHint, name }),

  save: (c: CampaignSaveInput) =>
    invoke<void>("intruder_campaign_save", {
      campaignId: c.id,
      name: c.name,
      attackType: c.attackType,
      reqRaw: c.reqRaw,
      urlHint: c.urlHint,
      follow: c.follow,
      setMap: c.setMap,
      threads: c.threads,
      delayMs: c.delayMs,
      maxRequests: c.maxRequests,
      extractors: c.extractors,
    }),

  remove: (campaignId: number) => invoke<void>("intruder_campaign_delete", { campaignId }),

  autoPositions: (reqRaw: string) => invoke<string>("intruder_positions_auto", { reqRaw }),

  savePayloads: (campaignId: number, sets: PayloadSetInput[]) =>
    invoke<void>("intruder_payloads_save", { campaignId, sets }),

  start: (campaignId: number) => invoke<IntruderRunState>("intruder_start", { campaignId }),

  pause: (campaignId: number) => invoke<IntruderRunState>("intruder_pause", { campaignId }),

  resume: (campaignId: number) => invoke<IntruderRunState>("intruder_resume", { campaignId }),

  stop: (campaignId: number) => invoke<IntruderRunState>("intruder_stop", { campaignId }),

  state: (campaignId: number) => invoke<IntruderRunState>("intruder_state", { campaignId }),

  results: (campaignId: number, offset = 0, limit = 500) =>
    invoke<{ rows: IntruderResultRow[]; total: number }>("intruder_results_list", {
      campaignId,
      offset,
      limit,
    }),

  result: (campaignId: number, seq: number) =>
    invoke<IntruderResultFull | null>("intruder_result_get", { campaignId, seq }),

  clearResults: (campaignId: number) => invoke<number>("intruder_results_clear", { campaignId }),
};
