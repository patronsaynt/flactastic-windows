/** `ArtistResolver.explicitlySeparated`: NUL, ";" or " / " lists. */
export function explicitlySeparated(raw: string): string[] | null {
  for (const d of ["\u0000", ";", " / "]) {
    if (raw.includes(d)) {
      return raw
        .split(d)
        .map((s) => s.trim())
        .filter(Boolean);
    }
  }
  return null;
}

/** The chips an artist field starts with. */
export function artistChips(raw: string | null | undefined): string[] {
  if (!raw) return [];
  return explicitlySeparated(raw) ?? [raw];
}

/** Trimmed chips as one tag value: null, the single name, or `A ; B`. */
export function joinedChips(chips: string[]): string | null {
  const cleaned = chips.map((c) => c.trim()).filter(Boolean);
  if (!cleaned.length) return null;
  return cleaned.length === 1 ? cleaned[0] : cleaned.join(" ; ");
}
