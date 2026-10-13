import { invoke } from "@tauri-apps/api/core";
import type { CheckInfo, Issue, IssueCounts, IssueSeverity, ScanConfig, ScanKind, ScanProgress, ScanTask } from "../types";

export const SEVERITIES: IssueSeverity[] = ["critical", "high", "medium", "low", "info"];

/** Severity → CSS suffix used by `.issue-badge` / `.sev-chip`. */
export function sevClass(sev: string): string {
  switch (sev) {
    case "critical":
      return "crit";
    case "high":
      return "high";
    case "medium":
      return "med";
    case "low":
      return "low";
    default:
      return "info";
  }
}

export const scannerApi = {
  checks: () => invoke<CheckInfo[]>("checks_list"),

  list: (opts?: {
    severities?: IssueSeverity[];
    host?: string;
    status?: string;
    text?: string;
    offset?: number;
    limit?: number;
  }) =>
    invoke<{ rows: Issue[]; total: number }>("issues_list", {
      severities: opts?.severities,
      host: opts?.host,
      status: opts?.status,
      text: opts?.text,
      offset: opts?.offset,
      limit: opts?.limit,
    }),

  get: (id: number) => invoke<Issue | null>("issues_get", { id }),

  setStatus: (id: number, status: string) =>
    invoke<number>("issues_update_status", { id, status }),

  remove: (id: number) => invoke<number>("issues_delete", { id }),

  counts: () => invoke<IssueCounts>("issues_counts"),

  // ---- scan tasks (4b) ----
  scanStart: (target: string, kind?: ScanKind, configId?: number) =>
    invoke<number>("scan_start", { target, kind, configId }),

  scanCancel: (taskId: number) => invoke<void>("scan_cancel", { taskId }),

  scanPause: (taskId: number) => invoke<void>("scan_pause", { taskId }),

  scanResume: (taskId: number) => invoke<void>("scan_resume", { taskId }),

  scanTasks: () => invoke<ScanTask[]>("scan_tasks_list"),

  scanState: () => invoke<{ running: ScanProgress[] }>("scan_state"),

  // ---- scan configs (4c) ----
  scanConfigList: () => invoke<ScanConfig[]>("scan_config_list"),

  scanConfigGet: (configId: number) => invoke<ScanConfig>("scan_config_get", { configId }),

  scanConfigSave: (config: Partial<ScanConfig> & { name: string }) =>
    invoke<number>("scan_config_save", { config }),

  scanConfigDelete: (configId: number) => invoke<void>("scan_config_delete", { configId }),
};
