// Loads the English Hunspell dictionary (~0.6 MB) and builds a speller from it.
// Imported dynamically, so it costs nothing until spell checking is first used.

import nspell from 'nspell';
import affUrl from 'dictionary-en-files/index.aff?url';
import dicUrl from 'dictionary-en-files/index.dic?url';

export async function loadEnglish() {
  const [aff, dic] = await Promise.all(
    [affUrl, dicUrl].map(async (url) => {
      const res = await fetch(url);
      if (!res.ok) throw new Error(`dictionary: ${res.status}`);
      return res.text();
    })
  );
  return nspell(aff, dic);
}
