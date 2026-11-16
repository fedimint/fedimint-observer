import type { GuardianHealthWindow } from '../../../types/api';
import { formatUtcDate, formatUtcDayHour } from '../../../utils/time';
import { slotUptime, type AvailabilityLevel, type HistorySlot } from './guardianModel';
import { formatMs, formatPct } from './format';

export const WINDOW_OPTIONS = [
  { value: '24h', label: '24h' },
  { value: '7d', label: '7d' },
  { value: '30d', label: '30d' },
] as const satisfies readonly { value: GuardianHealthWindow; label: string }[];

export const LEVEL_FILL: Record<AvailabilityLevel, string> = {
  ok: 'bg-ok',
  minor: 'bg-warn',
  major: 'bg-serious',
  down: 'bg-crit',
  none: 'cell-empty',
};

export const SLOT_DESCRIPTION: Record<GuardianHealthWindow, string> = {
  '24h': 'hourly',
  '7d': '4-hour blocks',
  '30d': 'daily, UTC',
};

export function slotRange(slot: HistorySlot, window: GuardianHealthWindow): string {
  if (window === '30d') return formatUtcDate(slot.start);
  return `${formatUtcDayHour(slot.start)}–${new Date(slot.end).toISOString().slice(11, 16)} UTC`;
}

export function describeSlot(guardian: string, slot: HistorySlot, window: GuardianHealthWindow): string {
  const uptime = slotUptime(slot);
  const range = slotRange(slot, window);
  if (uptime === null) return `${guardian}, ${range}: not polled`;
  const latency = slot.latencyMs !== null ? `, mean latency ${formatMs(slot.latencyMs)}` : '';
  return `${guardian}, ${range}: ${formatPct(uptime)} available over ${slot.samples} polls${latency}`;
}
