// Stroke icons on a 24px grid. Decorative: hidden from screen readers, so the
// element that carries one must also carry text or an accessible name.

const PATHS = {
  overview: 'M3 13h8V3H3v10Zm0 8h8v-6H3v6Zm10 0h8V11h-8v10Zm0-18v6h8V3h-8Z',
  federations: 'M12 2 3 7v10l9 5 9-5V7l-9-5Zm0 0v10m0 0 9-5m-9 5-9-5m9 5v10',
  guardians: 'M12 3 4 6v6c0 4.5 3.4 8.3 8 9 4.6-.7 8-4.5 8-9V6l-8-3Zm-3 9 2 2 4-4',
  reliability: 'M3 12h4l3-8 4 16 3-8h4',
  directory: 'M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2M9 11a4 4 0 1 0 0-8 4 4 0 0 0 0 8Zm13 10v-2a4 4 0 0 0-3-3.9M16 3.1a4 4 0 0 1 0 7.8',
  liquidity: 'M12 2.7 6.3 8.4a8 8 0 1 0 11.4 0L12 2.7Z',
  nostr: 'M4 4h16v12H5.2L4 17.2V4Zm4 5h8m-8 3h5',
  gateway: 'M13 2 3 14h9l-1 8 10-12h-9l1-8Z',
  clock: 'M12 7v5l3 2m6-2a9 9 0 1 1-18 0 9 9 0 0 1 18 0Z',
  check: 'M20 6 9 17l-5-5',
  checkCircle: 'M22 11.1V12a10 10 0 1 1-5.9-9.1M22 4 12 14l-3-3',
  alert: 'M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0ZM12 9v4m0 4h.01',
  xCircle: 'M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20Zm3-13-6 6m0-6 6 6',
  minusCircle: 'M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20ZM8 12h8',
  help: 'M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20Zm-2.1-13a3 3 0 0 1 5.8 1c0 2-3 3-3 3m.1 4h.01',
  info: 'M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20Zm0-6v-4m0-4h.01',
  copy: 'M9 9h11v11H9V9Zm-4 6H4V4h11v1',
  external: 'M15 3h6v6m-11 5L21 3m-3 10v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6',
  menu: 'M4 6h16M4 12h16M4 18h16',
  close: 'M18 6 6 18M6 6l12 12',
  sun: 'M12 17a5 5 0 1 0 0-10 5 5 0 0 0 0 10Zm0-15v2m0 16v2M4.2 4.2l1.4 1.4m12.8 12.8 1.4 1.4M2 12h2m16 0h2M4.2 19.8l1.4-1.4M18.4 5.6l1.4-1.4',
  moon: 'M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8Z',
  monitor: 'M3 4h18v12H3V4Zm5 16h8m-4-4v4',
  key: 'M21 2l-2 2m-7.6 7.6a5.5 5.5 0 1 1-7.8 7.8 5.5 5.5 0 0 1 7.8-7.8Zm0 0L15.5 7.5m0 0 3 3L22 7l-3-3m-3.5 3.5L19 4',
  link: 'M10 13a5 5 0 0 0 7.5.5l3-3a5 5 0 0 0-7-7l-1.7 1.7M14 11a5 5 0 0 0-7.5-.5l-3 3a5 5 0 0 0 7 7l1.7-1.7',
  chevronRight: 'm9 18 6-6-6-6',
  chevronLeft: 'm15 18-6-6 6-6',
  arrowLeft: 'M19 12H5m7 7-7-7 7-7',
  github: 'M9 19c-5 1.5-5-2.5-7-3m14 6v-3.9a3.4 3.4 0 0 0-.9-2.6c3.1-.4 6.4-1.5 6.4-7a5.4 5.4 0 0 0-1.5-3.8 5 5 0 0 0-.1-3.8s-1.2-.3-3.9 1.5a13.4 13.4 0 0 0-7 0C6.3 1.6 5.1 2 5.1 2a5 5 0 0 0-.1 3.8A5.4 5.4 0 0 0 3.5 9.5c0 5.4 3.3 6.6 6.4 7a3.4 3.4 0 0 0-.9 2.6V23',
  signal: 'M2 20h.01M7 20v-4m5 4v-8m5 8V8m5 12V4',
  layers: 'm12 2 10 5-10 5L2 7l10-5Zm-10 10 10 5 10-5M2 17l10 5 10-5',
  flask: 'M9 3h6m-5 0v6L4.5 18.5A2 2 0 0 0 6.2 21h11.6a2 2 0 0 0 1.7-2.5L14 9V3M7 15h10',
  filter: 'M22 3H2l8 9.5V19l4 2v-8.5L22 3Z',
  sort: 'm3 16 4 4 4-4M7 20V4m14 4-4-4-4 4m4-4v16',
} as const;

export type IconName = keyof typeof PATHS;

export function Icon({ name, className = 'size-4', strokeWidth = 1.8 }: { name: IconName; className?: string; strokeWidth?: number }) {
  return (
    <svg
      aria-hidden="true"
      focusable="false"
      className={`shrink-0 ${className}`}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={strokeWidth}
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <path d={PATHS[name]} />
    </svg>
  );
}
