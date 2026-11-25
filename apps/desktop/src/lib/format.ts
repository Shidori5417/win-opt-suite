import type {
  AntiCheatDriver,
  AppError_Serialize,
  ChangeKind,
  ConflictWarning_Serialize,
  EnvironmentTag,
  MetricKind,
  MetricSample,
  Profile,
  ProviderKind,
  RegistryValue,
  ResourceState_Serialize,
  RiskLevel,
  Severity,
  SystemFacts,
  TransactionStatus,
  TweakCategory,
} from "../bindings";
import type { Locale, TranslateFn, TranslationKey } from "../i18n";

export type UiError =
  | { kind: "app"; error: AppError_Serialize }
  | { kind: "unexpected"; message: string };

export function toAppError(error: AppError_Serialize): UiError {
  return { kind: "app", error };
}

export function toUnexpectedError(err: unknown): UiError {
  if (err instanceof Error) {
    return { kind: "unexpected", message: err.message };
  }
  return { kind: "unexpected", message: String(err) };
}

export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) {
    return "0 B";
  }
  const units = ["B", "KiB", "MiB", "GiB", "TiB"] as const;
  let value = bytes;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }
  const unit = units[unitIndex] ?? "B";
  const digits = unitIndex === 0 ? 0 : value >= 100 ? 0 : value >= 10 ? 1 : 2;
  return `${value.toFixed(digits)} ${unit}`;
}

export function formatGpuVendor(vendorId: number): string {
  const hex = `0x${vendorId.toString(16).toUpperCase().padStart(4, "0")}`;
  switch (vendorId) {
    case 0x10de:
      return `NVIDIA (${hex})`;
    case 0x1002:
      return `AMD (${hex})`;
    case 0x8086:
      return `Intel (${hex})`;
    case 0x1414:
      return `Microsoft (${hex})`;
    default:
      return hex;
  }
}

export function deriveEnvironmentTags(facts: SystemFacts): EnvironmentTag[] {
  const tags: EnvironmentTag[] = [facts.is_laptop ? "laptop" : "desktop"];
  if (facts.on_battery) {
    tags.push("on_battery");
  }
  if (facts.is_virtual_machine) {
    tags.push("virtual_machine");
  }
  if (facts.is_domain_joined) {
    tags.push("domain_joined");
  }
  for (const driver of facts.anti_cheat) {
    tags.push(antiCheatToTag(driver));
  }
  return tags;
}

function antiCheatToTag(driver: AntiCheatDriver): EnvironmentTag {
  switch (driver) {
    case "vanguard":
      return "anti_cheat_vanguard";
    case "faceit":
      return "anti_cheat_faceit";
    case "easy_anti_cheat":
      return "anti_cheat_easy_anti_cheat";
    case "battl_eye":
      return "anti_cheat_battl_eye";
  }
}

export function profileLabel(profile: Profile, t: TranslateFn): string {
  const key: TranslationKey = `profile.${profile}`;
  return t(key);
}

export function riskLabel(risk: RiskLevel, t: TranslateFn): string {
  const key: TranslationKey = `risk.${risk}`;
  return t(key);
}

export function categoryLabel(category: TweakCategory, t: TranslateFn): string {
  const key: TranslationKey = `category.${category}`;
  return t(key);
}

export function providerLabel(provider: ProviderKind, t: TranslateFn): string {
  const key: TranslationKey = `provider.${provider}`;
  return t(key);
}

export function changeKindLabel(change: ChangeKind, t: TranslateFn): string {
  const key: TranslationKey = `change.${change}`;
  return t(key);
}

export function severityLabel(severity: Severity, t: TranslateFn): string {
  const key: TranslationKey = `severity.${severity}`;
  return t(key);
}

export function statusLabel(status: TransactionStatus, t: TranslateFn): string {
  const key: TranslationKey = `status.${status}`;
  return t(key);
}

export function envTagLabel(tag: EnvironmentTag, t: TranslateFn): string {
  const key: TranslationKey = `env.${tag}`;
  return t(key);
}

export function antiCheatLabel(driver: AntiCheatDriver, t: TranslateFn): string {
  const key: TranslationKey = `anticheat.${driver}`;
  return t(key);
}

export function metricKindLabel(kind: MetricKind, t: TranslateFn): string {
  const key: TranslationKey = `metric.${kind}`;
  return t(key);
}

export function formatRegistryValue(value: RegistryValue): string {
  switch (value.type) {
    case "dword":
      return `REG_DWORD (${value.data})`;
    case "qword":
      return `REG_QWORD (${value.data})`;
    case "string":
      return `REG_SZ ("${value.data}")`;
    case "expand_string":
      return `REG_EXPAND_SZ ("${value.data}")`;
    case "multi_string":
      return `REG_MULTI_SZ ([${value.data.join(", ")}])`;
    case "binary":
      return `REG_BINARY (${value.data.length} B)`;
  }
}

export function formatResourceState(
  state: ResourceState_Serialize | null,
  t: TranslateFn,
): string {
  if (state === null) {
    return t("state.absent");
  }
  switch (state.kind) {
    case "registry_value":
      return formatRegistryValue(state.value);
    case "service":
      return `start_type=${state.start_type}`;
    case "scheduled_task":
      return state.enabled ? t("state.enabled") : t("state.disabled");
    case "active_power_scheme":
      return state.scheme_guid;
    case "power_setting":
      return `AC=${state.ac_value} / DC=${state.dc_value}`;
    case "firewall_rule_enabled":
      return state.enabled ? t("state.enabled") : t("state.disabled");
    case "firewall_rule":
      return `${state.rule.direction} ${state.rule.action} (${state.rule.enabled ? t("state.enabled") : t("state.disabled")})`;
  }
}

