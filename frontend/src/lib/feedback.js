// Where alpha testers send bug reports and impressions. Set VITE_FEEDBACK_URL
// when building to point at a form or an issue tracker; the default is the
// issue tracker of the public releases repository, which has to exist before a
// build goes to testers.
export const FEEDBACK_URL =
  import.meta.env?.VITE_FEEDBACK_URL || 'https://github.com/seriouslysahid/renkeu-releases/issues/new';
