import { useCallback, useRef, useState } from 'react';
import { ApiError } from '../../services/http';
import { errorMessage } from '../../utils/errors';
import { useAdminSession } from './useAdminSession';

interface Action<A extends unknown[], T> {
  /** The last successful answer */
  data: T | null;
  /** Why the latest run failed, fit to show */
  error: string | null;
  running: boolean;
  /** Runs it; answers the result, or null if it failed or a later run replaced it */
  run: (...args: A) => Promise<T | null>;
  /** Replaces the kept answer, e.g. after a change made elsewhere */
  setData: (update: (previous: T | null) => T | null) => void;
}

/**
 * Something an admin starts by hand. Every admin request asks the signer for a
 * signature, so none is started by rendering: an effect would ask again on
 * every remount.
 *
 * A 403 ends the session the request was made in: the page only offers what
 * the signed-in role may do, so the server refusing it means the key's access
 * is gone. An answer that arrives after a newer sign-in leaves that one alone.
 */
export function useAction<A extends unknown[], T>(action: (...args: A) => Promise<T>): Action<A, T> {
  const [state, setState] = useState<{ data: T | null; error: string | null; running: boolean }>({ data: null, error: null, running: false });
  const { admin, accessLost } = useAdminSession();
  // Only the latest run may change the state
  const latest = useRef(0);

  const run = async (...args: A) => {
    const current = ++latest.current;
    // The session this request is made in, fixed now: the run may outlive it
    const sessionId = admin?.sessionId;
    setState((previous) => ({ ...previous, error: null, running: true }));
    try {
      const data = await action(...args);
      if (current !== latest.current) return null;
      setState({ data, error: null, running: false });
      return data;
    } catch (err) {
      if (current === latest.current) setState((previous) => ({ ...previous, error: errorMessage(err), running: false }));
      if (err instanceof ApiError && err.status === 403) accessLost(sessionId);
      return null;
    }
  };

  const setData = useCallback((update: (previous: T | null) => T | null) => {
    setState((previous) => ({ ...previous, data: update(previous.data) }));
  }, []);

  return { ...state, run, setData };
}