export function formatConflictWarning(
  warning: ConflictWarning_Serialize,
  t: TranslateFn,
): string {
  const k = warning.kind;
  switch (k.kind) {
    case "declared_conflict":
      return t("conflict.declared_conflict", {
        tweak: k.tweak,
        other: k.other,
      });
    case "resource_contention":
      return t("conflict.resource_contention", {
        resource: k.resource,
        tweaks: k.tweaks.join(", "),
      });
    case "excluded_environment":
      return t("conflict.excluded_environment", {
        tweak: k.tweak,
        tag: envTagLabel(k.tag, t),
      });
    case "unsupported_build": {
      const range =
        k.max_build !== null
          ? `${k.min_build}..=${k.max_build}`
          : `>= ${k.min_build}`;
      return t("conflict.unsupported_build", {
        tweak: k.tweak,
        range,
        current: k.current_build,
      });
    }
  }
}

export function formatAppError(
  error: AppError_Serialize,
  t: TranslateFn,
): string {
  switch (error.code) {
    case "access_denied":
      return t("error.access_denied", { resource: error.resource });
    case "not_found":
      return t("error.not_found", { resource: error.resource });
    case "unsupported_platform":
      return t("error.unsupported_platform", { reason: error.reason });
    case "provider_not_registered":
      return t("error.provider_not_registered", {
        provider: providerLabel(error.provider, t),
      });
    case "action_provider_mismatch":
      return t("error.action_provider_mismatch", {
        expected: providerLabel(error.expected, t),
        found: providerLabel(error.found, t),
      });
    case "invalid_manifest":
      return t("error.invalid_manifest", {
        id: error.id,
        count: error.issues.length,
      });
    case "conflict_blocked":
      return t("error.conflict_blocked", { count: error.warnings.length });
    case "lock_unavailable":
      return t("error.lock_unavailable");
    case "recovery_required":
      return t("error.recovery_required", {
        transaction_id: error.transaction_id,
      });
    case "duplicate_provider":
      return t("error.duplicate_provider", { provider: providerLabel(error.provider, t) });
    case "invalid_transaction_state":
      return t("error.invalid_transaction_state", {
        transaction_id: error.transaction_id,
        status: statusLabel(error.status, t),
      });
    case "rollback_superseded":
      return t("error.rollback_superseded", {
        transaction_id: error.transaction_id,
        newer_transaction_id: error.newer_transaction_id,
        resources: error.resources.join(", "),
      });
    case "verification_failed":
      return t("error.verification_failed", { resource: error.resource });
    case "rollback_incomplete":
      return t("error.rollback_incomplete", {
        transaction_id: error.transaction_id,
        failed_resources: error.failed_resources.join(", "),
      });
    case "storage":
      return t("error.storage", { message: error.message });
    case "migration":
      return t("error.migration", {
        version: error.version,
        message: error.message,
      });
    case "serialization":
      return t("error.serialization", { message: error.message });
    case "io":
      return t("error.io", { message: error.message });
    case "os":
      return t("error.os", {
        os_code: `0x${(error.os_code >>> 0).toString(16).padStart(8, "0")}`,
        message: error.message,
      });
    case "internal":
      return t("error.internal", { message: error.message });
  }
}

export function formatUiError(error: UiError, t: TranslateFn): string {
  if (error.kind === "app") {
    return formatAppError(error.error, t);
  }
  return t("error.unknown", { message: error.message });
}

export function formatTimestamp(unixMs: number, locale: Locale): string {
  const date = new Date(unixMs);
  if (Number.isNaN(date.getTime())) {
    return String(unixMs);
  }
  return new Intl.DateTimeFormat(locale === "tr" ? "tr-TR" : "en-US", {
    dateStyle: "medium",
    timeStyle: "medium",
  }).format(date);
}

export interface MetricDeltaRow {
  kind: MetricKind;
  before: number | null;
  after: number | null;
  delta: number | null;
}

export function pairMetrics(samples: MetricSample[]): MetricDeltaRow[] {
  const order: MetricKind[] = [
    "idle_memory_used_bytes",
    "running_services",
    "running_processes",
  ];
  const byKind = new Map<
    MetricKind,
    { before: number | null; after: number | null }
  >();

  for (const sample of samples) {
    const current = byKind.get(sample.kind) ?? { before: null, after: null };
    if (sample.phase === "before") {
      current.before = sample.value;
    } else {
      current.after = sample.value;
    }
    byKind.set(sample.kind, current);
  }

  const rows: MetricDeltaRow[] = [];
  for (const kind of order) {
    const entry = byKind.get(kind);
    if (!entry) {
      continue;
    }
    const delta =
      entry.before !== null && entry.after !== null
        ? entry.after - entry.before
        : null;
    rows.push({ kind, before: entry.before, after: entry.after, delta });
  }
  return rows;
}

export function formatMetricValue(
  kind: MetricKind,
  value: number | null,
): string {
  if (value === null) {
    return "—";
  }
  if (kind === "idle_memory_used_bytes") {
    return formatBytes(value);
  }
  return String(Math.round(value));
}

export function formatMetricDelta(
  kind: MetricKind,
  delta: number | null,
): string {
  if (delta === null) {
    return "—";
  }
  if (delta === 0) {
    return kind === "idle_memory_used_bytes" ? "0 B" : "0";
  }
  const sign = delta > 0 ? "+" : "-";
  const abs = Math.abs(delta);
  if (kind === "idle_memory_used_bytes") {
    return `${sign}${formatBytes(abs)}`;
  }
  return `${sign}${Math.round(abs)}`;
}
