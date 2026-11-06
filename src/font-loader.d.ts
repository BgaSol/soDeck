export interface FontValue { source: "preset" | "local"; id?: string; family?: string }
export interface FontPreset { id: string; title: string; regular: string; bold: string }
export function loadFont(value: FontValue, presets: FontPreset[], resolve: (path: string) => string): Promise<string>;
