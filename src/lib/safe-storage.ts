// The window's saved view choices (folder view, thumbnail size, theme…) go through here, never
// through localStorage directly: when the browser's storage is blocked, full or damaged it
// throws, and a view choice must never stop CasaPic from opening. A failed read gives null (the
// default is used); a failed write is skipped (the choice lasts until CasaPic closes).
// `npm run check:storage` fails if any other file touches localStorage or sessionStorage.

export const storage = {
  get(key: string): string | null {
    try {
      return window.localStorage.getItem(key);
    } catch {
      return null;
    }
  },
  set(key: string, value: string): void {
    try {
      window.localStorage.setItem(key, value);
    } catch {
      // storage blocked or full: keep the choice for this run only
    }
  },
};
