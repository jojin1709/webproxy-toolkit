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

// ---------------------------------------------------------- Intruder (Phase 3)

export type IntruderAttack = "sniper" | "battering_ram" | "pitchfork" | "cluster_bomb";

export type IntruderCampaignSummary = {
  id: number;
  name: string;
  attack_type: IntruderAttack;
  status: string;
  positions: number;
  updated_at: string;
};

export type IntruderCampaign = {
  id: number;
  name: string;
  attack_type: IntruderAttack;
  req_raw: string;
  url_hint: string | null;
  follow: boolean;
  set_map: string;
  threads: number;
  delay_ms: number;
  max_requests: number;
  extractors: string;
  status: string;
  created_at: string;
  updated_at: string;
};

export type IntruderProcessor = {
  kind: string;
  a?: string | null;
  b?: string | null;
};

export type IntruderExtractor = {
  name: string;
  pattern: string;
};

export type IntruderPayloadSet = {
  id: number;
  campaign_id: number;
  set_index: number;
  name: string;
  kind: string; // list | numbers
  items: string;
  start_i: number | null;
  end_i: number | null;
  step_i: number | null;
  processors: string; // JSON [{kind, a, b}]
};

export type IntruderRunState = {
  campaign_id: number;
  status: string;
  sent: number;
  total: number;
  errors: number;
  done: boolean;
  elapsed_ms: number;
  rps: number;
};

export type IntruderResultRow = {
  seq: number;
  combo: string; // JSON string[]
  status: number | null;
  resp_len: number;
  time_ms: number;
  error: string | null;
  extracted: string; // JSON {name: value}
};

export type IntruderResultFull = IntruderResultRow & {
  req_raw: string;
  resp_raw: string | null;
};

export type IntruderCampaignDetail = {
  campaign: IntruderCampaign;
  payloads: IntruderPayloadSet[];
  state: IntruderRunState;
};

// ---------------------------------------------------------- Scanner (Phase 4)

export type IssueSeverity = "info" | "low" | "medium" | "high" | "critical";
export type IssueStatus = "open" | "acknowledged" | "false-positive";

export type Issue = {
  id: number;
  check_id: string;
  name: string;
  severity: IssueSeverity;
  confidence: "tentative" | "firm" | "certain";
  host: string;
  url: string;
  method: string;
  path: string;
  param: string;
  description: string;
  remediation: string;
  evidence: string;
  exchange_id: number | null;
  status: IssueStatus;
  created_at: string;
};

export type IssueCounts = Record<IssueSeverity, number>;

export type CheckInfo = {
  id: string;
  name: string;
  kind: "passive" | "active";
  severity: IssueSeverity;
  description: string;
  remediation: string;
};

export type IssueFilter = {
  severities: IssueSeverity[];
  host: string;
  status: "" | IssueStatus;
  text: string;
};

export type ScanKind = "crawl" | "audit" | "crawl-audit";

export type ScanTask = {
  id: number;
  kind: ScanKind;
  target: string;
  config_id: number | null;
  status: "running" | "paused" | "done" | "cancelled" | "error";
  pages_crawled: number;
  requests_sent: number;
  issues_found: number;
  error: string | null;
  created_at: string;
  updated_at: string;
};

export type ScanProgress = {
  task_id: number;
  pages: number;
  requests: number;
  issues: number;
  status: string;
  queued?: number;
};

export type ScanConfig = {
  id: number;
  name: string;
  passive_enabled: boolean;
  active_checks: string[]; // empty = all
  crawl_max_pages: number;
  crawl_max_depth: number;
  audit_threads: number;
  throttle_ms: number;
  is_default: boolean;
  created_at: string;
};
