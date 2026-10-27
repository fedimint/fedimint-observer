import { SHUTDOWN_STYLES, shutdownDate, timeUntilShutdown, type Shutdown } from '../utils/shutdown';

export function ShutdownBadge({ shutdown }: { shutdown: Shutdown }) {
  const date = shutdownDate(shutdown);
  const over = shutdown.stage === 'over';
  return (
    <span
      className={`inline-block whitespace-nowrap rounded px-2 py-0.5 text-[10px] sm:text-xs font-medium ${SHUTDOWN_STYLES[shutdown.stage].badge}`}
      title={over
        ? `Its guardians announced it shut down on ${date}`
        : `Its guardians announced it shuts down on ${date}, ${timeUntilShutdown(shutdown)}`}
    >
      {over ? 'Shut down' : 'Shuts down'} {date}
    </span>
  );
}
