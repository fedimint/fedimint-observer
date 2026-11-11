/**
 * The guardian view of a federation, derived from what the observer has
 * stored: the config (seats), `/health` (versions, 30-day averages, block
 * heights), `/health/live` (latest polls and peer views) and
 * `/health/history` (availability buckets). Pure functions only.
 *
 * Identity is never conflated: a Fedimint peer id, the name in the config,
 * the Manifold Guardian key that signed a seat binding and that MG's operator
 * are separate things, and each is shown only when its own source has it.
 */
import type {
  FederationLiveHealth,
  GuardianHealth,
  GuardianHealthHistory,
  GuardianLiveStatus,
  GuardianPeerView,
  ManifoldSeatBinding,
  ManifoldSeatBindingsReport,
} from '../../../types/api';
import { guardianThreshold, type ConfigGuardian } from '../../../utils/config.ts';

/** A guardian answering within this many seconds of the live snapshot is online, like the federation list's rule */
export const ONLINE_WITHIN_S = 5 * 60;
/** Polls back off to every 30 minutes for fully offline federations, so a poll within this shows the collector is watching */
export const POLLED_WITHIN_S = 35 * 60;

export type GuardianState = 'online' | 'offline' | 'unknown';

export interface GuardianRow {
  id: number;
  url: string;
  /** Name from the federation config, not an operator identity */
  configName: string | null;
  state: GuardianState;
  live: GuardianLiveStatus | null;
  /** Latest answer: the live poll, else the last online hour from history */
  lastSeen: string | null;
  lastSeenApproximate: boolean;
  latencyMs: number | null;
  sessionCount: number | null;
  /** Sessions behind the furthest online guardian */
  sessionLag: number | null;
  version: string | null;
  blockHeight: number | null;
  blockOutdated: boolean;
  /** Rolling 30-day uptime from /health */
  uptime30d: number | null;
  avgLatency30d: number | null;
  /** From the 7-day history, when loaded */
  uptime7d: number | null;
  /** Verified seat binding; null when the directory is absent or invalid */
  binding: ManifoldSeatBinding | null;
  warnings: GuardianWarning[];
}

export type WarningTone = 'warn' | 'crit' | 'info';

export interface GuardianWarning {
  id: string;
  tone: WarningTone;
  guardianId: number | null;
  message: string;
}

export interface Resilience {
  total: number;
  threshold: number;
  /** Guardians that may fail with consensus still progressing */
  faultTolerance: number;
  online: number;
  offline: number;
  unknown: number;
  level: 'operational' | 'at_threshold' | 'below_threshold' | 'unknown';
}

export function guardianState(live: GuardianLiveStatus | undefined, generatedAt: string | null): GuardianState {
  if (!live || !generatedAt) return 'unknown';
  const now = Date.parse(generatedAt);
  const age = (time: string | null) => (time === null ? Infinity : (now - Date.parse(time)) / 1000);
  if (age(live.answered_at) <= ONLINE_WITHIN_S) return 'online';
  if (age(live.polled_at) <= POLLED_WITHIN_S) return 'offline';
  return 'unknown';
}

interface RowInputs {
  guardians: readonly ConfigGuardian[];
  health: Record<string, GuardianHealth> | null;
  live: FederationLiveHealth | null;
  history7d: GuardianHealthHistory | null;
  seatBindings: ManifoldSeatBindingsReport | null;
}

