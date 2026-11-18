/**
 * Numbers each sign-in, so that an answer to a request made under an earlier
 * sign-in can't end a later one: a 403 that arrives after the admin signed out
 * and in again, even with the same key, belongs to the old session.
 */
export class SessionTracker {
  private last = 0;
  private active: number | null = null;

  /** A sign-in succeeded; its number is the current session */
  begin(): number {
    this.active = ++this.last;
    return this.active;
  }

  /** The current session ended: signed out, or its access was lost */
  end(): void {
    this.active = null;
  }

  /** Whether `id` is the session signed in now */
  isCurrent(id: number | undefined): boolean {
    return id !== undefined && id === this.active;
  }
}
