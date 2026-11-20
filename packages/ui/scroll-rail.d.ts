export function resolveScrollRailMode(state: {
  preferred: boolean;
  mounted: boolean;
  usable: boolean;
  forcedColors: boolean;
  narrow: boolean;
}): 'minimap' | 'native';
