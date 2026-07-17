/** Stable accent per speaker label so the eye can follow a voice down the
 * page — shared by the live transcript, presence header, and the
 * post-meeting segment editor. */
const chipPalette = [
  'bg-sky-500/15 text-sky-700 dark:text-sky-300',
  'bg-emerald-500/15 text-emerald-700 dark:text-emerald-300',
  'bg-amber-500/15 text-amber-700 dark:text-amber-300',
  'bg-violet-500/15 text-violet-700 dark:text-violet-300',
  'bg-rose-500/15 text-rose-700 dark:text-rose-300',
  'bg-cyan-500/15 text-cyan-700 dark:text-cyan-300'
];

/** Chip classes for `label`, colored by its position in `labels` (the
 * meeting's speaker labels in first-appearance order). */
export function chipClass(label: string, labels: string[]): string {
  const idx = labels.indexOf(label);
  return chipPalette[(idx >= 0 ? idx : 0) % chipPalette.length];
}
