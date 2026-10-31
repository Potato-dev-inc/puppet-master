const RUN = /([\u3400-\u9fff\u3040-\u30ff\uf900-\ufaff]+)|([a-z0-9_]+)/g;

const segmenter = new Intl.Segmenter("zh", { granularity: "word" });

export function tokenize(text: string): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  const push = (token: string): void => {
    if (token.length === 0 || seen.has(token)) return;
    seen.add(token);
    out.push(token);
  };

  for (const match of text.toLowerCase().matchAll(RUN)) {
    const cjk = match[1];
    if (cjk !== undefined) {
      for (const part of segmenter.segment(cjk)) {
        if (part.isWordLike === true) push(part.segment);
      }
      const chars = Array.from(cjk);
      if (chars.length === 1) {
        const only = chars[0];
        if (only !== undefined) push(only);
      } else {
        for (let i = 0; i + 1 < chars.length; i++) {
          const a = chars[i];
          const b = chars[i + 1];
          if (a !== undefined && b !== undefined) push(a + b);
        }
      }
      continue;
    }
    const latin = match[2];
    if (latin !== undefined && latin.length >= 2) push(latin);
  }

  return out;
}
