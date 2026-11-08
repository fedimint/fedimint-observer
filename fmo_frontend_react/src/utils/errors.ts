/** The message of a thrown Error, or `fallback` for anything else that was thrown */
export function errorMessage(err: unknown, fallback: string): string {
  return err instanceof Error ? err.message : fallback;
}
