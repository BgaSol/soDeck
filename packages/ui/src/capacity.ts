/** Capacity pressure, distinct from task completion. Inputs use exact decimal units. */
export interface CapacityInput { used: string; total: string }
export type CapacityLevel = 'normal' | 'warning' | 'critical';
export const capacityThresholds = { warning: 80, critical: 90 } as const;
const labels: Record<CapacityLevel, string> = {normal: '正常', warning: '偏高', critical: '紧张'};

export function capacityUsage(value: CapacityInput) {
  if (!/^\d+$/.test(value.used) || !/^\d+$/.test(value.total)) return null;
  const used = BigInt(value.used), total = BigInt(value.total);
  if (total === 0n || used > total) return null;
  const level: CapacityLevel = used * 100n >= total * BigInt(capacityThresholds.critical)
    ? 'critical' : used * 100n >= total * BigInt(capacityThresholds.warning) ? 'warning' : 'normal';
  // Divide before converting to Number: capacities can exceed Number.MAX_SAFE_INTEGER.
  const percent = Number(used * 1000n / total) / 10;
  return {percent, level, description: `${percent.toFixed(1)}% · ${labels[level]}`};
}
