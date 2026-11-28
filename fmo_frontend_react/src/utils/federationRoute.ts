/** A federation page's id and tab, or null for other paths and ids that do not decode */
export function federationRoute(pathname: string): { id: string; tab: string } | null {
  const match = /^\/federations\/([^/]+)\/?(.*)$/.exec(pathname);
  if (!match) return null;
  let id: string;
  try {
    id = decodeURIComponent(match[1]);
  } catch {
    return null;
  }
  return { id, tab: match[2].replace(/\/$/, '').toLowerCase() };
}