export function buildGuardianRows({ guardians, health, live, history7d, seatBindings }: RowInputs): GuardianRow[] {
  const liveById = new Map(live?.guardians.map((g) => [g.guardian_id, g]));
  const historyById = new Map(history7d?.guardians.map((g) => [g.guardian_id, g]));
  const bindings = seatBindings?.status === 'verified' ? seatBindings.bindings : [];
  const states = new Map(guardians.map((g) => [g.id, guardianState(liveById.get(g.id), live?.generated_at ?? null)]));

  const leaderSession = Math.max(
    -1,
    ...guardians
      .filter((g) => states.get(g.id) === 'online')
      .map((g) => liveById.get(g.id)?.session_count ?? -1),
  );

  return guardians.map((guardian) => {
    const liveStatus = liveById.get(guardian.id) ?? null;
    const state = states.get(guardian.id) ?? 'unknown';
    const guardianHealth = health?.[String(guardian.id)];
    const history = historyById.get(guardian.id);
    const online = state === 'online';
    const sessionCount = online ? liveStatus?.session_count ?? null : null;
    const sessionLag = sessionCount !== null && leaderSession >= 0 ? leaderSession - sessionCount : null;

    const row: GuardianRow = {
      id: guardian.id,
      url: guardian.url,
      configName: guardian.name,
      state,
      live: liveStatus,
      lastSeen: liveStatus?.answered_at ?? history?.last_online_hour ?? null,
      lastSeenApproximate: !liveStatus?.answered_at && !!history?.last_online_hour,
      latencyMs: online ? liveStatus?.latency_ms ?? null : null,
      sessionCount,
      sessionLag,
      version: guardianHealth?.software_version ?? null,
      blockHeight: guardianHealth?.latest?.block_height ?? null,
      blockOutdated: guardianHealth?.latest?.block_outdated ?? false,
      uptime30d: guardianHealth ? guardianHealth.avg_uptime : null,
      avgLatency30d: guardianHealth && guardianHealth.avg_latency > 0 ? guardianHealth.avg_latency : null,
      uptime7d: history?.uptime_pct ?? null,
      binding: bindings.find((b) => b.peer_id === guardian.id) ?? null,
      warnings: [],
    };
    row.warnings = guardianWarnings(row);
    return row;
  });
}

function label(row: Pick<GuardianRow, 'id' | 'configName'>): string {
  return row.configName ? `Guardian ${row.id} (${row.configName})` : `Guardian ${row.id}`;
}

function guardianWarnings(row: GuardianRow): GuardianWarning[] {
  const warnings: GuardianWarning[] = [];
  if (row.state === 'offline') {
    warnings.push({
      id: `offline-${row.id}`,
      tone: 'warn',
      guardianId: row.id,
      message: row.lastSeen
        ? `${label(row)} is not answering the observer.`
        : `${label(row)} has not answered the observer in the last 90 days.`,
    });
  }
  if (row.sessionLag !== null && row.sessionLag > 1) {
    warnings.push({
      id: `lag-${row.id}`,
      tone: 'warn',
      guardianId: row.id,
      message: `${label(row)} is ${row.sessionLag} sessions behind the furthest guardian.`,
    });
  }
  if (row.state === 'online' && row.blockOutdated) {
    warnings.push({
      id: `block-${row.id}`,
      tone: 'warn',
      guardianId: row.id,
      message: `${label(row)} reports a Bitcoin block height more than 6 blocks behind the observer's.`,
    });
  }
  const shutdown = row.live?.scheduled_shutdown;
  if (row.state === 'online' && shutdown !== null && shutdown !== undefined) {
    warnings.push({
      id: `shutdown-${row.id}`,
      tone: 'info',
      guardianId: row.id,
      message: `${label(row)} announced a scheduled shutdown at session ${shutdown}.`,
    });
  }
  return warnings;
}

export function resilience(rows: readonly GuardianRow[], liveLoaded: boolean): Resilience {
  const total = rows.length;
  const threshold = guardianThreshold(total);
  const online = rows.filter((r) => r.state === 'online').length;
  const offline = rows.filter((r) => r.state === 'offline').length;
  const unknown = total - online - offline;
  let level: Resilience['level'];
  if (!liveLoaded || total === 0) level = 'unknown';
  else if (online > threshold || (total === 1 && online === 1)) level = 'operational';
  else if (online === threshold) level = 'at_threshold';
  // Guardians in an unknown state might still answer, so only offline ones count against consensus
  else if (online + unknown >= threshold) level = 'unknown';
  else level = 'below_threshold';
  return { total, threshold, faultTolerance: total - threshold, online, offline, unknown, level };
}

/** Federation-level messages first, then each guardian's, most severe first */
export function federationWarnings(rows: readonly GuardianRow[], res: Resilience, connectivity: ConnectivityIssue[]): GuardianWarning[] {
  const warnings: GuardianWarning[] = [];
  if (res.level === 'below_threshold') {
    warnings.push({
      id: 'below-threshold',
      tone: 'crit',
      guardianId: null,
      message: `Only ${res.online} of ${res.total} guardians answer; consensus needs ${res.threshold}, so the federation cannot process transactions until more return.`,
    });
  } else if (res.level === 'at_threshold' && res.total > 1) {
    warnings.push({
      id: 'at-threshold',
      tone: 'warn',
      guardianId: null,
      message: `Exactly ${res.threshold} of ${res.total} guardians answer: consensus still works, but one more outage would stop it.`,
    });
  }
  const perGuardian = rows.flatMap((row) => row.warnings).map((warning) =>
    // While the federation still has slack, one guardian offline is a warning, not a failure
    warning.id.startsWith('offline-') && res.level === 'below_threshold' ? { ...warning, tone: 'crit' as const } : warning,
  );
  const connectivityWarnings: GuardianWarning[] = connectivity.map((issue) => ({
    id: `p2p-${issue.peerId}-${issue.kind}`,
    tone: issue.kind === 'flagged' ? 'warn' : 'info',
    guardianId: issue.peerId,
    message: issue.message,
  }));
  const rank: Record<WarningTone, number> = { crit: 0, warn: 1, info: 2 };
  return [...warnings, ...[...perGuardian, ...connectivityWarnings].sort((a, b) => rank[a.tone] - rank[b.tone])];
}

