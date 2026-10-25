const WORD_START = /[\s:/@._-]/;

/**
 * cmdk filter: every query term must occur in the item's value or keywords
 * (cmdk's default fuzzy scoring matches far too loosely for short queries).
 * Items where every term starts a word rank above mid-word matches.
 */
export function filterCommand(value: string, search: string, keywords: string[] = []): number {
  const text = [value, ...keywords].join(" ").toLowerCase();
  const terms = search.toLowerCase().split(/\s+/).filter(Boolean);
  let atWordStart = true;
  for (const term of terms) {
    let i = text.indexOf(term);
    if (i === -1) return 0;
    let found = false;
    while (i !== -1 && !found) {
      found = i === 0 || WORD_START.test(text.charAt(i - 1));
      i = text.indexOf(term, i + 1);
    }
    atWordStart &&= found;
  }
  return atWordStart ? 1 : 0.5;
}
