export const fontPresets = [
  ['jetbrains-mono','JetBrains Mono'], ['source-code-pro','Source Code Pro'], ['ibm-plex-mono','IBM Plex Mono'],
].map(([id,title])=>({id,title,regular:`ui/fonts/${id}-regular.woff2`,bold:`ui/fonts/${id}-bold.woff2`}));
export const fontAsset = path => new URL(path.replace(/^ui\//,''), new URL('./',location.href)).href;
