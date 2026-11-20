/**
 * Engines supply actual navigation availability, independently of preview data.
 * @param {{preferred:boolean,mounted:boolean,usable:boolean,forcedColors:boolean,narrow:boolean}} state
 * @returns {'minimap'|'native'}
 */
export function resolveScrollRailMode({preferred,mounted,usable,forcedColors,narrow}) {
  return preferred && mounted && usable && !forcedColors && !narrow ? 'minimap' : 'native';
}
