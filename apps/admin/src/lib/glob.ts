/**
 * Go `path.Match` semantics: `*` matches any run of non-'/' characters, `?`
 * one non-'/' character, `[...]` a character class (`[^...]` negated) and
 * `\` escapes. Used to find the branch protection that applies to a branch.
 */
export function pathMatch(pattern: string, name: string): boolean {
  let re = "^";
  for (let i = 0; i < pattern.length; i++) {
    const c = pattern.charAt(i);
    if (c === "*") re += "[^/]*";
    else if (c === "?") re += "[^/]";
    else if (c === "\\" && i + 1 < pattern.length) re += escape(pattern.charAt(++i));
    else if (c === "[") {
      const end = pattern.indexOf("]", i + 2);
      if (end === -1) return false; // malformed: Go returns ErrBadPattern
      // Class syntax ([a-z], [^x], \-escapes) is the same in JS regexes.
      re += `[${pattern.slice(i + 1, end)}]`;
      i = end;
    } else re += escape(c);
  }
  re += "$";
  try {
    return new RegExp(re).test(name);
  } catch {
    return false;
  }
}

function escape(c: string): string {
  return c.replace(/[.*+?^${}()|[\]\\/]/g, "\\$&");
}