/** The versions guardians report, most common first */
export function versionSummary(rows: readonly GuardianRow[]): { version: string; guardians: number[] }[] {
  const byVersion = new Map<string, number[]>();
  for (const row of rows) {
    if (!row.version) continue;
    byVersion.set(row.version, [...(byVersion.get(row.version) ?? []), row.id]);
  }
  return [...byVersion.entries()]
    .map(([version, guardians]) => ({ version, guardians }))
    .sort((a, b) => b.guardians.length - a.guardians.length || a.version.localeCompare(b.version));
}

// --- Peer connectivity -------------------------------------------------------

export type PeerCell = 'connected' | 'disconnected' | 'flagged' | 'self' | 'no_report';

export interface ConnectivityMatrix {
  /** Guardians whose latest answer carried a peer view, within the live window */
  reporters: number[];
  peers: number[];
  cell: (reporter: number, peer: number) => PeerCell;
  /** When each reporter's view was taken */
  reportedAt: Map<number, string>;
}

export interface ConnectivityIssue {
  peerId: number;
  kind: 'flagged' | 'split' | 'isolated';
  message: string;
}

export function connectivityMatrix(rows: readonly GuardianRow[]): ConnectivityMatrix {
  const views = new Map<number, Map<number, GuardianPeerView>>();
  const reportedAt = new Map<number, string>();
  for (const row of rows) {
    if (row.state !== 'online' || !row.live || row.live.peers.length === 0) continue;
    views.set(row.id, new Map(row.live.peers.map((peer) => [peer.peer_id, peer])));
    if (row.live.answered_at) reportedAt.set(row.id, row.live.answered_at);
  }
  return {
    reporters: [...views.keys()],
    peers: rows.map((row) => row.id),
    reportedAt,
    cell(reporter, peer) {
      if (reporter === peer) return 'self';
      const view = views.get(reporter)?.get(peer);
      if (!view) return 'no_report';
      if (view.flagged) return 'flagged';
      return view.connected ? 'connected' : 'disconnected';
    },
  };
}

/**
 * Where guardians' own views of the P2P network say something the observer's
 * polls do not: a guardian others flag as not contributing, guardians that
 * disagree about a peer's connection, or a guardian the observer reaches but
 * no peer does.
 */
export function connectivityIssues(rows: readonly GuardianRow[], matrix: ConnectivityMatrix): ConnectivityIssue[] {
  const issues: ConnectivityIssue[] = [];
  const byId = new Map(rows.map((row) => [row.id, row]));
  for (const peer of matrix.peers) {
    const reports = matrix.reporters.filter((r) => r !== peer).map((r) => matrix.cell(r, peer)).filter((c) => c !== 'no_report');
    if (reports.length === 0) continue;
    const name = label(byId.get(peer) ?? { id: peer, configName: null });
    const flagged = reports.filter((c) => c === 'flagged').length;
    const disconnected = reports.filter((c) => c === 'disconnected').length;
    const connected = reports.filter((c) => c === 'connected').length;
    if (flagged > 0) {
      issues.push({ peerId: peer, kind: 'flagged', message: `${flagged} of ${reports.length} reporting guardians flag ${name} as not contributing to consensus.` });
    } else if (disconnected > 0 && connected === 0 && byId.get(peer)?.state === 'online') {
      issues.push({ peerId: peer, kind: 'isolated', message: `${name} answers the observer, but no reporting guardian has a P2P connection to it.` });
    } else if (disconnected > 0 && connected > 0) {
      issues.push({ peerId: peer, kind: 'split', message: `Guardians disagree about ${name}: ${connected} report a connection, ${disconnected} do not.` });
    }
  }
  return issues;
}

