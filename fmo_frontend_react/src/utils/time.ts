// Every absolute date and time in the app is shown in UTC, the zone the server
// buckets days in, so a day never shifts with the viewer's clock.

type DateInput = string | number | Date | null | undefined;

function toDate(value: DateInput): Date | null {
  if (value === null || value === undefined) return null;
  const date = value instanceof Date ? value : new Date(value);
  return Number.isNaN(date.getTime()) ? null : date;
}

/** "Nov 7, 2026, 19:20 UTC" */
export function formatUtcDateTime(value: DateInput, fallback = 'N/A'): string {
  const date = toDate(value);
  if (!date) return fallback;
  const text = date.toLocaleString('en-US', {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
    hourCycle: 'h23',
    timeZone: 'UTC',
  });
  return `${text} UTC`;
}

/** "19:20:05 UTC" */
export function formatUtcTime(value: DateInput, fallback = 'N/A'): string {
  const date = toDate(value);
  if (!date) return fallback;
  return `${date.toLocaleTimeString('en-US', { hourCycle: 'h23', timeZone: 'UTC' })} UTC`;
}

/** "Nov 7, 2026" for the UTC day of an instant, or of a "YYYY-MM-DD" day as written */
export function formatUtcDate(value: DateInput, options: Intl.DateTimeFormatOptions = { month: 'short', day: 'numeric', year: 'numeric' }, fallback = 'N/A'): string {
  // A bare "YYYY-MM-DD" already parses as UTC midnight, so it keeps its day
  const date = toDate(value);
  if (!date) return typeof value === 'string' ? value : fallback;
  return date.toLocaleDateString('en-US', { ...options, timeZone: 'UTC' });
}

/** "Nov 7, 19:00" in UTC, for chart points a few hours apart */
export function formatUtcDayHour(value: DateInput, fallback = ''): string {
  const date = toDate(value);
  if (!date) return fallback;
  return date.toLocaleString('en-US', { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit', hourCycle: 'h23', timeZone: 'UTC' });
}
