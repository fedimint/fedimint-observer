import { createContext, useCallback, useContext, useMemo, useRef, useState, type ReactNode } from 'react';
import { adminApi, type AdminIdentity } from '../../services/adminApi';
import { ApiError } from '../../services/http';
import { browserSigner } from '../../services/nostrAuth';
import type { AdminSession } from '../../types/api';
import { errorMessage } from '../../utils/errors';
import { SessionTracker } from './lib/sessions';

/** A signed-in admin: who they are to the observer, and what signs their requests */
export interface SignedInAdmin {
  session: AdminSession;
  identity: AdminIdentity;
  /** Which sign-in this is; requests remember it, see `accessLost` */
  sessionId: number;
}

/** Why signing in did not work, and the key that was refused, to pass on to an owner */
interface SignInFailure {
  /** What happened, before the reason: "Could not sign in" */
  title: string;
  message: string;
  /** Set when the key itself was refused: an owner can grant it access */
  refusedKey: string | null;
}

interface AdminSessionValue {
  admin: SignedInAdmin | null;
  signingIn: boolean;
  failure: SignInFailure | null;
  signIn: () => Promise<void>;
  signOut: () => void;
  /**
   * The server refused a request made in session `sessionId`: the key's access
   * was revoked or ran out. Ignored unless that session is still the current
   * one, so a late answer can't sign out a newer session.
   */
  accessLost: (sessionId: number | undefined) => void;
}

const AdminSessionContext = createContext<AdminSessionValue | undefined>(undefined);

/**
 * Who is signed in as an admin, for the whole app.
 *
 * Nothing is stored: being signed in only means the page remembers which key to
 * ask the signer to sign with. Every admin request is signed and checked on its
 * own, so the server decides each time, and what the page shows is a convenience.
 * Reloading the page signs out.
 */
export function AdminSessionProvider({ children }: { children: ReactNode }) {
  const [admin, setAdmin] = useState<SignedInAdmin | null>(null);
  const [signingIn, setSigningIn] = useState(false);
  const [failure, setFailure] = useState<SignInFailure | null>(null);
  // Only the latest attempt may change the state: an earlier one could answer late
  const attempt = useRef(0);
  // The signed-in key, for callbacks that must not depend on the state
  const signedInKey = useRef<string | null>(null);
  const sessions = useRef(new SessionTracker());

  const signIn = useCallback(async () => {
    const current = ++attempt.current;
    setSigningIn(true);
    setFailure(null);
    let pubkey: string | null = null;
    try {
      const signer = browserSigner();
      pubkey = await signer.getPublicKey();
      const identity = { signer, pubkey };
      const session = await adminApi.session(identity);
      if (current === attempt.current) {
        signedInKey.current = pubkey;
        setFailure(null);
        setAdmin({ session, identity, sessionId: sessions.current.begin() });
      }
    } catch (err) {
      if (current !== attempt.current) return;
      const refused = err instanceof ApiError && err.status === 403;
      sessions.current.end();
      signedInKey.current = null;
      setAdmin(null);
      setFailure({ title: 'Could not sign in', message: errorMessage(err), refusedKey: refused ? pubkey : null });
    } finally {
      if (current === attempt.current) setSigningIn(false);
    }
  }, []);

  const signOut = useCallback(() => {
    attempt.current++;
    sessions.current.end();
    signedInKey.current = null;
    setAdmin(null);
    setFailure(null);
    setSigningIn(false);
  }, []);

  // A sign-in running meanwhile carries on: it may be with another key
  const accessLost = useCallback((sessionId: number | undefined) => {
    if (!sessions.current.isCurrent(sessionId)) return;
    sessions.current.end();
    setFailure({
      title: 'You were signed out',
      message: 'Your admin access was revoked or has expired.',
      refusedKey: signedInKey.current,
    });
    signedInKey.current = null;
    setAdmin(null);
  }, []);

  const value = useMemo(
    () => ({ admin, signingIn, failure, signIn, signOut, accessLost }),
    [admin, signingIn, failure, signIn, signOut, accessLost],
  );

  return <AdminSessionContext.Provider value={value}>{children}</AdminSessionContext.Provider>;
}

// eslint-disable-next-line react-refresh/only-export-components
export function useAdminSession() {
  const context = useContext(AdminSessionContext);
  if (!context) {
    throw new Error('useAdminSession must be used within an AdminSessionProvider');
  }
  return context;
}