// --- History -----------------------------------------------------------------

export interface HistorySlot {
  start: number;
  end: number;
  samples: number;
  polledS: number;
  onlineS: number;
  /** Mean latency over the slot's answered, timed polls */
  latencyMs: number | null;
}

/** Uptime of a slot in percent; null when it was not polled */
export function slotUptime(slot: HistorySlot): number | null {
  return slot.polledS > 0 ? (slot.onlineS / slot.polledS) * 100 : null;
}

/**
 * Every bucket of the window for one guardian, unpolled ones included, merged
 * `group` at a time (e.g. hourly into 4-hour slots for a 7-day heatmap).
 */
export function guardianSlots(history: GuardianHealthHistory, guardianId: number, group = 1): HistorySlot[] {
  const start = Date.parse(history.start);
  const bucketMs = history.bucket_seconds * 1000;
  const series = history.guardians.find((g) => g.guardian_id === guardianId);
  const byStart = new Map(series?.buckets.map((b) => [Date.parse(b.start), b]));
  const slots: HistorySlot[] = [];
  for (let i = 0; i < history.bucket_count; i += group) {
    let samples = 0;
    let polledS = 0;
    let onlineS = 0;
    let latencySum = 0;
    let latencyWeight = 0;
    const count = Math.min(group, history.bucket_count - i);
    for (let j = 0; j < count; j++) {
      const bucket = byStart.get(start + (i + j) * bucketMs);
      if (!bucket) continue;
      samples += bucket.samples;
      polledS += bucket.polled_s;
      onlineS += bucket.online_s;
      if (bucket.avg_latency_ms !== null && bucket.online_samples > 0) {
        latencySum += bucket.avg_latency_ms * bucket.online_samples;
        latencyWeight += bucket.online_samples;
      }
    }
    slots.push({
      start: start + i * bucketMs,
      end: start + (i + count) * bucketMs,
      samples,
      polledS,
      onlineS,
      latencyMs: latencyWeight > 0 ? latencySum / latencyWeight : null,
    });
  }
  return slots;
}

export type AvailabilityLevel = 'ok' | 'minor' | 'major' | 'down' | 'none';

export function availabilityLevel(uptime: number | null): AvailabilityLevel {
  if (uptime === null) return 'none';
  if (uptime >= 98) return 'ok';
  if (uptime >= 90) return 'minor';
  if (uptime > 0) return 'major';
  return 'down';
}

/** Heatmap slots per window: hours for 24h, 4-hour blocks for 7d, days for 30d */
export function heatmapGrouping(history: GuardianHealthHistory): number {
  return history.window === '7d' ? 4 : 1;
}

/** Guardians answering per slot (the sum of each one's online share), against the threshold */
export function reachabilitySeries(history: GuardianHealthHistory, guardianIds: readonly number[]) {
  const perGuardian = guardianIds.map((id) => guardianSlots(history, id));
  const length = perGuardian[0]?.length ?? 0;
  return Array.from({ length }, (_, i) => {
    const slots = perGuardian.map((slots) => slots[i]);
    const polled = slots.filter((slot) => slot.polledS > 0);
    return {
      start: slots[0].start,
      end: slots[0].end,
      answering: polled.length === 0 ? null : polled.reduce((sum, slot) => sum + slot.onlineS / slot.polledS, 0),
      polledGuardians: polled.length,
    };
  });
}

export interface OfflinePeriod {
  guardianId: number;
  start: number;
  end: number;
  /** Still going at the end of the window */
  ongoing: boolean;
}

/**
 * Runs of polled slots in which the guardian never answered, at least
 * `minSlots` long. Unpolled slots end a run: they are unknown, not offline.
 */
export function offlinePeriods(history: GuardianHealthHistory, guardianId: number, minSlots: number): OfflinePeriod[] {
  const slots = guardianSlots(history, guardianId);
  const periods: OfflinePeriod[] = [];
  let runStart = -1;
  slots.forEach((slot, index) => {
    const down = slot.polledS > 0 && slot.onlineS === 0;
    if (down && runStart < 0) runStart = index;
    const last = index === slots.length - 1;
    if ((!down || last) && runStart >= 0) {
      const endIndex = down ? index : index - 1;
      if (endIndex - runStart + 1 >= minSlots) {
        periods.push({ guardianId, start: slots[runStart].start, end: slots[endIndex].end, ongoing: down && last });
      }
      runStart = -1;
    }
  });
  return periods;
}
