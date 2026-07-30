// Which OS the app is running on. Resolved once at module load — the
// platform can't change under a running process — and safe in non-browser
// contexts (vitest, node tooling), where it reports Windows, the shipping
// default. Components branch on this for chrome and wording differences;
// everything functional stays behind the backend's platform seam.
export const isMac: boolean =
  typeof navigator !== 'undefined' &&
  /mac/i.test(navigator.platform || navigator.userAgent || '');
