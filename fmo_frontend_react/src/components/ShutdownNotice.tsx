import { Link } from 'react-router-dom';
import type { FederationSuccessor } from '../types/api';
import { Copyable } from './Copyable';
import {
  SHUTDOWN_STYLES,
  shutdownDate,
  timeUntilShutdown,
  type Shutdown,
  type ShutdownStage,
} from '../utils/shutdown';

// Calm while the date is far off
const ADVICE: Record<Exclude<ShutdownStage, 'over'>, string> = {
  planned: 'There is no need to act now: move your funds to another federation before then.',
  soon: 'Move your funds to another federation before then.',
  imminent: 'Move your funds to another federation now.',
};

interface ShutdownNoticeProps {
  shutdown: Shutdown;
  successor: FederationSuccessor | null;
  /** The successor's name, null if this observer doesn't follow it */
  successorName: string | null;
}

export function ShutdownNotice({ shutdown, successor, successorName }: ShutdownNoticeProps) {
  const date = shutdownDate(shutdown, 'long');
  return (
    <div role="status" className={`mb-6 sm:mb-8 rounded-lg border p-4 text-sm ${SHUTDOWN_STYLES[shutdown.stage].notice}`}>
      <p className="font-semibold">
        {shutdown.stage === 'over' ? `Shut down on ${date}` : `Shuts down on ${date}`}
      </p>
      <p className="mt-1">
        {shutdown.stage === 'over'
          ? `Its guardians announced that the federation shut down on ${date}. If you still hold funds in it, contact its guardians.`
          : `Its guardians announced that the federation shuts down ${timeUntilShutdown(shutdown)}. ${ADVICE[shutdown.stage]}`}
      </p>
      {successor && (
        <div className="mt-3">
          {successorName !== null ? (
            <p>
              They ask users to move to{' '}
              <Link to={`/federations/${successor.id}`} className="font-medium underline hover:no-underline">
                {successorName}
              </Link>
              .
            </p>
          ) : (
            <>
              <p>They ask users to move to the federation of this invite code:</p>
              <div className="mt-2 max-w-xl">
                <Copyable text={successor.invite} />
              </div>
            </>
          )}
        </div>
      )}
    </div>
  );
}
