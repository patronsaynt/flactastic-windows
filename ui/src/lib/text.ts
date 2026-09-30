/** `localizedStandardCompare`: case/diacritic-insensitive, numeric-aware. */
const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" });

export const standardCompare = (a: string, b: string) => collator.compare(a, b);

/** `localizedCaseInsensitiveContains` */
export function containsCI(haystack: string | null | undefined, needle: string): boolean {
  if (!haystack) return false;
  return haystack.toLocaleLowerCase().includes(needle.toLocaleLowerCase());
}

export const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;
