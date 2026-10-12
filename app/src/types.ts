export type TabId =
  | "proxy"
  | "repeater"
  | "intruder"
  | "decoder"
  | "comparer"
  | "sequencer"
  | "target"
  | "dashboard"
  | "extensions"
  | "collaborator"
  | "logger"
  | "settings";

// Cross-tool message hand-off (history → repeater/intruder/…)
export type Transfer = {
  raw: string;
  url: string;
  response?: string;
  seq: number;
};

export type HttpExchange = {
  id: number;
  started_at: string;
  time_ms: number | null;
  client_addr?: string | null;
  host: string;
  method: string;
  url: string;
  req_headers: [string, string][];
  req_body: string | null;
  status: number | null;
  resp_headers: [string, string][];
  resp_body: string | null;
  tls: boolean;
  websocket: boolean;
  in_scope: boolean;
};

export type ProxyStatus = {
  running: boolean;
  port: number;
  intercept: boolean;
  intercept_resp: boolean;
  held_count: number;
  history_count: number;
};

export type HeldInfo = {
  id: number;
  raw_request: string;
  host: string;
  port: number;
  tls: boolean;
  direction: string;
};

export type ScopeRule = {
  id: number;
  mode: string;
  kind: string;
  pattern: string;
  enabled: boolean;
};

export type MatchRule = {
  id: number;
  enabled: boolean;
  phase: string;
  match_kind: string;
  match_val: string;
  replace_val: string;
  comment: string | null;
};

export type RepeaterTab = {
  id: number;
  name: string;
  req_raw: string;
  last_status: number | null;
  last_time_ms: number | null;
  last_resp_raw: string | null;
  sort_order: number;
};

export type RepeaterHistoryEntry = {
  id: number;
  tab_id: number;
  sent_at: string;
  req_raw: string;
  resp_raw: string | null;
  status: number | null;
  time_ms: number | null;
};

export type WsConn = {
  id: number;
  ts_start: string;
  ts_end: string | null;
  url: string;
  host: string;
  port: number;
  tls: boolean;
  msgs_out: number;
  msgs_in: number;
};

export type WsFrame = {
  id: number;
  conn_id: number;
  ts: string;
  dir: string;
  opcode: number;
  len: number;
  payload: string;
};

export type SearchHit = {
  source: "history" | "log";
  id: number;
  title: string;
  detail: string;
  snippet: string;
  tab: string;
};

export type LogEntry = {
  id: number;
  ts: string;
  tool: string;
  level: string;
  message: string;
};

export type LogFilter = {
  tool?: string | null;
  level?: string | null;
  query?: string | null;
  limit?: number | null;
};

export const emptyStatus: ProxyStatus = {
  running: false,
  port: 8080,
  intercept: false,
  intercept_resp: false,
  held_count: 0,
  history_count: 0,
};

export function headersToText(h: [string, string][]): string {
  return h.map(([k, v]) => `${k}: ${v}`).join("\n");
}

export function statusClass(s: number | null): string {
  if (s == null) return "chip";
  if (s >= 200 && s < 400) return "chip ok";
  if (s >= 400) return "chip err";
  return "chip";
}
